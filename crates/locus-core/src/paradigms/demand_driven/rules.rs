//! DA rule implementations.
//!
//! Implemented:
//! - [`da001`]: trait declared with exactly one implementation in the
//!   workspace and no accepted port role — speculative variation surface.
//!
//! Future DA rules will cover the rest of the spec (single-entry registries,
//! one-variant strategy enums, factories that construct one type, …). DA001
//! is the cleanest first slice: AIR schema v5 emits both `TypeKind::Trait`
//! declarations and `AirItem::Impl` records, so the rule needs no fuzzy text
//! matching — a trait with one impl block is a structural fact.

use std::collections::BTreeMap;

use locus_air::{AirItem, AirType, AirWorkspace, TypeKind};

use super::lockfile_schema::DaSection;
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

    // Second pass: count impl blocks per trait. We match an `AirImpl.trait_path`
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
                let Some(raw) = imp.trait_path.as_deref() else {
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

#[cfg(test)]
mod tests {
    use super::*;
    use locus_air::{
        AIR_SCHEMA_VERSION, AirFile, AirImpl, AirItem, AirPackage, AirSpan, AirType, TypeKind,
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
            derives: Vec::new(),
            attrs: Vec::new(),
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
            derives: Vec::new(),
            attrs: Vec::new(),
            span: trait_span.clone(),
            doc: None,
        });
        let air = air_with(vec![trait_item, impl_for(Some("Clock"), "SystemClock")]);
        let diags = da001(&air, &enabled(), CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].span, trait_span);
    }
}
