//! DA rule implementations.
//!
//! Implemented:
//! - [`da001`]: trait declared with exactly one implementation in the
//!   workspace and no accepted port role — speculative variation surface.
//! - [`da002`]: function whose name matches `factory_name_patterns`
//!   (`create_*`, `make_*`, `*_factory`, `build_*`) but only ever
//!   constructs a single concrete type — the abstraction has zero
//!   variation, it's a renamed constructor.
//! - [`da007`]: enum whose name matches `strategy_name_patterns`
//!   (`*Strategy`, `*Mode`, `*Policy`) but has exactly one variant — a
//!   stub abstraction with no actual variation.
//!
//! All DA rules are gated on the section's `enabled` flag. With the seed
//! pattern lists shipping non-empty, the only opt-in step a user makes is
//! flipping `paradigms.DA.enabled = true`.

use std::collections::BTreeMap;

use locus_air::{ActionKind, AirFunction, AirItem, AirType, AirWorkspace, TypeKind};

use super::lockfile_schema::{DaSection, matches_name_glob};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

fn da001_diagnostic(decl: &AirType, mode: CheckMode) -> Diagnostic {
    Diagnostic {
        rule_id: "DA001".to_string(),
        severity: mode.elevate(Severity::Warning),
        span: decl.span.clone(),
        concept: Some(decl.name.clone()),
        message: format!(
            "trait `{}` has exactly one implementation — abstraction may be speculative",
            decl.name
        ),
        why: vec![
            format!("trait declared at `{}`", decl.symbol),
            "exactly one `impl` block found in the workspace".into(),
            "Demand-Driven Architecture: an abstraction is justified by present \
             demand (multiple impls, accepted port role, generated boundary, …); \
             a trait with one impl and no accepted role is variation rent without \
             variation"
                .into(),
        ],
        suggested_fix: Some(format!(
            "if this trait is a real port / accepted single-impl seam, add `\"{}\"` \
             (or its full symbol `\"{}\"`) to `paradigms.DA.accepted_single_impl` in \
             `locus.lock`; otherwise inline the trait's contract into the concrete \
             type and call sites",
            decl.name, decl.symbol
        )),
    }
}

/// Count impl blocks per declared trait. Returns a map of symbol → impl count.
fn count_da001_impls<'a>(
    air: &AirWorkspace,
    traits: &'a BTreeMap<String, &AirType>,
) -> BTreeMap<&'a str, u32> {
    let mut impl_counts: BTreeMap<&str, u32> =
        traits.keys().map(|k| (k.as_str(), 0u32)).collect();
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                let AirItem::Impl(imp) = item else { continue };
                let Some(raw) = imp.interface.as_deref() else {
                    continue;
                };
                let normalized = strip_generics(raw);
                let short = last_segment(normalized);
                for (sym, decl) in traits {
                    if normalized == sym.as_str() || short == decl.name.as_str() {
                        if let Some(c) = impl_counts.get_mut(sym.as_str()) {
                            *c += 1;
                        }
                        break;
                    }
                }
            }
        }
    }
    impl_counts
}

