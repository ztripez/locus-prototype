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

    // First pass: every trait declaration in the workspace, keyed by symbol.
    // We carry the trait's name and span alongside the symbol so the
    // diagnostic can anchor on the trait's source location.
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

    // Second pass: count impl blocks per trait. We match an `AirImplBlock.interface`
    // to a declared trait by either:
    // - exact symbol equality (`my_crate::ports::Clock` matches symbol), OR
    // - last-segment name equality (the trait was imported and used as `Clock`).
    // The short-name fallback is necessary because `render_path` reflects the
    // path as written, not as resolved — `use foo::Clock; impl Clock for X`
    // emits `trait_path = "Clock"`. A name collision between two traits with
    // the same short name would over-count, which is acceptable for a Warning:
    // the diagnostic invites the user to inspect, not to enforce silently.
    let mut impl_counts: BTreeMap<&str, u32> = traits.keys().map(|k| (k.as_str(), 0u32)).collect();
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                let AirItem::Impl(imp) = item else { continue };
                let Some(raw) = imp.interface.as_deref() else {
                    continue; // inherent impl
                };
                let normalized = strip_generics(raw);
                let short = last_segment(normalized);
                for (sym, decl) in &traits {
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

    let mut out = Vec::new();
    for (sym, decl) in &traits {
        let count = impl_counts.get(sym.as_str()).copied().unwrap_or(0);
        if count != 1 {
            continue; // 0 → out of scope (future rule); >=2 → real variation, fine
        }
        if section.is_accepted(sym, &decl.name) {
            continue; // user has accepted this single-impl trait
        }

        out.push(Diagnostic {
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
        });
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
pub fn da002(air: &AirWorkspace, section: &DaSection, mode: CheckMode) -> Vec<Diagnostic> {
    if !section.enabled || section.factory_name_patterns.is_empty() {
        return Vec::new();
    }

    // Index Construct truth-actions by enclosing function symbol. Each
    // entry's value is the count + a sample target (used for the
    // diagnostic's why).
    struct ConstructStats {
        count: u32,
        first_target: String,
    }
    let mut by_fn: BTreeMap<&str, ConstructStats> = BTreeMap::new();
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
                let entry = by_fn.entry(fn_sym).or_insert_with(|| ConstructStats {
                    count: 0,
                    first_target: act.target.clone(),
                });
                entry.count += 1;
            }
        }
    }

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
                out.push(diagnostic_da002(
                    func,
                    matched_pattern,
                    &stats.first_target,
                    mode,
                ));
            }
        }
    }
    out
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
                out.push(Diagnostic {
                    rule_id: "DA007".to_string(),
                    severity: mode.elevate(Severity::Warning),
                    span: ty.span.clone(),
                    concept: Some(ty.name.clone()),
                    message: format!(
                        "strategy-shaped enum `{}` has exactly one variant \
                         (`{}`) — abstraction is a stub",
                        ty.name, only_variant
                    ),
                    why: vec![
                        format!("enum `{}` (`{}`)", ty.name, ty.symbol),
                        format!("name matches strategy pattern `{matched_pattern}`"),
                        format!("single variant: `{}` (no actual variation)", only_variant),
                        "Demand-Driven Architecture: a 1-variant enum \
                         carries no decision — it's an unstarted point of \
                         variation"
                            .into(),
                    ],
                    suggested_fix: Some(format!(
                        "if there is no real variation, inline the only \
                         variant `{variant}` at call sites and delete the \
                         enum. If a second variant is expected soon, \
                         narrow `paradigms.DA.strategy_name_patterns` so \
                         `{name}` isn't matched until the second variant \
                         lands.",
                        variant = only_variant,
                        name = ty.name,
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
    use locus_air::{
        AIR_SCHEMA_VERSION, AirFile, AirImplBlock, AirItem, AirPackage, AirSpan, AirType, TypeKind,
        Visibility,
    };

    fn trait_decl(name: &str, symbol: &str) -> AirItem {
        AirItem::Type(AirType {
            kind: TypeKind::Trait,
            name: name.into(),
            symbol: symbol.into(),
            visibility: Visibility::Public,
            fields: Vec::new(),
            variants: Vec::new(),
            decorators: Vec::new(),
            symbol_segments: Vec::new(),
            span: AirSpan::new("t.rs", 1, 1),
            doc: None,
        })
    }

    fn struct_decl(name: &str, symbol: &str) -> AirItem {
        AirItem::Type(AirType {
            kind: TypeKind::Struct,
            name: name.into(),
            symbol: symbol.into(),
            visibility: Visibility::Public,
            fields: Vec::new(),
            variants: Vec::new(),
            decorators: Vec::new(),
            symbol_segments: Vec::new(),
            span: AirSpan::new("t.rs", 1, 1),
            doc: None,
        })
    }

    fn impl_for(trait_path: Option<&str>, self_ty: &str) -> AirItem {
        AirItem::Impl(AirImplBlock {
            interface: trait_path.map(str::to_string),
            target_type: self_ty.into(),
            method_names: Vec::new(),
            dispatch: locus_air::ImplDispatch::Static,
            span: AirSpan::new("t.rs", 1, 1),
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
                    module_path: Some("x".into()),
                    items,
                    hints: Vec::new(),
                    parse_error: None,
                    line_count: 1,
                }],
            }],
            facts: Vec::new(),
        }
    }

    fn enabled() -> DaSection {
        DaSection {
            enabled: true,
            accepted_single_impl: Vec::new(),
            ..DaSection::default()
        }
    }

    // ---- positive case ----

    #[test]
    fn da001_fires_on_trait_with_one_implementation() {
        let air = air_with(vec![
            trait_decl("Clock", "x::Clock"),
            struct_decl("SystemClock", "x::SystemClock"),
            impl_for(Some("Clock"), "SystemClock"),
        ]);
        let diags = da001(&air, &enabled(), CheckMode::Human);
        assert_eq!(diags.len(), 1, "got {diags:?}");
        assert_eq!(diags[0].rule_id, "DA001");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(diags[0].message.contains("Clock"));
        assert_eq!(diags[0].concept.as_deref(), Some("Clock"));
    }

    // ---- negative cases ----

    #[test]
    fn da001_quiet_on_trait_with_two_implementations() {
        // Two impls = real variation; abstraction earns its rent.
        let air = air_with(vec![
            trait_decl("Notifier", "x::Notifier"),
            impl_for(Some("Notifier"), "EmailNotifier"),
            impl_for(Some("Notifier"), "SmsNotifier"),
        ]);
        assert!(da001(&air, &enabled(), CheckMode::Human).is_empty());
    }

    #[test]
    fn da001_quiet_on_trait_with_zero_implementations() {
        // Zero impls is a separate failure mode (orphaned surface) — outside
        // DA001's scope. DA001 fires only on the single-impl shape.
        let air = air_with(vec![trait_decl("Untouched", "x::Untouched")]);
        assert!(da001(&air, &enabled(), CheckMode::Human).is_empty());
    }

    #[test]
    fn da001_silent_when_section_disabled_default() {
        // No `enabled = true` → DA never fires, even with a slam-dunk
        // single-impl trait. Same lockfile-driven UX as DG/MO/CX.
        let air = air_with(vec![
            trait_decl("Clock", "x::Clock"),
            impl_for(Some("Clock"), "SystemClock"),
        ]);
        assert!(da001(&air, &DaSection::default(), CheckMode::Human).is_empty());
    }

    // ---- exemption ----

    #[test]
    fn da001_quiet_when_trait_in_accepted_list_by_short_name() {
        let air = air_with(vec![
            trait_decl("Clock", "x::Clock"),
            impl_for(Some("Clock"), "SystemClock"),
        ]);
        let section = DaSection {
            enabled: true,
            accepted_single_impl: vec!["Clock".into()],
            ..DaSection::default()
        };
        assert!(da001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn da001_quiet_when_trait_in_accepted_list_by_full_symbol() {
        let air = air_with(vec![
            trait_decl("Clock", "my_crate::ports::Clock"),
            impl_for(Some("my_crate::ports::Clock"), "SystemClock"),
        ]);
        let section = DaSection {
            enabled: true,
            accepted_single_impl: vec!["my_crate::ports::Clock".into()],
            ..DaSection::default()
        };
        assert!(da001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn da001_quiet_when_trait_matched_by_wildcard_namespace_pattern() {
        let air = air_with(vec![
            trait_decl("Cache", "my_crate::infra::Cache"),
            impl_for(Some("my_crate::infra::Cache"), "RedisCache"),
        ]);
        let section = DaSection {
            enabled: true,
            accepted_single_impl: vec!["my_crate::infra::*".into()],
            ..DaSection::default()
        };
        assert!(da001(&air, &section, CheckMode::Human).is_empty());
    }

    // ---- agent-strict elevation ----

    #[test]
    fn da001_agent_strict_elevates_warning_to_fatal() {
        let air = air_with(vec![
            trait_decl("Clock", "x::Clock"),
            impl_for(Some("Clock"), "SystemClock"),
        ]);
        let diags = da001(&air, &enabled(), CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].severity,
            Severity::Fatal,
            "agent-strict should elevate Warning to Fatal"
        );
    }

    // ---- edge cases ----

    #[test]
    fn da001_recognises_full_symbol_and_short_name_impls_as_same_trait() {
        // One impl writes the full path, the other uses the short name after
        // a `use`. Both should resolve to the same trait → count == 2 → quiet.
        let air = air_with(vec![
            trait_decl("Notifier", "my_crate::Notifier"),
            impl_for(Some("my_crate::Notifier"), "Email"),
            impl_for(Some("Notifier"), "Sms"),
        ]);
        assert!(
            da001(&air, &enabled(), CheckMode::Human).is_empty(),
            "two impls — one by full symbol, one by short name — should both count"
        );
    }

    #[test]
    fn da001_strips_generic_args_when_matching_trait_path() {
        // `impl Trait<X> for Y` should still be counted as an impl of `Trait`.
        let air = air_with(vec![
            trait_decl("Convert", "x::Convert"),
            impl_for(Some("Convert<u32>"), "Wrapper"),
        ]);
        let diags = da001(&air, &enabled(), CheckMode::Human);
        assert_eq!(
            diags.len(),
            1,
            "generic-arg impl should still match; got {diags:?}"
        );
    }

    #[test]
    fn da001_ignores_inherent_impls() {
        // Inherent `impl SystemClock {}` (no `trait_path`) must not count
        // toward Clock's impl tally.
        let air = air_with(vec![
            trait_decl("Clock", "x::Clock"),
            impl_for(None, "SystemClock"), // inherent impl
            impl_for(Some("Clock"), "SystemClock"),
        ]);
        let diags = da001(&air, &enabled(), CheckMode::Human);
        assert_eq!(diags.len(), 1, "inherent impl should not raise the count");
    }

    #[test]
    fn da001_counts_impls_across_files_and_packages() {
        // Trait declared in package x, single impl in package y. The rule
        // must walk all packages, not just the declaring one.
        let air = AirWorkspace {
            schema_version: AIR_SCHEMA_VERSION,
            packages: vec![
                AirPackage {
                    name: "x".into(),
                    version: "0".into(),
                    root_dir: "/x".into(),
                    files: vec![AirFile {
                        path: "x/lib.rs".into(),
                        module_path: Some("x".into()),
                        items: vec![trait_decl("Clock", "x::Clock")],
                        hints: Vec::new(),
                        parse_error: None,
                        line_count: 1,
                    }],
                },
                AirPackage {
                    name: "y".into(),
                    version: "0".into(),
                    root_dir: "/y".into(),
                    files: vec![AirFile {
                        path: "y/lib.rs".into(),
                        module_path: Some("y".into()),
                        items: vec![impl_for(Some("x::Clock"), "SystemClock")],
                        hints: Vec::new(),
                        parse_error: None,
                        line_count: 1,
                    }],
                },
            ],
            facts: Vec::new(),
        };
        let diags = da001(&air, &enabled(), CheckMode::Human);
        assert_eq!(diags.len(), 1, "cross-package impl should count");
        assert!(diags[0].why.iter().any(|w| w.contains("x::Clock")));
    }

    #[test]
    fn da001_one_diagnostic_per_violating_trait() {
        // Two distinct single-impl traits → two diagnostics. Independent rule
        // application per trait.
        let air = air_with(vec![
            trait_decl("Clock", "x::Clock"),
            impl_for(Some("Clock"), "SystemClock"),
            trait_decl("Logger", "x::Logger"),
            impl_for(Some("Logger"), "StdoutLogger"),
        ]);
        let diags = da001(&air, &enabled(), CheckMode::Human);
        assert_eq!(diags.len(), 2, "got {diags:?}");
        let concepts: Vec<&str> = diags.iter().filter_map(|d| d.concept.as_deref()).collect();
        assert!(concepts.contains(&"Clock"));
        assert!(concepts.contains(&"Logger"));
    }

    #[test]
    fn da001_diagnostic_anchors_on_trait_declaration_span() {
        // The diagnostic should point at the trait, not the impl — the
        // architectural decision lives at the declaration.
        let trait_span = AirSpan::new("traits.rs", 42, 47);
        let trait_item = AirItem::Type(AirType {
            kind: TypeKind::Trait,
            name: "Clock".into(),
            symbol: "x::Clock".into(),
            visibility: Visibility::Public,
            fields: Vec::new(),
            variants: Vec::new(),
            decorators: Vec::new(),
            symbol_segments: Vec::new(),
            span: trait_span.clone(),
            doc: None,
        });
        let air = air_with(vec![trait_item, impl_for(Some("Clock"), "SystemClock")]);
        let diags = da001(&air, &enabled(), CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].span, trait_span);
    }

    // ------------- DA002 / DA007 helpers -------------

    use locus_air::{ActionKind, AirFunction, AirTruthAction, AirVariant};

    fn fn_decl(name: &str, symbol: &str) -> AirItem {
        AirItem::Function(AirFunction {
            name: name.into(),
            symbol: symbol.into(),
            visibility: Visibility::Public,
            params: Vec::new(),
            return_type: None,
            span: AirSpan::new("t.rs", 1, 1),
            line_count: 1,
            decorators: Vec::new(),
            symbol_segments: Vec::new(),
            doc: None,
        })
    }

    fn construct_action(target: &str, function: &str) -> AirItem {
        AirItem::TruthAction(AirTruthAction {
            action: ActionKind::Construct,
            target: target.into(),
            function: Some(function.into()),
            span: AirSpan::new("t.rs", 2, 2),
            confidence: 1.0,
            reasons: Vec::new(),
        })
    }

    fn enum_decl(name: &str, symbol: &str, variants: &[&str]) -> AirItem {
        AirItem::Type(AirType {
            kind: TypeKind::Enum,
            name: name.into(),
            symbol: symbol.into(),
            visibility: Visibility::Public,
            fields: Vec::new(),
            variants: variants
                .iter()
                .map(|v| AirVariant {
                    name: (*v).into(),
                    fields: Vec::new(),
                })
                .collect(),
            decorators: Vec::new(),
            symbol_segments: Vec::new(),
            span: AirSpan::new("t.rs", 1, 1),
            doc: None,
        })
    }

    // ------------- DA002 tests -------------

    #[test]
    fn da002_fires_when_factory_constructs_one_target() {
        // `make_widget` constructs `Widget` once → DA002 fires.
        let air = air_with(vec![
            fn_decl("make_widget", "x::make_widget"),
            construct_action("x::Widget", "x::make_widget"),
        ]);
        let diags = da002(&air, &enabled(), CheckMode::Human);
        assert_eq!(diags.len(), 1, "got {diags:?}");
        assert_eq!(diags[0].rule_id, "DA002");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(diags[0].message.contains("make_widget"));
        assert!(diags[0].message.contains("exactly one type"));
        assert_eq!(diags[0].concept.as_deref(), Some("make_widget"));
    }

    #[test]
    fn da002_quiet_when_factory_constructs_two_or_more_distinct_targets() {
        // Real variation justifies the factory.
        let air = air_with(vec![
            fn_decl("create_notifier", "x::create_notifier"),
            construct_action("x::EmailNotifier", "x::create_notifier"),
            construct_action("x::SmsNotifier", "x::create_notifier"),
        ]);
        assert!(da002(&air, &enabled(), CheckMode::Human).is_empty());
    }

    #[test]
    fn da002_quiet_when_factory_has_zero_construct_actions() {
        // A pass-through factory with no Construct truth-action — out of
        // DA002's scope (a different rule could flag pure delegation).
        let air = air_with(vec![fn_decl("build_thing", "x::build_thing")]);
        assert!(da002(&air, &enabled(), CheckMode::Human).is_empty());
    }

    #[test]
    fn da002_quiet_when_function_name_doesnt_match_pattern() {
        let air = air_with(vec![
            fn_decl("compute_widget", "x::compute_widget"),
            construct_action("x::Widget", "x::compute_widget"),
        ]);
        assert!(da002(&air, &enabled(), CheckMode::Human).is_empty());
    }

    #[test]
    fn da002_silent_when_section_disabled() {
        let air = air_with(vec![
            fn_decl("make_widget", "x::make_widget"),
            construct_action("x::Widget", "x::make_widget"),
        ]);
        // `enabled = false` (the default) silences DA002 even with a
        // textbook violation in scope.
        assert!(da002(&air, &DaSection::default(), CheckMode::Human).is_empty());
    }

    #[test]
    fn da002_silent_when_factory_name_patterns_empty() {
        let air = air_with(vec![
            fn_decl("make_widget", "x::make_widget"),
            construct_action("x::Widget", "x::make_widget"),
        ]);
        let section = DaSection {
            enabled: true,
            factory_name_patterns: Vec::new(),
            ..DaSection::default()
        };
        assert!(da002(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn da002_agent_strict_elevates_to_fatal() {
        let air = air_with(vec![
            fn_decl("make_widget", "x::make_widget"),
            construct_action("x::Widget", "x::make_widget"),
        ]);
        let diags = da002(&air, &enabled(), CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    // ------------- DA007 tests -------------

    #[test]
    fn da007_fires_on_single_variant_strategy_enum() {
        let air = air_with(vec![enum_decl(
            "RetryStrategy",
            "x::RetryStrategy",
            &["Linear"],
        )]);
        let diags = da007(&air, &enabled(), CheckMode::Human);
        assert_eq!(diags.len(), 1, "got {diags:?}");
        assert_eq!(diags[0].rule_id, "DA007");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(diags[0].message.contains("RetryStrategy"));
        assert!(diags[0].message.contains("Linear"));
        assert_eq!(diags[0].concept.as_deref(), Some("RetryStrategy"));
    }

    #[test]
    fn da007_quiet_when_strategy_enum_has_two_or_more_variants() {
        let air = air_with(vec![enum_decl(
            "RetryStrategy",
            "x::RetryStrategy",
            &["Linear", "Exponential"],
        )]);
        assert!(da007(&air, &enabled(), CheckMode::Human).is_empty());
    }

    #[test]
    fn da007_quiet_when_enum_name_doesnt_match_pattern() {
        // `RetryConfig` is not a `*Strategy`/`*Mode`/`*Policy` — out of scope.
        let air = air_with(vec![enum_decl(
            "RetryConfig",
            "x::RetryConfig",
            &["Default"],
        )]);
        assert!(da007(&air, &enabled(), CheckMode::Human).is_empty());
    }

    #[test]
    fn da007_silent_when_section_disabled() {
        let air = air_with(vec![enum_decl(
            "RetryStrategy",
            "x::RetryStrategy",
            &["Linear"],
        )]);
        assert!(da007(&air, &DaSection::default(), CheckMode::Human).is_empty());
    }

    #[test]
    fn da007_agent_strict_elevates_to_fatal() {
        let air = air_with(vec![enum_decl(
            "AccessPolicy",
            "x::AccessPolicy",
            &["AllowAll"],
        )]);
        let diags = da007(&air, &enabled(), CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    #[test]
    fn da007_skips_struct_with_strategy_name() {
        // Only enums fire — a `RetryStrategy` *struct* is some other shape
        // and DA007 should ignore it.
        let air = air_with(vec![struct_decl("RetryStrategy", "x::RetryStrategy")]);
        assert!(da007(&air, &enabled(), CheckMode::Human).is_empty());
    }
}
