//! BO rules.
//!
//! Implemented:
//! - [`bo001`]: domain/application file imports a transport- or
//!   persistence-style dependency. Conceptually adjacent to DG001 but uses
//!   BO's own lockfile shape (`domain_paths` × `forbidden_in_domain`) and is
//!   dedicated to the boundary-vs-domain split.
//! - [`bo002`]: function in a domain file exposes a persistence-shaped type
//!   in its parameter or return signature (`persistence_type_patterns`).
//! - [`bo004`]: canonical type carries a forbidden derive (e.g.
//!   `Serialize`/`Deserialize`) — domain types should not be coupled to
//!   serialization/schema frameworks.

use locus_air::{AirItem, AirWorkspace};

use super::lockfile_schema::{BoSection, matches_pattern};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

/// BO001 — domain/application file imports a forbidden transport/persistence
/// dependency.
///
/// For every `AirFile` whose `module_path` matches any pattern in
/// `domain_paths`, walk its `AirImport` items. Fire when the import path
/// matches any pattern in `forbidden_in_domain`.
///
/// Always Fatal: domain leakage of transport/persistence breaks the layered
/// architecture the user has declared via the lockfile — same justification
/// as DG001's forbidden edges.
pub fn bo001(air: &AirWorkspace, section: &BoSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.domain_paths.is_empty() || section.forbidden_in_domain.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            let Some(domain_pattern) = section
                .domain_paths
                .iter()
                .find(|pat| matches_pattern(pat, module_path))
            else {
                continue;
            };
            for item in &file.items {
                let AirItem::Import(imp) = item else {
                    continue;
                };
                let Some(forbidden_pattern) = section
                    .forbidden_in_domain
                    .iter()
                    .find(|pat| matches_pattern(pat, &imp.path))
                else {
                    continue;
                };
                out.push(Diagnostic {
                    rule_id: "BO001".to_string(),
                    severity: mode.elevate(Severity::Fatal),
                    span: imp.span.clone(),
                    concept: None,
                    message: format!(
                        "domain module `{module_path}` imports forbidden \
                         transport/persistence path `{}`",
                        imp.path
                    ),
                    why: vec![
                        format!(
                            "importer `{module_path}` matches domain_paths pattern \
                             `{domain_pattern}`"
                        ),
                        format!(
                            "import `{}` matches forbidden_in_domain pattern \
                             `{forbidden_pattern}`",
                            imp.path
                        ),
                        "domain/application code must not depend directly on transport, \
                         persistence, or serialization frameworks; those concerns belong \
                         at the boundary"
                            .into(),
                    ],
                    suggested_fix: Some(
                        "convert at the boundary (introduce a port/adapter, or move the \
                         conversion into an application-layer service that calls the \
                         framework on the domain's behalf); if the import is a \
                         domain-friendly utility, narrow the `paradigms.BO.forbidden_in_domain` \
                         pattern in `locus.lock` so it no longer matches"
                            .into(),
                    ),
                });
            }
        }
    }
    out
}

