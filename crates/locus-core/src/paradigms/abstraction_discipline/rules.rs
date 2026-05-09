//! AB rule implementations.
//!
//! Implemented:
//! - [`ab001`]: trait declared in the workspace with exactly one impl. The
//!   "manager / processor / DataHandler" pattern from the spec — a trait
//!   added "in case other implementations exist someday" but in fact only
//!   ever points at one concrete type. Speculative abstraction.
//! - [`ab002`]: type (trait or struct) named after a generic role
//!   (`*Manager`, `*Service`, `*Processor`, …) without an accepted
//!   single-impl trait or accepted-abstraction-name mapping.
//!
//! Counting rules (AB001):
//! - 0 impls: skip (likely a library API surface, an unimplemented port, or
//!   an associated-type-only trait). The signal is too weak to fire on.
//! - 1 impl: fire AB001 unless the trait is exempted by
//!   `accepted_single_impl_traits`.
//! - 2+ impls: justified, skip.
//!
//! Trait/impl matching:
//! - A trait is identified by its [`AirType`] symbol (`crate::foo::Trait`).
//! - An impl's `trait_path` is rendered with the same clean type-text
//!   formatting as a type symbol (per AIR docs), so we match an impl to a
//!   declared trait when the impl's `trait_path` either equals the trait's
//!   full symbol or ends with `::<trait_short_name>`. The suffix fallback
//!   covers the common case where the trait is referenced inside an impl by
//!   a re-exported / shorter path than its declaration symbol.

use std::collections::HashMap;

use locus_air::{AirItem, AirSpan, AirWorkspace, TypeKind};

use super::lockfile_schema::{AbSection, matches_name_pattern, matches_pattern};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

/// AB001 — trait declared in the workspace has exactly one impl.
///
/// Severity: Warning by default; Fatal under `--agent-strict` via
/// [`CheckMode::elevate`]. Spec lists this as a heuristic: a single-impl
/// trait might still be a real port awaiting its second environment, so
/// blocking by default would be too aggressive. The escape hatch is the
/// `accepted_single_impl_traits` list.
///
/// Unlike MO/UT, AB001 fires even on a fully default section: the spec is
/// explicit that speculative abstraction should be flagged eagerly so users
/// have to examine each occurrence and explicitly accept legitimate ports.
pub fn ab001(air: &AirWorkspace, section: &AbSection, mode: CheckMode) -> Vec<Diagnostic> {
    // Step 1: collect every declared trait — symbol → (short name, span).
    // We keep declarations in source order across packages so diagnostic
    // output is stable.
    struct TraitDecl {
        symbol: String,
        name: String,
        span: AirSpan,
    }
    let mut traits: Vec<TraitDecl> = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                if let AirItem::Type(ty) = item
                    && ty.kind == TypeKind::Trait
                {
                    traits.push(TraitDecl {
                        symbol: ty.symbol.clone(),
                        name: ty.name.clone(),
                        span: ty.span.clone(),
                    });
                }
            }
        }
    }

    if traits.is_empty() {
        return Vec::new();
    }

    // Step 2: count impls per declared trait. We index by both full symbol
    // and short name so impls written against either form are caught.
    //
    // Tracking the lone impl's `self_ty` lets us name it in the diagnostic
    // — that's the most useful piece of context for the developer.
    #[derive(Default)]
    struct ImplCount {
        count: u32,
        first_self_ty: Option<String>,
    }
    let mut by_symbol: HashMap<&str, ImplCount> = HashMap::new();
    let mut by_short: HashMap<&str, ImplCount> = HashMap::new();
    // If two declared traits share a short name, the short-name index can't
    // disambiguate. In that case we only trust full-symbol matches; any
    // short-name fallback is suppressed by checking `ambiguous_shorts`.
    let mut short_name_seen: HashMap<&str, u32> = HashMap::new();
    for t in &traits {
        by_symbol.insert(t.symbol.as_str(), ImplCount::default());
        by_short.entry(t.name.as_str()).or_default();
        *short_name_seen.entry(t.name.as_str()).or_insert(0) += 1;
    }
    let ambiguous_shorts: std::collections::HashSet<&str> = short_name_seen
        .into_iter()
        .filter_map(|(k, v)| (v > 1).then_some(k))
        .collect();

    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                let AirItem::Impl(im) = item else { continue };
                let Some(trait_path) = im.interface.as_deref() else {
                    // Inherent impl (`impl Foo`) — never counts toward a trait.
                    continue;
                };

                // Try full-symbol match first.
                if let Some(slot) = by_symbol.get_mut(trait_path) {
                    slot.count += 1;
                    if slot.first_self_ty.is_none() {
                        slot.first_self_ty = Some(im.target_type.clone());
                    }
                    continue;
                }

                // Suffix fallback: an impl's `trait_path` ends with the
                // trait's short name (e.g. `ports::Clock` matches a trait
                // declared as `crate::ports::Clock`). Skip ambiguous shorts.
                let short = trait_path.rsplit("::").next().unwrap_or(trait_path);
                if ambiguous_shorts.contains(short) {
                    continue;
                }
                if let Some(slot) = by_short.get_mut(short) {
                    slot.count += 1;
                    if slot.first_self_ty.is_none() {
                        slot.first_self_ty = Some(im.target_type.clone());
                    }
                }
            }
        }
    }

    // Step 3: emit one diagnostic per single-impl trait, in declaration order.
    let mut out = Vec::new();
    for t in &traits {
        // Prefer the full-symbol slot, but fall back to short-name when full
        // symbol got zero hits (because every impl referenced the trait by
        // a shorter path).
        let by_sym = by_symbol.get(t.symbol.as_str());
        let by_sht = if ambiguous_shorts.contains(t.name.as_str()) {
            None
        } else {
            by_short.get(t.name.as_str())
        };
        let (count, lone_self_ty) = match (by_sym, by_sht) {
            (Some(s), _) if s.count > 0 => (s.count, s.first_self_ty.clone()),
            (_, Some(h)) => (h.count, h.first_self_ty.clone()),
            (Some(s), None) => (s.count, s.first_self_ty.clone()),
            (None, None) => (0, None),
        };

        if count != 1 {
            continue;
        }

        // Exemption check: full symbol or short name, against any pattern.
        let exempted = section
            .accepted_single_impl_traits
            .iter()
            .any(|pat| matches_pattern(pat, &t.symbol) || matches_pattern(pat, &t.name));
        if exempted {
            continue;
        }

        let lone = lone_self_ty.unwrap_or_else(|| "<unknown>".to_string());
        out.push(Diagnostic {
            rule_id: "AB001".to_string(),
            severity: mode.elevate(Severity::Warning),
            span: t.span.clone(),
            concept: None,
            message: format!(
                "trait `{}` has exactly one impl (`{}`) — likely speculative abstraction",
                t.symbol, lone
            ),
            why: vec![
                format!("trait symbol `{}`", t.symbol),
                "impl count: 1".into(),
                format!("only impl is for `{lone}`"),
                "single-impl traits with no boundary role are usually \
                 speculative abstraction (the spec's manager / processor / \
                 DataHandler pattern)"
                    .into(),
            ],
            suggested_fix: Some(format!(
                "remove the trait and use `{lone}` directly, or — if this is a \
                 genuine port awaiting a second impl (e.g. a test double in a \
                 separate environment) — add `{}` to \
                 `paradigms.AB.accepted_single_impl_traits` in `locus.lock`",
                t.symbol
            )),
        });
    }

    out
}