/// DA001 — trait with exactly one implementation and no accepted port role.
///
/// Walks every `AirItem::Type` whose `kind == Trait`, counts the
/// `AirItem::Impl` blocks in the workspace whose `trait_path` resolves to
/// that trait (matching either the trait's full symbol or its short name —
/// the language adapter renders trait paths as written, which is sometimes
/// `crate::foo::Trait` and sometimes just `Trait` after a `use`). Fires when
/// the count is exactly one.
///
/// Zero-impl traits are explicitly out of scope here — they belong to a
/// future "orphaned API surface" rule (DA002 or similar), and the failure
/// mode is different (truly unused, not speculatively-abstracted).
///
/// Severity: Warning by default; `--agent-strict` elevates to Fatal via
/// [`CheckMode::elevate`]. The spec puts DA in the warn-then-discuss tier:
/// some single-impl traits (real ports, generated code, intentional seams)
/// are fine, and the user accepts them by adding to `accepted_single_impl`
/// rather than by deleting the trait.
///
/// Lockfile-driven silence: the section's `enabled` flag is `false` by
/// default. DA001 short-circuits in that case so DA never bombards an
/// un-onboarded project — same convention as DG/MO/CX.
pub fn da001(air: &AirWorkspace, section: &DaSection, mode: CheckMode) -> Vec<Diagnostic> {
    if !section.enabled {
        return Vec::new();
    }
    let mut traits: BTreeMap<String, &AirType> = BTreeMap::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                if let AirItem::Type(t) = item
                    && t.kind == TypeKind::Trait
                {
                    traits.insert(t.symbol.clone(), t);
                }
            }
        }
    }
    if traits.is_empty() {
        return Vec::new();
    }
    let impl_counts = count_da001_impls(air, &traits);
    let mut out = Vec::new();
    for (sym, decl) in &traits {
        let count = impl_counts.get(sym.as_str()).copied().unwrap_or(0);
        if count != 1 {
            continue;
        }
        if section.is_accepted(sym, &decl.name) {
            continue;
        }
        out.push(da001_diagnostic(decl, mode));
    }
    out
}

/// Strip a trailing `<...>` generic argument list from a rendered path so
/// `Foo<T>` and `Foo` compare equal. Keeps the input unchanged when there's
/// no `<`.
fn strip_generics(path: &str) -> &str {
    match path.find('<') {
        Some(idx) => path[..idx].trim_end(),
        None => path,
    }
}

/// Return the last `::`-separated segment of a path. Used for matching a
/// trait declaration's `name` against a `trait_path` rendered as a short
/// name after a `use` import.
fn last_segment(path: &str) -> &str {
    path.rsplit("::").next().unwrap_or(path)
}

/// DA002 — single-construct factory function.
///
/// For every `AirItem::Function` whose `name` matches any pattern in
/// `section.factory_name_patterns`, count the `AirItem::TruthAction`
/// entries with `action == Construct` and `function == Some(func.symbol)`.
/// Fires when the count is exactly 1: a `create_*` / `make_*` /
/// `build_*` / `*_factory` that only ever constructs one type is just a
/// renamed constructor — the factory abstraction earned no variation.
///
/// Zero-construct factories (the function's body has no `Construct`
/// truth-action — e.g. it's a façade that delegates) and multi-construct
/// factories (real variation, justified) are both quiet.
///
/// Severity: Warning by default; Fatal under `--agent-strict`.
///
/// Stays silent when `factory_name_patterns` is empty, when
/// `section.enabled == false`, or when the workspace has no functions.

/// Build a per-function-symbol → `Da002ConstructStats` index from all
/// `Construct` truth-actions in the workspace. Functions with no `Construct`
/// actions are absent from the map.
fn build_da002_construct_index(air: &AirWorkspace) -> BTreeMap<&str, Da002ConstructStats> {
    let mut by_fn: BTreeMap<&str, Da002ConstructStats> = BTreeMap::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                let AirItem::TruthAction(act) = item else {
                    continue;
                };
                if act.action != ActionKind::Construct {
                    continue;
                }
                let Some(fn_sym) = act.function.as_deref() else {
                    continue;
                };
                let entry = by_fn.entry(fn_sym).or_insert_with(|| Da002ConstructStats {
                    count: 0,
                    first_target: act.target.clone(),
                });
                entry.count += 1;
            }
        }
    }
    by_fn
}

pub fn da002(air: &AirWorkspace, section: &DaSection, mode: CheckMode) -> Vec<Diagnostic> {
    if !section.enabled || section.factory_name_patterns.is_empty() {
        return Vec::new();
    }

    let by_fn = build_da002_construct_index(air);

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                let AirItem::Function(func) = item else {
                    continue;
                };
                let Some(matched_pattern) = section
                    .factory_name_patterns
                    .iter()
                    .find(|p| matches_name_glob(p, &func.name))
                else {
                    continue;
                };
                let Some(stats) = by_fn.get(func.symbol.as_str()) else {
                    continue; // 0 constructions — out of scope for DA002
                };
                if stats.count != 1 {
                    continue;
                }
                out.push(diagnostic_da002(func, matched_pattern, &stats.first_target, mode));
            }
        }
    }
    out
}

