//! MO rule implementations.
//!
//! Implemented:
//! - [`mo001`]: too many public top-level types in a single file.
//!
//! Future MO rules will cover the spec's full responsibility-entropy story
//! (multiple architectural roles in one module, canonical types co-located
//! with adapters, …). MO001 is the first slice — a count-based heuristic
//! for the simplest variant of "this file owns too much."

use locus_air::{AirItem, AirWorkspace, Visibility};

use super::lockfile_schema::MoSection;
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

/// MO001 — module file has too many public top-level types.
///
/// For each `AirFile` with a `module_path`, count `AirItem::Type` items
/// whose visibility is `Public`. Compare against the file's effective
/// budget:
/// - if the file's `module_path` matches an override's `module` pattern,
///   the override's `max_public_types` wins;
/// - otherwise the section's `default_max_public_types` (or the constant
///   fallback) is used.
///
/// One diagnostic per file (not per type) — the violation is the file's
/// responsibility, not any individual type.
///
/// Severity: Warning by default. `--agent-strict` elevates to Fatal via
/// [`CheckMode::elevate`].
///
/// Lockfile-driven silence: when the section is fully default (no
/// `default_max_public_types` set AND no overrides), MO001 emits nothing.
/// Same convention as the other lockfile-driven rules — pre-onboarding,
/// we don't have the user's intent and shouldn't fire on un-configured
/// projects.
pub fn mo001(air: &AirWorkspace, section: &MoSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.default_max_public_types.is_none() && section.overrides.is_empty() {
        return Vec::new();
    }
    let default_budget = section.effective_default();
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            let count = file
                .items
                .iter()
                .filter(|item| matches!(item, AirItem::Type(t) if t.visibility == Visibility::Public))
                .count() as u32;

            let matched_override = section.matching_override(module_path);
            let budget = matched_override
                .map(|o| o.max_public_types)
                .unwrap_or(default_budget);
            if count <= budget {
                continue;
            }

            // Anchor the diagnostic at the file's first public type when
            // possible — otherwise at line 1 of the file. Either way, the
            // diagnostic is per-file, not per-type.
            let span = file
                .items
                .iter()
                .find_map(|item| match item {
                    AirItem::Type(t) if t.visibility == Visibility::Public => Some(t.span.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| locus_air::AirSpan::new(file.path.clone(), 1, 1));

            let mut why = vec![
                format!("file `{module_path}` defines {count} public top-level type(s)"),
                if let Some(o) = matched_override {
                    format!(
                        "budget {budget} from override `module = {}`",
                        o.module
                    )
                } else {
                    format!("budget {budget} (workspace default)")
                },
            ];
            if matched_override.is_none() && section.default_max_public_types.is_none() {
                why.push(format!(
                    "no `default_max_public_types` configured; using built-in fallback {}",
                    default_budget
                ));
            }

            out.push(Diagnostic {
                rule_id: "MO001".to_string(),
                severity: mode.elevate(Severity::Warning),
                span,
                concept: None,
                message: format!(
                    "module `{module_path}` has {count} public top-level types (budget {budget})"
                ),
                why,
                suggested_fix: Some(
                    "split the module into submodules each owning one architectural role, \
                     or — if this density is intended (e.g. an API surface) — raise the \
                     budget by adding an override to `paradigms.MO.overrides` in \
                     `locus.lock`"
                        .into(),
                ),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::lockfile_schema::{MoOverride, MoSection};
    use super::*;
    use locus_air::{
        AIR_SCHEMA_VERSION, AirFile, AirPackage, AirSpan, AirType, TypeKind, Visibility,
    };

    fn pub_type(name: &str) -> AirItem {
        AirItem::Type(AirType {
            kind: TypeKind::Struct,
            name: name.into(),
            symbol: format!("x::{name}"),
            visibility: Visibility::Public,
            fields: Vec::new(),
            variants: Vec::new(),
            derives: Vec::new(),
            attrs: Vec::new(),
            span: AirSpan::new("t.rs", 1, 1),
        })
    }

    fn priv_type(name: &str) -> AirItem {
        AirItem::Type(AirType {
            kind: TypeKind::Struct,
            name: name.into(),
            symbol: format!("x::{name}"),
            visibility: Visibility::Private,
            fields: Vec::new(),
            variants: Vec::new(),
            derives: Vec::new(),
            attrs: Vec::new(),
            span: AirSpan::new("t.rs", 1, 1),
        })
    }

    fn air_with(module: Option<&str>, items: Vec<AirItem>) -> AirWorkspace {
        AirWorkspace {
            schema_version: AIR_SCHEMA_VERSION,
            packages: vec![AirPackage {
                name: "x".into(),
                version: "0".into(),
                root_dir: "/".into(),
                files: vec![AirFile {
                    path: "t.rs".into(),
                    module_path: module.map(str::to_string),
                    items,
                    hints: Vec::new(),
                    parse_error: None,
                }],
            }],
        }
    }

    fn n_pub_types(n: usize) -> Vec<AirItem> {
        (0..n).map(|i| pub_type(&format!("T{i}"))).collect()
    }

    fn configured(default_budget: u32) -> MoSection {
        MoSection {
            default_max_public_types: Some(default_budget),
            overrides: Vec::new(),
        }
    }

    #[test]
    fn mo001_silent_on_default_section() {
        // No fields configured — must stay silent regardless of file shape.
        // Mirrors the DG/OT lockfile-driven convention.
        let air = air_with(Some("foo::bar"), n_pub_types(50));
        let section = MoSection::default();
        assert!(mo001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn mo001_fires_when_count_exceeds_default_budget() {
        // 6 public types under default budget of 5 → fires.
        let air = air_with(Some("foo::bar"), n_pub_types(6));
        let section = configured(5);
        let diags = mo001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1, "expected one diag, got {diags:?}");
        assert_eq!(diags[0].rule_id, "MO001");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(diags[0].message.contains("foo::bar"));
        assert!(diags[0].message.contains("6"));
        assert!(diags[0].message.contains("budget 5"));
    }

    #[test]
    fn mo001_quiet_when_count_at_or_below_default_budget() {
        let section = configured(5);
        // exactly at budget
        let air = air_with(Some("foo::bar"), n_pub_types(5));
        assert!(mo001(&air, &section, CheckMode::Human).is_empty());
        // under budget
        let air = air_with(Some("foo::bar"), n_pub_types(2));
        assert!(mo001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn mo001_only_counts_public_top_level_types() {
        // 4 private + 5 public = 9 items, but only 5 are pub → at budget, quiet.
        let mut items = n_pub_types(5);
        for i in 0..4 {
            items.push(priv_type(&format!("Priv{i}")));
        }
        let air = air_with(Some("foo::bar"), items);
        let section = configured(5);
        assert!(mo001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn mo001_override_raises_budget_effectively() {
        // Default budget 5; api module has 12 public types, override gives 20.
        let air = air_with(Some("lore::api::v1"), n_pub_types(12));
        let section = MoSection {
            default_max_public_types: Some(5),
            overrides: vec![MoOverride {
                module: "lore::api::*".into(),
                max_public_types: 20,
            }],
        };
        assert!(
            mo001(&air, &section, CheckMode::Human).is_empty(),
            "override should raise budget above the file's count"
        );
    }

    #[test]
    fn mo001_override_lowers_budget_effectively() {
        // Default 5; domain file has 5 public types (within default). Override
        // lowers the domain budget to 2 → fires.
        let air = air_with(Some("lore::domain::user"), n_pub_types(5));
        let section = MoSection {
            default_max_public_types: Some(5),
            overrides: vec![MoOverride {
                module: "lore::domain::*".into(),
                max_public_types: 2,
            }],
        };
        let diags = mo001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1, "override should lower budget below count");
        assert_eq!(diags[0].rule_id, "MO001");
        assert!(diags[0].message.contains("budget 2"));
        assert!(
            diags[0]
                .why
                .iter()
                .any(|w| w.contains("override") && w.contains("lore::domain::*")),
            "expected override mention in `why`; got {:?}",
            diags[0].why
        );
    }

    #[test]
    fn mo001_first_override_wins() {
        let air = air_with(Some("lore::api::v1"), n_pub_types(8));
        let section = MoSection {
            default_max_public_types: Some(5),
            overrides: vec![
                MoOverride {
                    module: "lore::api::*".into(),
                    max_public_types: 20,
                },
                MoOverride {
                    module: "lore::*".into(),
                    max_public_types: 3,
                },
            ],
        };
        // First override (20) wins, so 8 public types is fine.
        assert!(mo001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn mo001_agent_strict_elevates_to_fatal() {
        let air = air_with(Some("foo::bar"), n_pub_types(6));
        let section = configured(5);
        let diags = mo001(&air, &section, CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].severity,
            Severity::Fatal,
            "agent-strict should elevate Warning to Fatal"
        );
    }

    #[test]
    fn mo001_skips_files_without_module_path() {
        // No module_path → can't apply overrides → skip entirely.
        let air = air_with(None, n_pub_types(50));
        let section = configured(5);
        assert!(mo001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn mo001_one_diagnostic_per_file() {
        // Two violating files → two diagnostics, regardless of how many
        // public types each contains.
        let air = AirWorkspace {
            schema_version: AIR_SCHEMA_VERSION,
            packages: vec![AirPackage {
                name: "x".into(),
                version: "0".into(),
                root_dir: "/".into(),
                files: vec![
                    AirFile {
                        path: "a.rs".into(),
                        module_path: Some("x::a".into()),
                        items: n_pub_types(10),
                        hints: Vec::new(),
                        parse_error: None,
                    },
                    AirFile {
                        path: "b.rs".into(),
                        module_path: Some("x::b".into()),
                        items: n_pub_types(7),
                        hints: Vec::new(),
                        parse_error: None,
                    },
                ],
            }],
        };
        let section = configured(5);
        let diags = mo001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 2, "got {diags:?}");
    }

    #[test]
    fn mo001_with_only_overrides_and_no_default_uses_fallback_for_unmatched() {
        // overrides set → section is non-default → MO001 active. Files that
        // don't match any override fall back to DEFAULT_MAX_PUBLIC_TYPES (5).
        let air = air_with(Some("other::module"), n_pub_types(6));
        let section = MoSection {
            default_max_public_types: None,
            overrides: vec![MoOverride {
                module: "lore::api::*".into(),
                max_public_types: 20,
            }],
        };
        let diags = mo001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1, "fallback budget should apply; got {diags:?}");
        assert!(diags[0].message.contains("budget 5"));
        assert!(
            diags[0]
                .why
                .iter()
                .any(|w| w.contains("built-in fallback")),
            "expected fallback explanation in why; got {:?}",
            diags[0].why
        );
    }
}