/// AB002 — manager/processor abstraction without accepted role.
///
/// For each `AirItem::Type` with `TypeKind::Trait` or `TypeKind::Struct`,
/// fire when the type's short name matches any pattern in
/// `section.suspect_abstraction_patterns` AND the type is NOT exempted by
/// either:
/// - `section.accepted_single_impl_traits` — the existing AB001 acceptance
///   list, reused so a port trait already accepted as legitimately
///   single-impl (`Clock`, etc.) doesn't re-fire under AB002, OR
/// - `section.accepted_abstraction_names` — the new AB002-specific
///   acceptance list, for cases where a `*Service` / `*Manager` name is
///   genuinely the right domain term (rare, but real).
///
/// Match semantics: each acceptance pattern is checked against both the
/// type's full symbol (`crate::core::UserManager`) and its short name
/// (`UserManager`), mirroring AB001's exemption shape.
///
/// Severity: Warning by default; Fatal under `--agent-strict`.
///
/// Lockfile-driven defaults: the section ships with a non-empty
/// `suspect_abstraction_patterns` (the spec's seeded role-name list), so
/// AB002 fires immediately on un-onboarded code. Users curate the
/// patterns or accept individual names rather than starting from silence
/// — abstraction discipline is the spec's "examine and decide
/// deliberately" paradigm, not a paradigm where un-configured means
/// silent.
pub fn ab002(air: &AirWorkspace, section: &AbSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.suspect_abstraction_patterns.is_empty() {
        // User explicitly emptied the list → silent. Same convention as
        // disabling a paradigm via empty configuration.
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                let AirItem::Type(ty) = item else { continue };
                if !matches!(ty.kind, TypeKind::Trait | TypeKind::Struct) {
                    continue;
                }

                // Find the first matching suspect pattern; recording it
                // gives the diagnostic a useful "why" anchor.
                let Some(matched_pattern) = section
                    .suspect_abstraction_patterns
                    .iter()
                    .find(|pat| matches_name_pattern(pat, &ty.name))
                else {
                    continue;
                };

                // Exemption check: full symbol or short name, against
                // either the AB001 single-impl-traits list or the AB002
                // accepted-names list.
                let exempted_by_single_impl = section
                    .accepted_single_impl_traits
                    .iter()
                    .any(|pat| matches_pattern(pat, &ty.symbol) || matches_pattern(pat, &ty.name));
                let exempted_by_name = section
                    .accepted_abstraction_names
                    .iter()
                    .any(|pat| matches_pattern(pat, &ty.symbol) || matches_pattern(pat, &ty.name));
                if exempted_by_single_impl || exempted_by_name {
                    continue;
                }

                let kind_word = match ty.kind {
                    TypeKind::Trait => "trait",
                    TypeKind::Struct => "struct",
                    _ => unreachable!(),
                };

                out.push(Diagnostic {
                    rule_id: "AB002".to_string(),
                    severity: mode.elevate(Severity::Warning),
                    span: ty.span.clone(),
                    concept: None,
                    message: format!(
                        "{kind_word} `{}` is named after a generic role (`{matched_pattern}`) \
                         without an accepted abstraction record — likely \
                         manager/processor/coordinator anti-pattern",
                        ty.symbol
                    ),
                    why: vec![
                        format!("{kind_word} symbol `{}`", ty.symbol),
                        format!(
                            "short name `{}` matches suspect pattern `{matched_pattern}`",
                            ty.name
                        ),
                        "neither `accepted_single_impl_traits` nor `accepted_abstraction_names` \
                         covers this symbol"
                            .into(),
                    ],
                    suggested_fix: Some(format!(
                        "rename the type after the *domain concept it owns* (e.g. `UserManager` \
                         → `UserDirectory`), or — if this name is genuinely the right one — \
                         add `{}` to `paradigms.AB.accepted_abstraction_names` in `locus.lock`",
                        ty.symbol
                    )),
                });
            }
        }
    }
    out
}

#[cfg(test)]
#[path = "rules_tests.rs"]
mod tests;