/// BO002 — persistence type leaking into a domain function signature.
///
/// For every `AirFunction` whose containing `AirFile.module_path` matches any
/// pattern in `domain_paths`, fire when one of its parameter types or its
/// return type matches any pattern in `persistence_type_patterns` (textual
/// match against the rendered `type_text`).
///
/// Severity: Fatal — same justification as BO001. A `sqlx::PgRow` parameter
/// in a domain function couples the domain to the persistence framework just
/// as surely as importing it would; the import-site check (BO001) wouldn't
/// catch the case where a re-export brings the type in under a different
/// path. This rule is the signature-level companion.
///
/// Silent when either `domain_paths` or `persistence_type_patterns` is empty.
pub fn bo002(air: &AirWorkspace, section: &BoSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.domain_paths.is_empty() || section.persistence_type_patterns.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            let Some(domain_pattern) = section
                .domain_paths
                .iter()
                .find(|pat| matches_pattern(pat, module_path))
            else {
                continue;
            };
            for item in &file.items {
                let AirItem::Function(func) = item else {
                    continue;
                };

                // Check parameters first, then return type. Fire at most once
                // per (function, persistence pattern) match — first hit wins
                // so the diagnostic stays scoped to the actual offender.
                let mut hit: Option<(String, String, String)> = None; // (where, type_text, persistence_pattern)
                for (pname, ptype) in &func.params {
                    if let Some(p) = section
                        .persistence_type_patterns
                        .iter()
                        .find(|pat| type_text_matches(pat, ptype))
                    {
                        hit = Some((format!("parameter `{pname}`"), ptype.clone(), p.clone()));
                        break;
                    }
                }
                if hit.is_none()
                    && let Some(ret) = func.return_type.as_deref()
                    && let Some(p) = section
                        .persistence_type_patterns
                        .iter()
                        .find(|pat| type_text_matches(pat, ret))
                {
                    hit = Some(("return type".to_string(), ret.to_string(), p.clone()));
                }
                let Some((position, type_text, persistence_pattern)) = hit else {
                    continue;
                };

                out.push(Diagnostic {
                    rule_id: "BO002".to_string(),
                    severity: mode.elevate(Severity::Fatal),
                    span: func.span.clone(),
                    concept: None,
                    message: format!(
                        "domain function `{}` exposes persistence-shaped type \
                         `{type_text}` in {position}",
                        func.symbol
                    ),
                    why: vec![
                        format!(
                            "module `{module_path}` matches domain_paths pattern \
                             `{domain_pattern}`"
                        ),
                        format!(
                            "{position} type `{type_text}` matches \
                             persistence_type_patterns pattern \
                             `{persistence_pattern}`"
                        ),
                        "domain functions must speak in domain types; \
                         persistence-shaped values belong on the boundary, \
                         translated by an adapter or repository"
                            .into(),
                    ],
                    suggested_fix: Some(format!(
                        "introduce a domain type and a converter at the \
                         boundary; if `{type_text}` is genuinely a domain \
                         concept (rare), narrow \
                         `paradigms.BO.persistence_type_patterns` in `locus.lock` \
                         so `{persistence_pattern}` no longer matches"
                    )),
                });
            }
        }
    }
    out
}

/// Match a `persistence_type_patterns` entry against an `AirFunction`
/// `type_text`. The rendered `type_text` may include borrows, generics,
/// commas, paths, etc. (e.g. `&sqlx::PgRow`, `Vec<sea_orm::DbErr>`,
/// `Result<Foo, diesel::result::Error>`). We use a substring-aware match
/// over the path-shaped portions: any contiguous path-like fragment in
/// `type_text` is fed through [`matches_pattern`] against the pattern.
fn type_text_matches(pattern: &str, type_text: &str) -> bool {
    // Fast path: exact whole-text match (covers patterns without wildcards
    // and bare type texts like `sqlx::PgRow`).
    if matches_pattern(pattern, type_text) {
        return true;
    }
    // Tokenize on characters that can't appear inside a Rust path. The
    // remaining chunks are candidate path-shaped fragments.
    for fragment in type_text.split(|c: char| !(c.is_alphanumeric() || c == ':' || c == '_')) {
        if fragment.is_empty() {
            continue;
        }
        if matches_pattern(pattern, fragment) {
            return true;
        }
    }
    false
}

