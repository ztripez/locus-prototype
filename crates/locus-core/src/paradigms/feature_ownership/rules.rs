//! FO rules.
//!
//! Implemented:
//! - [`fo001`]: same concept defined in two different features (the inverse
//!   of DG003 — DG003 forbids feature A *reaching into* feature B's
//!   internals; FO001 forbids feature A and feature B both *defining* the
//!   same public type name).
//!
//! Follow the OT (`crates/locus-core/src/paradigms/one_truth/rules.rs`) and
//! DG (`crates/locus-core/src/paradigms/dependency_graph/rules.rs`) patterns
//! when adding more rules: each rule is a `pub fn <prefix>NNN(...) -> Vec<Diagnostic>`,
//! lockfile-driven where possible, with severity handling via
//! `CheckMode::elevate`.

use std::collections::BTreeMap;

use locus_air::{AirItem, AirSpan, AirWorkspace, Visibility};

use super::lockfile_schema::{FoFeature, FoSection, matches_pattern};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

/// FO001 — same concept defined in two different features.
///
/// For every public `AirItem::Type`, compute `(feature, type_name)` if the
/// file's `module_path` matches some feature's `module` pattern. Group by
/// `type_name` (case-sensitive). Whenever the same name is defined in two or
/// more different features, fire one diagnostic per non-incumbent definition
/// (the second, third, etc. feature to define that name). The "incumbent" is
/// the feature whose definition is encountered first in workspace iteration
/// order (package, then file, then item).
///
/// Always Fatal: same-name public types across features is a structural
/// ownership conflict — at most one feature can own the canonical concept.
pub fn fo001(air: &AirWorkspace, section: &FoSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.features.is_empty() {
        return Vec::new();
    }

    // For each type name, remember the first (feature, symbol, span) we
    // saw. Iteration order over packages/files/items is the source-walk
    // order, so "first" is deterministic for a given AIR.
    struct Incumbent<'a> {
        feature: &'a FoFeature,
        symbol: String,
        #[allow(dead_code)]
        span: AirSpan,
    }
    let mut incumbents: BTreeMap<String, Incumbent<'_>> = BTreeMap::new();

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            let Some(feature) = owning_feature(&section.features, module_path) else {
                continue;
            };
            for item in &file.items {
                let AirItem::Type(ty) = item else {
                    continue;
                };
                if ty.visibility != Visibility::Public {
                    continue;
                }
                match incumbents.get(&ty.name) {
                    None => {
                        incumbents.insert(
                            ty.name.clone(),
                            Incumbent {
                                feature,
                                symbol: ty.symbol.clone(),
                                span: ty.span.clone(),
                            },
                        );
                    }
                    Some(prev) if std::ptr::eq(prev.feature, feature) => {
                        // Same name, same feature — not a feature-ownership
                        // conflict. (OT may still complain if the symbol is a
                        // duplicate; that's a different paradigm.)
                    }
                    Some(prev) => {
                        out.push(Diagnostic {
                            rule_id: "FO001".to_string(),
                            severity: mode.elevate(Severity::Fatal),
                            span: ty.span.clone(),
                            concept: Some(ty.name.clone()),
                            message: format!(
                                "type `{name}` is defined in both feature `{a}` and feature `{b}`",
                                name = ty.name,
                                a = prev.feature.name,
                                b = feature.name,
                            ),
                            why: vec![
                                format!("`{}` belongs to feature `{}`", ty.symbol, feature.name),
                                format!(
                                    "`{module_path}` matches feature `{}`'s module pattern `{}`",
                                    feature.name, feature.module
                                ),
                                format!(
                                    "feature `{}` already defines a public type `{}` (`{}`)",
                                    prev.feature.name, ty.name, prev.symbol
                                ),
                            ],
                            suggested_fix: Some(format!(
                                "rename this type to a feature-specific name (e.g. \
                                 `{feat_name}::{name}` could become `{feat_pascal}{name}`), or \
                                 move the concept to whichever feature owns it and import it \
                                 from there",
                                feat_name = feature.name,
                                feat_pascal = pascalize(&feature.name),
                                name = ty.name,
                            )),
                        });
                    }
                }
            }
        }
    }
    out
}

/// Find the first feature whose `module` pattern matches `path`. Returns
/// `None` when the path doesn't belong to any declared feature. Mirrors DG's
/// resolver semantics: overlapping `module` patterns are user error and
/// resolve by declaration order.
fn owning_feature<'a>(features: &'a [FoFeature], path: &str) -> Option<&'a FoFeature> {
    features.iter().find(|f| matches_pattern(&f.module, path))
}

