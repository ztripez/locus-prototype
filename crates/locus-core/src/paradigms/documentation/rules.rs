//! DC rule implementations.
//!
//! Implemented:
//! - [`dc001`]: public type or function has no doc comment. Heuristic
//!   baseline for documentation ownership — a public symbol with no
//!   `///` / `#[doc = "..."]` is an undocumented API surface, which the
//!   spec calls out as a failure of documentation ownership
//!   (`docs/PARADIGMS.md` §"Paradigm 17: Documentation / Comment
//!   Ownership").
//!
//! DC001 is opt-in: it returns no diagnostics unless
//! `paradigms.DC.require_public_docs` is `true`. Patterns listed in
//! `paradigms.DC.exempt_paths` skip the file entirely (intended for test
//! modules, generated code, FFI shims).

use locus_air::{AirItem, AirWorkspace, Visibility};

use super::lockfile_schema::{DcSection, matches_pattern};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

/// DC001 — public API has no doc comment.
///
/// For every `AirFile` whose `module_path` does *not* match any pattern in
/// `exempt_paths`, fire one diagnostic per `AirItem::Type` or
/// `AirItem::Function` whose `visibility` is `Public` and whose `doc` is
/// `None`.
///
/// Returns no diagnostics when `section.require_public_docs` is `false`
/// (the default). This keeps the rule silent for projects that haven't
/// opted into the "public API must be documented" policy.
///
/// Severity: Warning by default; Fatal under `--agent-strict`. Documented
/// public API is a guardrail agents are particularly prone to skipping, so
/// the strict-mode elevation is deliberate.
pub fn dc001(air: &AirWorkspace, section: &DcSection, mode: CheckMode) -> Vec<Diagnostic> {
    if !section.require_public_docs {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            // Files without a module_path can't be matched against
            // exempt_paths. Treat them as non-exempt — the rule still
            // applies, falling back on the file `path` for diagnostic
            // text.
            let module_path = file.module_path.as_deref();
            if let Some(mp) = module_path
                && section
                    .exempt_paths
                    .iter()
                    .any(|pat| matches_pattern(pat, mp))
            {
                continue;
            }
            let module_label = module_path.unwrap_or(&file.path);

            for item in &file.items {
                match item {
                    AirItem::Type(ty) => {
                        if ty.visibility != Visibility::Public {
                            continue;
                        }
                        if ty.doc.is_some() {
                            continue;
                        }
                        out.push(Diagnostic {
                            rule_id: "DC001".to_string(),
                            severity: mode.elevate(Severity::Warning),
                            span: ty.span.clone(),
                            concept: None,
                            message: format!(
                                "public type `{}` in `{}` has no doc comment",
                                ty.name, module_label,
                            ),
                            why: vec![
                                format!("type `{}` (`{}`)", ty.name, ty.symbol),
                                "visibility is Public".into(),
                                "doc is None (no `///` or `#[doc = \"...\"]` text)".into(),
                                format!(
                                    "module `{module_label}` did not match any \
                                     `paradigms.DC.exempt_paths` pattern"
                                ),
                            ],
                            suggested_fix: Some(format!(
                                "add a `///` doc comment on `{}` describing why it exists \
                                 and what invariant it carries; if this region is \
                                 intentionally undocumented, add a pattern to \
                                 `paradigms.DC.exempt_paths` (e.g. `{module_label}` or a \
                                 `parent::*` wildcard) — see `docs/PARADIGMS.md` \
                                 §\"Paradigm 17: Documentation / Comment Ownership\"",
                                ty.name,
                            )),
                        });
                    }
                    AirItem::Function(func) => {
                        if func.visibility != Visibility::Public {
                            continue;
                        }
                        if func.doc.is_some() {
                            continue;
                        }
                        out.push(Diagnostic {
                            rule_id: "DC001".to_string(),
                            severity: mode.elevate(Severity::Warning),
                            span: func.span.clone(),
                            concept: None,
                            message: format!(
                                "public function `{}` in `{}` has no doc comment",
                                func.name, module_label,
                            ),
                            why: vec![
                                format!("function `{}` (`{}`)", func.name, func.symbol),
                                "visibility is Public".into(),
                                "doc is None (no `///` or `#[doc = \"...\"]` text)".into(),
                                format!(
                                    "module `{module_label}` did not match any \
                                     `paradigms.DC.exempt_paths` pattern"
                                ),
                            ],
                            suggested_fix: Some(format!(
                                "add a `///` doc comment on `{}` describing why it exists \
                                 and what invariant it carries; if this region is \
                                 intentionally undocumented, add a pattern to \
                                 `paradigms.DC.exempt_paths` (e.g. `{module_label}` or a \
                                 `parent::*` wildcard) — see `docs/PARADIGMS.md` \
                                 §\"Paradigm 17: Documentation / Comment Ownership\"",
                                func.name,
                            )),
                        });
                    }
                    _ => {}
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use locus_air::{
        AIR_SCHEMA_VERSION, AirFile, AirFunction, AirPackage, AirSpan, AirType, TypeKind,
        Visibility,
    };

    fn ty_item(name: &str, vis: Visibility, doc: Option<&str>) -> AirItem {
        AirItem::Type(AirType {
            kind: TypeKind::Struct,
            name: name.into(),
            symbol: format!("x::api::{name}"),
            visibility: vis,
            fields: Vec::new(),
            variants: Vec::new(),
            derives: Vec::new(),
            attrs: Vec::new(),
            span: AirSpan::new("t.rs", 1, 1),
            doc: doc.map(|s| s.to_string()),
        })
    }

    fn fn_item(name: &str, vis: Visibility, doc: Option<&str>) -> AirItem {
        AirItem::Function(AirFunction {
            name: name.into(),
            symbol: format!("x::api::{name}"),
            visibility: vis,
            params: Vec::new(),
            return_type: None,
            span: AirSpan::new("t.rs", 1, 1),
            line_count: 1,
            doc: doc.map(|s| s.to_string()),
        })
    }

    fn air_with_module(module: &str, items: Vec<AirItem>) -> AirWorkspace {
        AirWorkspace {
            schema_version: AIR_SCHEMA_VERSION,
            packages: vec![AirPackage {
                name: "x".into(),
                version: "0".into(),
                root_dir: "/".into(),
                files: vec![AirFile {
                    path: "t.rs".into(),
                    module_path: Some(module.into()),
                    items,
                    hints: Vec::new(),
                    parse_error: None,
                    line_count: 1,
                }],
            }],
        }
    }

    #[test]
    fn dc001_silent_when_require_public_docs_is_default_false() {
        let air = air_with_module(
            "x::api",
            vec![
                ty_item("Widget", Visibility::Public, None),
                fn_item("make_widget", Visibility::Public, None),
            ],
        );
        let section = DcSection::default();
        assert!(dc001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn dc001_fires_on_public_type_without_doc() {
        let air = air_with_module("x::api", vec![ty_item("Widget", Visibility::Public, None)]);
        let section = DcSection {
            require_public_docs: true,
            exempt_paths: Vec::new(),
        };
        let diags = dc001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "DC001");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(diags[0].message.contains("Widget"));
        assert!(diags[0].message.contains("x::api"));
        assert!(diags[0].message.contains("no doc comment"));
    }

    #[test]
    fn dc001_fires_on_public_function_without_doc() {
        let air = air_with_module(
            "x::api",
            vec![fn_item("make_widget", Visibility::Public, None)],
        );
        let section = DcSection {
            require_public_docs: true,
            exempt_paths: Vec::new(),
        };
        let diags = dc001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "DC001");
        assert!(diags[0].message.contains("make_widget"));
        assert!(diags[0].message.contains("public function"));
    }

    #[test]
    fn dc001_quiet_on_private_items() {
        let air = air_with_module(
            "x::api",
            vec![
                ty_item("Widget", Visibility::Private, None),
                ty_item("Inner", Visibility::Crate, None),
                ty_item("Restricted", Visibility::Restricted, None),
                fn_item("helper", Visibility::Private, None),
                fn_item("crate_helper", Visibility::Crate, None),
            ],
        );
        let section = DcSection {
            require_public_docs: true,
            exempt_paths: Vec::new(),
        };
        assert!(dc001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn dc001_quiet_on_items_with_doc() {
        let air = air_with_module(
            "x::api",
            vec![
                ty_item("Widget", Visibility::Public, Some("a thing")),
                fn_item("make_widget", Visibility::Public, Some("makes one")),
            ],
        );
        let section = DcSection {
            require_public_docs: true,
            exempt_paths: Vec::new(),
        };
        assert!(dc001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn dc001_skips_files_matching_exempt_paths() {
        let air = air_with_module(
            "x::api::tests",
            vec![
                ty_item("Widget", Visibility::Public, None),
                fn_item("make_widget", Visibility::Public, None),
            ],
        );
        let section = DcSection {
            require_public_docs: true,
            exempt_paths: vec!["x::api::tests::*".into(), "x::api::tests".into()],
        };
        assert!(dc001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn dc001_agent_strict_elevates_to_fatal() {
        let air = air_with_module("x::api", vec![ty_item("Widget", Visibility::Public, None)]);
        let section = DcSection {
            require_public_docs: true,
            exempt_paths: Vec::new(),
        };
        let diags = dc001(&air, &section, CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    #[test]
    fn dc001_fires_per_undocumented_item_in_mixed_file() {
        let air = air_with_module(
            "x::api",
            vec![
                ty_item("Documented", Visibility::Public, Some("good")),
                ty_item("UndocType", Visibility::Public, None),
                fn_item("undoc_fn", Visibility::Public, None),
                ty_item("PrivateType", Visibility::Private, None),
            ],
        );
        let section = DcSection {
            require_public_docs: true,
            exempt_paths: Vec::new(),
        };
        let diags = dc001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 2);
        let messages: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
        assert!(messages.iter().any(|m| m.contains("UndocType")));
        assert!(messages.iter().any(|m| m.contains("undoc_fn")));
        assert!(!messages.iter().any(|m| m.contains("Documented")));
        assert!(!messages.iter().any(|m| m.contains("PrivateType")));
    }
}