/// BO004 — accepted canonical type carries a forbidden derive.
///
/// For every `AirItem::Type` whose containing `AirFile.module_path` matches a
/// `canonical_paths` pattern, fire when any of its `derives` matches a name
/// in `forbidden_canonical_derives`. The point: canonical domain types
/// shouldn't depend on serialization/schema frameworks (`Serialize`,
/// `Deserialize`, `ToSchema`, etc.) — those concerns belong at the boundary,
/// where DTO types do the marshalling.
///
/// Match semantics: derive entries in `forbidden_canonical_derives` are
/// matched as **trait short names**. We compare against both the literal
/// derive token (e.g. `serde::Serialize`) and its last `::` segment
/// (`Serialize`) so a configuration of `["Serialize"]` works whether the
/// derive was authored qualified or unqualified.
///
/// Severity: Warning — having `Serialize` on a canonical type is sloppy but
/// not a hard structural break. Elevated to Fatal under `--agent-strict`.
///
/// Silent when `canonical_paths` is empty (no types are nominated as
/// canonical, so there's nothing to enforce).
pub fn bo004(air: &AirWorkspace, section: &BoSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.canonical_paths.is_empty() || section.forbidden_canonical_derives.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            let Some(canonical_pattern) = section
                .canonical_paths
                .iter()
                .find(|pat| matches_pattern(pat, module_path))
            else {
                continue;
            };
            for item in &file.items {
                let AirItem::Type(ty) = item else {
                    continue;
                };
                for derive in &ty.derives {
                    let short = derive.rsplit("::").next().unwrap_or(derive.as_str());
                    let Some(forbidden) = section
                        .forbidden_canonical_derives
                        .iter()
                        .find(|d| d.as_str() == derive.as_str() || d.as_str() == short)
                    else {
                        continue;
                    };
                    out.push(Diagnostic {
                        rule_id: "BO004".to_string(),
                        severity: mode.elevate(Severity::Warning),
                        span: ty.span.clone(),
                        concept: None,
                        message: format!(
                            "canonical type `{}` carries forbidden derive \
                             `{derive}`",
                            ty.symbol
                        ),
                        why: vec![
                            format!(
                                "module `{module_path}` matches canonical_paths \
                                 pattern `{canonical_pattern}`"
                            ),
                            format!(
                                "derive `{derive}` matches \
                                 forbidden_canonical_derives entry `{forbidden}`"
                            ),
                            "canonical domain types must not depend on \
                             serialization/schema frameworks; serialization \
                             belongs on a boundary DTO"
                                .into(),
                        ],
                        suggested_fix: Some(format!(
                            "remove `{derive}` from `{}` and introduce a \
                             boundary DTO that does carry the derive plus a \
                             converter; if the derive is genuinely needed on \
                             the canonical (e.g. fixture/config), accept it \
                             via `paradigms.BO.forbidden_canonical_derives` in \
                             `locus.lock`",
                            ty.name
                        )),
                    });
                    break; // one diagnostic per type — even if multiple derives match
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use locus_air::{AIR_SCHEMA_VERSION, AirFile, AirImport, AirPackage, AirSpan, Visibility};

    fn import(path: &str) -> AirItem {
        AirItem::Import(AirImport {
            path: path.into(),
            visibility: Visibility::Private,
            span: AirSpan::new("t.rs", 1, 1),
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
            facts: Vec::new(),
        }
    }

    #[test]
    fn bo001_fires_when_domain_file_imports_forbidden_path() {
        let air = air_with_module("crate::domain::user", vec![import("sqlx::Pool")]);
        let section = BoSection {
            domain_paths: vec!["crate::domain::*".into()],
            forbidden_in_domain: vec!["sqlx::*".into()],
            ..Default::default()
        };
        let diags = bo001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "BO001");
        assert_eq!(diags[0].severity, Severity::Fatal);
        assert!(diags[0].message.contains("crate::domain::user"));
        assert!(diags[0].message.contains("sqlx::Pool"));
        assert!(
            diags[0].why.iter().any(|w| w.contains("crate::domain::*")),
            "expected domain pattern in why; got {:?}",
            diags[0].why
        );
        assert!(
            diags[0].why.iter().any(|w| w.contains("sqlx::*")),
            "expected forbidden pattern in why; got {:?}",
            diags[0].why
        );
    }

    #[test]
    fn bo001_quiet_when_non_domain_file_imports_forbidden_path() {
        // Adapter/infra layer is allowed to use sqlx — that's the whole point
        // of putting persistence at the boundary.
        let air = air_with_module("crate::infra::user_repo", vec![import("sqlx::Pool")]);
        let section = BoSection {
            domain_paths: vec!["crate::domain::*".into()],
            forbidden_in_domain: vec!["sqlx::*".into()],
            ..Default::default()
        };
        assert!(bo001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn bo001_quiet_when_domain_file_imports_non_forbidden_path() {
        let air = air_with_module(
            "crate::domain::user",
            vec![import("crate::domain::value::Email")],
        );
        let section = BoSection {
            domain_paths: vec!["crate::domain::*".into()],
            forbidden_in_domain: vec!["sqlx::*".into()],
            ..Default::default()
        };
        assert!(bo001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn bo001_silent_when_domain_paths_empty() {
        let air = air_with_module("crate::domain::user", vec![import("sqlx::Pool")]);
        let section = BoSection {
            domain_paths: vec![],
            forbidden_in_domain: vec!["sqlx::*".into()],
            ..Default::default()
        };
        assert!(bo001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn bo001_silent_when_forbidden_in_domain_empty() {
        let air = air_with_module("crate::domain::user", vec![import("sqlx::Pool")]);
        let section = BoSection {
            domain_paths: vec!["crate::domain::*".into()],
            forbidden_in_domain: vec![],
            ..Default::default()
        };
        assert!(bo001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn bo001_silent_with_default_section() {
        let air = air_with_module("crate::domain::user", vec![import("sqlx::Pool")]);
        let section = BoSection::default();
        assert!(bo001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn bo001_agent_strict_keeps_severity_fatal() {
        // BO001 is already Fatal in human mode; --agent-strict elevates but
        // can't go higher than Fatal — verify it stays Fatal, not panicked.
        let air = air_with_module("crate::domain::user", vec![import("reqwest::Client")]);
        let section = BoSection {
            domain_paths: vec!["crate::domain::*".into()],
            forbidden_in_domain: vec!["reqwest::*".into()],
            ..Default::default()
        };
        let diags = bo001(&air, &section, CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    // ----- BO002 -----

    fn function_item(
        name: &str,
        symbol: &str,
        params: Vec<(&str, &str)>,
        return_type: Option<&str>,
    ) -> AirItem {
        use locus_air::AirFunction;
        AirItem::Function(AirFunction {
            name: name.into(),
            symbol: symbol.into(),
            visibility: Visibility::Public,
            params: params
                .into_iter()
                .map(|(n, t)| (n.to_string(), t.to_string()))
                .collect(),
            return_type: return_type.map(|s| s.to_string()),
            span: AirSpan::new("t.rs", 1, 1),
            line_count: 1,
            doc: None,
        })
    }

    #[test]
    fn bo002_fires_on_persistence_param_in_domain_function() {
        let air = air_with_module(
            "crate::domain::user",
            vec![function_item(
                "load",
                "x::domain::user::load",
                vec![("row", "sqlx::PgRow")],
                None,
            )],
        );
        let section = BoSection {
            domain_paths: vec!["crate::domain::*".into()],
            persistence_type_patterns: vec!["sqlx::*".into()],
            ..Default::default()
        };
        let diags = bo002(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "BO002");
        assert_eq!(diags[0].severity, Severity::Fatal);
        assert!(diags[0].message.contains("sqlx::PgRow"));
        assert!(diags[0].message.contains("parameter `row`"));
        assert!(
            diags[0].why.iter().any(|w| w.contains("crate::domain::*")),
            "expected domain pattern in why; got {:?}",
            diags[0].why
        );
    }

    #[test]
    fn bo002_fires_on_persistence_return_type() {
        let air = air_with_module(
            "crate::domain::user",
            vec![function_item(
                "fetch",
                "x::domain::user::fetch",
                vec![],
                Some("Result<diesel::result::QueryResult, diesel::result::Error>"),
            )],
        );
        let section = BoSection {
            domain_paths: vec!["crate::domain::*".into()],
            persistence_type_patterns: vec!["diesel::*".into()],
            ..Default::default()
        };
        let diags = bo002(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("return type"));
    }

    #[test]
    fn bo002_quiet_in_non_domain_module() {
        // Adapter/infra layer is allowed to expose persistence types.
        let air = air_with_module(
            "crate::infra::user_repo",
            vec![function_item(
                "load",
                "x::infra::user_repo::load",
                vec![("row", "sqlx::PgRow")],
                None,
            )],
        );
        let section = BoSection {
            domain_paths: vec!["crate::domain::*".into()],
            persistence_type_patterns: vec!["sqlx::*".into()],
            ..Default::default()
        };
        assert!(bo002(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn bo002_silent_when_persistence_patterns_empty() {
        let air = air_with_module(
            "crate::domain::user",
            vec![function_item(
                "load",
                "x::domain::user::load",
                vec![("row", "sqlx::PgRow")],
                None,
            )],
        );
        let section = BoSection {
            domain_paths: vec!["crate::domain::*".into()],
            persistence_type_patterns: vec![],
            ..Default::default()
        };
        assert!(bo002(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn bo002_quiet_when_signature_uses_only_domain_types() {
        let air = air_with_module(
            "crate::domain::user",
            vec![function_item(
                "rename",
                "x::domain::user::rename",
                vec![("user", "User"), ("name", "&str")],
                Some("Result<User, DomainError>"),
            )],
        );
        let section = BoSection {
            domain_paths: vec!["crate::domain::*".into()],
            persistence_type_patterns: vec!["sqlx::*".into(), "diesel::*".into()],
            ..Default::default()
        };
        assert!(bo002(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn bo002_agent_strict_stays_fatal() {
        let air = air_with_module(
            "crate::domain::user",
            vec![function_item(
                "load",
                "x::domain::user::load",
                vec![("row", "sea_orm::ActiveModel")],
                None,
            )],
        );
        let section = BoSection {
            domain_paths: vec!["crate::domain::*".into()],
            persistence_type_patterns: vec!["sea_orm::*".into()],
            ..Default::default()
        };
        let diags = bo002(&air, &section, CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    // ----- BO004 -----

    fn type_with_derives(name: &str, symbol: &str, derives: Vec<&str>) -> AirItem {
        use locus_air::{AirType, TypeKind};
        AirItem::Type(AirType {
            kind: TypeKind::Struct,
            name: name.into(),
            symbol: symbol.into(),
            visibility: Visibility::Public,
            fields: Vec::new(),
            variants: Vec::new(),
            derives: derives.into_iter().map(|s| s.to_string()).collect(),
            attrs: Vec::new(),
            span: AirSpan::new("t.rs", 1, 1),
            doc: None,
        })
    }

    #[test]
    fn bo004_fires_on_serialize_in_canonical_module() {
        let air = air_with_module(
            "crate::domain::user",
            vec![type_with_derives(
                "User",
                "x::domain::user::User",
                vec!["Debug", "Clone", "Serialize"],
            )],
        );
        let section = BoSection {
            canonical_paths: vec!["crate::domain::*".into()],
            ..Default::default()
        };
        let diags = bo004(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "BO004");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(diags[0].message.contains("User"));
        assert!(diags[0].message.contains("Serialize"));
    }

    #[test]
    fn bo004_quiet_when_canonical_paths_empty() {
        let air = air_with_module(
            "crate::domain::user",
            vec![type_with_derives(
                "User",
                "x::domain::user::User",
                vec!["Serialize"],
            )],
        );
        let section = BoSection::default(); // canonical_paths empty
        assert!(bo004(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn bo004_quiet_for_non_canonical_module() {
        let air = air_with_module(
            "crate::api::dto",
            vec![type_with_derives(
                "UserDto",
                "x::api::dto::UserDto",
                vec!["Serialize", "Deserialize"],
            )],
        );
        let section = BoSection {
            canonical_paths: vec!["crate::domain::*".into()],
            ..Default::default()
        };
        assert!(bo004(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn bo004_matches_qualified_derive_via_short_name() {
        // Some adapters render derives as `serde::Serialize`. The default
        // forbidden list uses short names — match by trailing segment.
        let air = air_with_module(
            "crate::domain::user",
            vec![type_with_derives(
                "User",
                "x::domain::user::User",
                vec!["serde::Serialize"],
            )],
        );
        let section = BoSection {
            canonical_paths: vec!["crate::domain::*".into()],
            ..Default::default()
        };
        let diags = bo004(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("serde::Serialize"));
    }

    #[test]
    fn bo004_emits_one_diagnostic_per_type_even_with_multiple_forbidden_derives() {
        let air = air_with_module(
            "crate::domain::user",
            vec![type_with_derives(
                "User",
                "x::domain::user::User",
                vec!["Serialize", "Deserialize", "ToSchema"],
            )],
        );
        let section = BoSection {
            canonical_paths: vec!["crate::domain::*".into()],
            ..Default::default()
        };
        let diags = bo004(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1, "one diag per type, not one per derive");
    }

    #[test]
    fn bo004_agent_strict_elevates_warning_to_fatal() {
        let air = air_with_module(
            "crate::domain::user",
            vec![type_with_derives(
                "User",
                "x::domain::user::User",
                vec!["Serialize"],
            )],
        );
        let section = BoSection {
            canonical_paths: vec!["crate::domain::*".into()],
            ..Default::default()
        };
        let diags = bo004(&air, &section, CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }
}
