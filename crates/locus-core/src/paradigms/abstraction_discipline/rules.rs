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
                let Some(trait_path) = im.trait_path.as_deref() else {
                    // Inherent impl (`impl Foo`) — never counts toward a trait.
                    continue;
                };

                // Try full-symbol match first.
                if let Some(slot) = by_symbol.get_mut(trait_path) {
                    slot.count += 1;
                    if slot.first_self_ty.is_none() {
                        slot.first_self_ty = Some(im.self_ty.clone());
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
                        slot.first_self_ty = Some(im.self_ty.clone());
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
mod tests {
    use super::*;
    use locus_air::{AIR_SCHEMA_VERSION, AirFile, AirImpl, AirPackage, AirType, Visibility};

    fn trait_decl(symbol: &str, name: &str) -> AirItem {
        AirItem::Type(AirType {
            kind: TypeKind::Trait,
            name: name.into(),
            symbol: symbol.into(),
            visibility: Visibility::Public,
            fields: Vec::new(),
            variants: Vec::new(),
            derives: Vec::new(),
            attrs: Vec::new(),
            span: AirSpan::new("t.rs", 10, 20),
            doc: None,
        })
    }

    fn struct_decl(symbol: &str, name: &str) -> AirItem {
        AirItem::Type(AirType {
            kind: TypeKind::Struct,
            name: name.into(),
            symbol: symbol.into(),
            visibility: Visibility::Public,
            fields: Vec::new(),
            variants: Vec::new(),
            derives: Vec::new(),
            attrs: Vec::new(),
            span: AirSpan::new("t.rs", 1, 1),
            doc: None,
        })
    }

    fn impl_for(trait_path: Option<&str>, self_ty: &str) -> AirItem {
        AirItem::Impl(AirImpl {
            trait_path: trait_path.map(str::to_string),
            self_ty: self_ty.into(),
            method_names: Vec::new(),
            span: AirSpan::new("t.rs", 30, 40),
        })
    }

    fn air_with(items: Vec<AirItem>) -> AirWorkspace {
        AirWorkspace {
            schema_version: AIR_SCHEMA_VERSION,
            packages: vec![AirPackage {
                name: "x".into(),
                version: "0".into(),
                root_dir: "/".into(),
                files: vec![AirFile {
                    path: "t.rs".into(),
                    module_path: Some("x::core".into()),
                    items,
                    hints: Vec::new(),
                    parse_error: None,
                    line_count: 100,
                }],
            }],
            facts: Vec::new(),
        }
    }

    #[test]
    fn ab001_fires_on_trait_with_exactly_one_impl() {
        let air = air_with(vec![
            trait_decl("x::core::Manager", "Manager"),
            struct_decl("x::core::ConcreteManager", "ConcreteManager"),
            impl_for(Some("x::core::Manager"), "ConcreteManager"),
        ]);
        let section = AbSection::default();
        let diags = ab001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1, "expected one diag, got {diags:?}");
        assert_eq!(diags[0].rule_id, "AB001");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(diags[0].message.contains("Manager"));
        assert!(diags[0].message.contains("ConcreteManager"));
        // Span anchored at the trait declaration, not the impl.
        assert_eq!(diags[0].span.line_start, 10);
    }

    #[test]
    fn ab001_quiet_on_trait_with_zero_impls() {
        let air = air_with(vec![trait_decl("x::core::ApiSurface", "ApiSurface")]);
        let section = AbSection::default();
        assert!(ab001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ab001_quiet_on_trait_with_two_impls() {
        let air = air_with(vec![
            trait_decl("x::core::Clock", "Clock"),
            impl_for(Some("x::core::Clock"), "SystemClock"),
            impl_for(Some("x::core::Clock"), "TestClock"),
        ]);
        let section = AbSection::default();
        assert!(ab001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ab001_quiet_on_trait_with_three_impls() {
        let air = air_with(vec![
            trait_decl("x::core::Storage", "Storage"),
            impl_for(Some("x::core::Storage"), "MemStorage"),
            impl_for(Some("x::core::Storage"), "DiskStorage"),
            impl_for(Some("x::core::Storage"), "S3Storage"),
        ]);
        let section = AbSection::default();
        assert!(ab001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ab001_exempted_by_full_symbol_pattern() {
        let air = air_with(vec![
            trait_decl("x::ports::Clock", "Clock"),
            impl_for(Some("x::ports::Clock"), "SystemClock"),
        ]);
        let section = AbSection {
            accepted_single_impl_traits: vec!["x::ports::*".into()],
            ..AbSection::default()
        };
        assert!(ab001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ab001_exempted_by_short_name_pattern() {
        let air = air_with(vec![
            trait_decl("x::core::Manager", "Manager"),
            impl_for(Some("x::core::Manager"), "ConcreteManager"),
        ]);
        let section = AbSection {
            accepted_single_impl_traits: vec!["Manager".into()],
            ..AbSection::default()
        };
        assert!(ab001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ab001_agent_strict_elevates_to_fatal() {
        let air = air_with(vec![
            trait_decl("x::core::Manager", "Manager"),
            impl_for(Some("x::core::Manager"), "ConcreteManager"),
        ]);
        let section = AbSection::default();
        let diags = ab001(&air, &section, CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    #[test]
    fn ab001_inherent_impls_do_not_count_as_trait_impls() {
        // Trait declared but never `impl`-ed. The struct has an inherent
        // impl (trait_path = None) — that must NOT be counted as the trait's
        // implementation, so the trait stays at 0 impls and AB001 stays
        // silent (the 0-impl skip rule).
        let air = air_with(vec![
            trait_decl("x::core::Manager", "Manager"),
            struct_decl("x::core::Thing", "Thing"),
            impl_for(None, "Thing"),
        ]);
        let section = AbSection::default();
        assert!(ab001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ab001_matches_via_short_name_when_impl_uses_shorter_path() {
        // Trait is declared as `x::core::Manager` but the impl references
        // it by the bare `Manager` name (e.g. via `use`). Should still be
        // counted as the trait's only impl.
        let air = air_with(vec![
            trait_decl("x::core::Manager", "Manager"),
            impl_for(Some("Manager"), "ConcreteManager"),
        ]);
        let section = AbSection::default();
        let diags = ab001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1, "got {diags:?}");
        assert!(diags[0].message.contains("ConcreteManager"));
    }

    #[test]
    fn ab001_diagnostic_includes_rule_id_and_suggested_fix() {
        let air = air_with(vec![
            trait_decl("x::core::Manager", "Manager"),
            impl_for(Some("x::core::Manager"), "ConcreteManager"),
        ]);
        let section = AbSection::default();
        let diags = ab001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        let d = &diags[0];
        assert_eq!(d.rule_id, "AB001");
        assert!(d.suggested_fix.is_some());
        let fix = d.suggested_fix.as_ref().unwrap();
        assert!(fix.contains("ConcreteManager"));
        assert!(fix.contains("accepted_single_impl_traits"));
        // why list mentions impl count and the lone self_ty
        assert!(d.why.iter().any(|w| w.contains("impl count: 1")));
        assert!(d.why.iter().any(|w| w.contains("ConcreteManager")));
    }

    #[test]
    fn ab001_does_not_fire_on_external_trait_impls() {
        // Implementing an external trait (e.g. `std::fmt::Display`) for a
        // local type adds an Impl with a trait_path that doesn't match any
        // declared trait. AB001 should ignore it — it only reasons about
        // traits declared in the workspace.
        let air = air_with(vec![
            struct_decl("x::core::Thing", "Thing"),
            impl_for(Some("std::fmt::Display"), "Thing"),
        ]);
        let section = AbSection::default();
        assert!(ab001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ab001_fires_once_per_single_impl_trait() {
        // Two single-impl traits → two diagnostics; one two-impl trait stays
        // silent. Confirms per-trait emission across a mixed workspace.
        let air = air_with(vec![
            trait_decl("x::core::A", "A"),
            impl_for(Some("x::core::A"), "AImpl"),
            trait_decl("x::core::B", "B"),
            impl_for(Some("x::core::B"), "BImpl1"),
            impl_for(Some("x::core::B"), "BImpl2"),
            trait_decl("x::core::C", "C"),
            impl_for(Some("x::core::C"), "CImpl"),
        ]);
        let section = AbSection::default();
        let diags = ab001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 2, "got {diags:?}");
        let msgs: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
        assert!(msgs.iter().any(|m| m.contains("x::core::A")));
        assert!(msgs.iter().any(|m| m.contains("x::core::C")));
        assert!(!msgs.iter().any(|m| m.contains("x::core::B")));
    }

    // --- AB002 tests ---------------------------------------------------

    #[test]
    fn ab002_fires_on_struct_named_with_suspect_suffix() {
        // `UserManager` matches the seeded `*Manager` pattern.
        let air = air_with(vec![struct_decl("x::core::UserManager", "UserManager")]);
        let section = AbSection::default();
        let diags = ab002(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1, "got {diags:?}");
        assert_eq!(diags[0].rule_id, "AB002");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(diags[0].message.contains("x::core::UserManager"));
        assert!(diags[0].message.contains("*Manager"));
    }

    #[test]
    fn ab002_fires_on_trait_named_with_suspect_suffix() {
        // Trait variant of the same suspect pattern.
        let air = air_with(vec![trait_decl(
            "x::core::OrderProcessor",
            "OrderProcessor",
        )]);
        let section = AbSection::default();
        let diags = ab002(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1, "got {diags:?}");
        assert!(diags[0].message.contains("trait `x::core::OrderProcessor`"));
    }

    #[test]
    fn ab002_quiet_on_domain_named_types() {
        // Names that don't match any seeded pattern → silent.
        let air = air_with(vec![
            struct_decl("x::core::User", "User"),
            struct_decl("x::core::OrderBook", "OrderBook"),
            trait_decl("x::ports::Clock", "Clock"),
        ]);
        let section = AbSection::default();
        assert!(ab002(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ab002_exempted_by_accepted_abstraction_names() {
        // Suspect name but accepted via `accepted_abstraction_names`.
        let air = air_with(vec![struct_decl("x::core::AuthService", "AuthService")]);
        let section = AbSection {
            accepted_abstraction_names: vec!["x::core::AuthService".into()],
            ..AbSection::default()
        };
        assert!(ab002(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ab002_exempted_by_accepted_single_impl_traits() {
        // AB001's exemption list is also honored by AB002 — a port trait
        // already accepted as legitimately single-impl shouldn't double-fire.
        let air = air_with(vec![trait_decl("x::ports::Clock", "Clock")]);
        // Clock isn't a seeded suspect name; force a suspect pattern that
        // matches it via `*lock` → simulate a custom seeded list.
        let section = AbSection {
            suspect_abstraction_patterns: vec!["*Clock".into()],
            accepted_single_impl_traits: vec!["x::ports::*".into()],
            ..AbSection::default()
        };
        assert!(ab002(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ab002_silent_when_suspect_patterns_emptied() {
        // User explicitly empties the seeded list → silent regardless of names.
        let air = air_with(vec![struct_decl("x::core::UserManager", "UserManager")]);
        let section = AbSection {
            suspect_abstraction_patterns: Vec::new(),
            ..AbSection::default()
        };
        assert!(ab002(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ab002_agent_strict_elevates_to_fatal() {
        let air = air_with(vec![struct_decl("x::core::PaymentEngine", "PaymentEngine")]);
        let section = AbSection::default();
        let diags = ab002(&air, &section, CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }
}