/// Construct-action accumulator for DA002.
struct Da002ConstructStats {
    count: u32,
    first_target: String,
}

fn diagnostic_da002(
    func: &AirFunction,
    matched_pattern: &str,
    target: &str,
    mode: CheckMode,
) -> Diagnostic {
    Diagnostic {
        rule_id: "DA002".to_string(),
        severity: mode.elevate(Severity::Warning),
        span: func.span.clone(),
        concept: Some(func.name.clone()),
        message: format!(
            "factory function `{}` constructs exactly one type — \
             abstraction may be speculative",
            func.name
        ),
        why: vec![
            format!("function `{}` (`{}`)", func.name, func.symbol),
            format!("name matches factory pattern `{matched_pattern}`"),
            "exactly one `Construct` truth-action recorded with this \
             function as its enclosing scope"
                .into(),
            format!("constructs `{target}`"),
            "Demand-Driven Architecture: a factory's job is to *vary* \
             construction; one target = renamed constructor"
                .into(),
        ],
        suggested_fix: Some(format!(
            "if `{name}` is a renamed constructor, replace it with \
             `{target}::new(...)` (or the type's accepted constructor) \
             and delete the wrapper. If `{name}` will gain variation \
             soon, narrow `paradigms.DA.factory_name_patterns` so its \
             current shape isn't flagged.",
            name = func.name,
            target = target,
        )),
    }
}

fn da007_diagnostic(ty: &AirType, matched_pattern: &str, only_variant: &str, mode: CheckMode) -> Diagnostic {
    Diagnostic {
        rule_id: "DA007".to_string(),
        severity: mode.elevate(Severity::Warning),
        span: ty.span.clone(),
        concept: Some(ty.name.clone()),
        message: format!(
            "strategy-shaped enum `{}` has exactly one variant \
             (`{only_variant}`) — abstraction is a stub",
            ty.name
        ),
        why: vec![
            format!("enum `{}` (`{}`)", ty.name, ty.symbol),
            format!("name matches strategy pattern `{matched_pattern}`"),
            format!("single variant: `{only_variant}` (no actual variation)"),
            "Demand-Driven Architecture: a 1-variant enum \
             carries no decision — it's an unstarted point of \
             variation"
                .into(),
        ],
        suggested_fix: Some(format!(
            "if there is no real variation, inline the only \
             variant `{only_variant}` at call sites and delete the \
             enum. If a second variant is expected soon, \
             narrow `paradigms.DA.strategy_name_patterns` so \
             `{name}` isn't matched until the second variant \
             lands.",
            name = ty.name,
        )),
    }
}

/// DA007 — single-variant strategy enum.
///
/// For every `AirItem::Type` whose `kind == Enum` and whose `name`
/// matches any pattern in `section.strategy_name_patterns`, fire when
/// `variants.len() == 1`. A 1-variant `*Strategy` / `*Mode` / `*Policy`
/// is a stub: there's no actual variation, just speculative shape.
///
/// Severity: Warning by default; Fatal under `--agent-strict`.
///
/// Stays silent when `strategy_name_patterns` is empty, when
/// `section.enabled == false`, or when no enums match.
pub fn da007(air: &AirWorkspace, section: &DaSection, mode: CheckMode) -> Vec<Diagnostic> {
    if !section.enabled || section.strategy_name_patterns.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                let AirItem::Type(ty) = item else {
                    continue;
                };
                if ty.kind != TypeKind::Enum {
                    continue;
                }
                let Some(matched_pattern) = section
                    .strategy_name_patterns
                    .iter()
                    .find(|p| matches_name_glob(p, &ty.name))
                else {
                    continue;
                };
                if ty.variants.len() != 1 {
                    continue;
                }
                let only_variant = &ty.variants[0].name;
                out.push(da007_diagnostic(ty, matched_pattern, only_variant, mode));
            }
        }
    }
    out
}

#[cfg(test)]
#[path = "rules_tests.rs"]
mod rules_tests;