/// Lower-snake → UpperCamel for the suggested-fix prose. Best-effort: split on
/// `_`/`::`/`-`/whitespace, capitalize each chunk, concatenate. We only use
/// this to nudge the user toward a rename — exact prefix doesn't matter.
fn pascalize(s: &str) -> String {
    s.split(|c: char| c == '_' || c == '-' || c == ':' || c.is_whitespace())
        .filter(|chunk| !chunk.is_empty())
        .map(|chunk| {
            let mut chars = chunk.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => {
                    first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase()
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use locus_air::{
        AIR_SCHEMA_VERSION, AirFile, AirPackage, AirSpan, AirType, TypeKind, Visibility,
    };

    fn ty(name: &str, symbol: &str, vis: Visibility) -> AirItem {
        AirItem::Type(AirType {
            kind: TypeKind::Struct,
            name: name.into(),
            symbol: symbol.into(),
            visibility: vis,
            fields: Vec::new(),
            variants: Vec::new(),
            derives: Vec::new(),
            attrs: Vec::new(),
            span: AirSpan::new("t.rs", 1, 1),
            doc: None,
        })
    }

    type FileSpec<'a> = (&'a str, Option<&'a str>, Vec<AirItem>);

    fn air_with_files(files: Vec<FileSpec<'_>>) -> AirWorkspace {
        AirWorkspace {
            schema_version: AIR_SCHEMA_VERSION,
            packages: vec![AirPackage {
                name: "x".into(),
                version: "0".into(),
                root_dir: "/".into(),
                files: files
                    .into_iter()
                    .map(|(path, module, items)| AirFile {
                        path: path.into(),
                        module_path: module.map(str::to_owned),
                        items,
                        hints: Vec::new(),
                        parse_error: None,
                        line_count: 1,
                    })
                    .collect(),
            }],
            facts: Vec::new(),
        }
    }

    fn feature(name: &str, module: &str) -> FoFeature {
        FoFeature {
            name: name.into(),
            module: module.into(),
        }
    }

    #[test]
    fn fo001_fires_on_duplicate_public_type_across_features() {
        let air = air_with_files(vec![
            (
                "billing/user.rs",
                Some("crate::billing::user"),
                vec![ty("User", "x::billing::user::User", Visibility::Public)],
            ),
            (
                "identity/user.rs",
                Some("crate::identity::user"),
                vec![ty("User", "x::identity::user::User", Visibility::Public)],
            ),
        ]);
        let section = FoSection {
            features: vec![
                feature("billing", "crate::billing::*"),
                feature("identity", "crate::identity::*"),
            ],
        };
        let diags = fo001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "FO001");
        assert_eq!(diags[0].severity, Severity::Fatal);
        assert!(diags[0].message.contains("User"));
        assert!(diags[0].message.contains("billing"));
        assert!(diags[0].message.contains("identity"));
        assert_eq!(diags[0].concept.as_deref(), Some("User"));
        // why mentions both symbols and the incumbent feature.
        assert!(
            diags[0]
                .why
                .iter()
                .any(|w| w.contains("x::identity::user::User"))
        );
        assert!(
            diags[0]
                .why
                .iter()
                .any(|w| w.contains("x::billing::user::User"))
        );
    }

    #[test]
    fn fo001_emits_one_diag_per_non_incumbent() {
        // 3 features defining `User` → 2 diagnostics (the incumbent is
        // whichever one we encounter first in iteration order).
        let air = air_with_files(vec![
            (
                "billing/user.rs",
                Some("crate::billing::user"),
                vec![ty("User", "x::billing::user::User", Visibility::Public)],
            ),
            (
                "identity/user.rs",
                Some("crate::identity::user"),
                vec![ty("User", "x::identity::user::User", Visibility::Public)],
            ),
            (
                "ops/user.rs",
                Some("crate::ops::user"),
                vec![ty("User", "x::ops::user::User", Visibility::Public)],
            ),
        ]);
        let section = FoSection {
            features: vec![
                feature("billing", "crate::billing::*"),
                feature("identity", "crate::identity::*"),
                feature("ops", "crate::ops::*"),
            ],
        };
        let diags = fo001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 2, "got {diags:?}");
        // Both subsequent definitions reference billing as the incumbent.
        for d in &diags {
            assert_eq!(d.rule_id, "FO001");
            assert!(d.message.contains("billing"));
            assert!(d.message.contains("User"));
        }
        // The two non-incumbent features should each appear once.
        let messages: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
        assert!(messages.iter().any(|m| m.contains("identity")));
        assert!(messages.iter().any(|m| m.contains("ops")));
    }

    #[test]
    fn fo001_quiet_when_same_name_lives_in_same_feature() {
        // Two files inside `billing` both define `User` (a duplicate-symbol
        // problem for OT, not FO — this rule cares about ownership across
        // features, not within one).
        let air = air_with_files(vec![
            (
                "billing/user.rs",
                Some("crate::billing::user"),
                vec![ty("User", "x::billing::user::User", Visibility::Public)],
            ),
            (
                "billing/account.rs",
                Some("crate::billing::account"),
                vec![ty("User", "x::billing::account::User", Visibility::Public)],
            ),
        ]);
        let section = FoSection {
            features: vec![
                feature("billing", "crate::billing::*"),
                feature("identity", "crate::identity::*"),
            ],
        };
        assert!(fo001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn fo001_quiet_when_duplicates_live_in_unfeatured_files() {
        // Neither file matches any feature — out of FO's jurisdiction.
        let air = air_with_files(vec![
            (
                "scripts/a.rs",
                Some("scripts::a"),
                vec![ty("User", "x::scripts::a::User", Visibility::Public)],
            ),
            (
                "scripts/b.rs",
                Some("scripts::b"),
                vec![ty("User", "x::scripts::b::User", Visibility::Public)],
            ),
            // One feature exists but doesn't include either file.
            (
                "billing/order.rs",
                Some("crate::billing::order"),
                vec![ty("Order", "x::billing::order::Order", Visibility::Public)],
            ),
        ]);
        let section = FoSection {
            features: vec![feature("billing", "crate::billing::*")],
        };
        assert!(fo001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn fo001_silent_when_features_empty() {
        let air = air_with_files(vec![
            (
                "billing/user.rs",
                Some("crate::billing::user"),
                vec![ty("User", "x::billing::user::User", Visibility::Public)],
            ),
            (
                "identity/user.rs",
                Some("crate::identity::user"),
                vec![ty("User", "x::identity::user::User", Visibility::Public)],
            ),
        ]);
        let section = FoSection::default();
        assert!(fo001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn fo001_skips_private_types() {
        // Private types in either feature are out of scope: feature
        // ownership only applies to types another feature could plausibly
        // import.
        let air = air_with_files(vec![
            (
                "billing/user.rs",
                Some("crate::billing::user"),
                vec![ty("User", "x::billing::user::User", Visibility::Private)],
            ),
            (
                "identity/user.rs",
                Some("crate::identity::user"),
                vec![ty("User", "x::identity::user::User", Visibility::Public)],
            ),
        ]);
        let section = FoSection {
            features: vec![
                feature("billing", "crate::billing::*"),
                feature("identity", "crate::identity::*"),
            ],
        };
        // Only one Public definition exists, so nothing fires.
        assert!(fo001(&air, &section, CheckMode::Human).is_empty());

        // And when both are private, still nothing fires.
        let air_both_private = air_with_files(vec![
            (
                "billing/user.rs",
                Some("crate::billing::user"),
                vec![ty("User", "x::billing::user::User", Visibility::Private)],
            ),
            (
                "identity/user.rs",
                Some("crate::identity::user"),
                vec![ty("User", "x::identity::user::User", Visibility::Private)],
            ),
        ]);
        assert!(fo001(&air_both_private, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn fo001_agent_strict_keeps_fatal() {
        let air = air_with_files(vec![
            (
                "billing/user.rs",
                Some("crate::billing::user"),
                vec![ty("User", "x::billing::user::User", Visibility::Public)],
            ),
            (
                "identity/user.rs",
                Some("crate::identity::user"),
                vec![ty("User", "x::identity::user::User", Visibility::Public)],
            ),
        ]);
        let section = FoSection {
            features: vec![
                feature("billing", "crate::billing::*"),
                feature("identity", "crate::identity::*"),
            ],
        };
        let diags = fo001(&air, &section, CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].severity,
            Severity::Fatal,
            "FO001 must remain Fatal under --agent-strict"
        );
    }

    #[test]
    fn fo001_quiet_when_only_one_feature_defines_the_name() {
        // `Order` lives only in billing; `User` only in identity. No
        // collisions across features → no diagnostics.
        let air = air_with_files(vec![
            (
                "billing/order.rs",
                Some("crate::billing::order"),
                vec![ty("Order", "x::billing::order::Order", Visibility::Public)],
            ),
            (
                "identity/user.rs",
                Some("crate::identity::user"),
                vec![ty("User", "x::identity::user::User", Visibility::Public)],
            ),
        ]);
        let section = FoSection {
            features: vec![
                feature("billing", "crate::billing::*"),
                feature("identity", "crate::identity::*"),
            ],
        };
        assert!(fo001(&air, &section, CheckMode::Human).is_empty());
    }
}
