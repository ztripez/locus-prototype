//! ER rule implementations.
//!
//! Implemented:
//! - [`er001`]: multiple public error types in one file (taxonomy fork).
//!
//! ER001 is heuristic and lockfile-free — it operates purely on AIR and the
//! `Error`/`Err` name suffix convention. Future rules (ER002+) will be
//! lockfile-driven, mirroring OT003+ once the [`ErSection`] grows fields.

use locus_air::{AirItem, AirWorkspace, Visibility};

use super::lockfile_schema::ErSection;
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

/// ER001 — multiple error types in the same module.
///
/// A file defining two or more public types whose names end with the
/// full-word suffix `Error` or `Err` is almost always a taxonomy fork: the
/// author introduced a new error type instead of extending the existing one.
/// The classic anti-pattern in the spec is `UserError` + `CreateUserError` +
/// `UserServiceError` all living side-by-side.
///
/// Algorithm:
/// 1. For each `AirFile`, collect public `AirItem::Type` entries whose name
///    ends with `Error` or `Err` as a *whole-word* suffix (so `UserError` and
///    `IoErr` match, but `Errand` does not).
/// 2. If the file has ≥ 2 such types, pin the first as the "incumbent" and
///    fire one diagnostic per *additional* error type — mirroring OT001's
///    "duplicate canonical" reporting style.
///
/// Severity: Warning by default; Fatal under `--agent-strict` (this is a
/// pattern agents are particularly prone to introducing, so the strict-mode
/// elevation is deliberate). The `_section` parameter is unused for now but
/// kept in the signature for symmetry with future ER rules.
pub fn er001(air: &AirWorkspace, _section: &ErSection, mode: CheckMode) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            // Collect public error types in declaration order.
            let mut error_types: Vec<&locus_air::AirType> = Vec::new();
            for item in &file.items {
                let AirItem::Type(ty) = item else {
                    continue;
                };
                if ty.visibility != Visibility::Public {
                    continue;
                }
                if !has_error_suffix(&ty.name) {
                    continue;
                }
                error_types.push(ty);
            }
            if error_types.len() < 2 {
                continue;
            }

            let incumbent = error_types[0];
            let all_names: Vec<String> = error_types.iter().map(|t| t.name.clone()).collect();

            // One diagnostic per *extra* error type (so 3 error types → 2
            // diagnostics), matching OT001's "incumbent + duplicates" shape.
            for extra in &error_types[1..] {
                out.push(Diagnostic {
                    rule_id: "ER001".to_string(),
                    severity: mode.elevate(Severity::Warning),
                    span: extra.span.clone(),
                    concept: None,
                    message: format!(
                        "`{}` is an additional error type in `{}`; \
                         `{}` is already the incumbent error type",
                        extra.name, file.path, incumbent.name,
                    ),
                    why: vec![
                        format!("file `{}`", file.path),
                        format!(
                            "error types in this file: {}",
                            all_names
                                .iter()
                                .map(|n| format!("`{n}`"))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                        format!("incumbent: `{}`", incumbent.name),
                    ],
                    suggested_fix: Some(format!(
                        "extend `{}` with a new variant rather than introducing a separate \
                         error type, or split `{}` into its own module if the taxonomy \
                         split is deliberate (see `docs/PARADIGMS.md` §\"Paradigm 13: \
                         Error Taxonomy Ownership\"; ER002+ will add lockfile-driven \
                         acceptance for intentional splits)",
                        incumbent.name, extra.name,
                    )),
                });
            }
        }
    }
    out
}

/// True if `name` ends with the full-word suffix `Error` or `Err`.
///
/// The suffix is matched case-sensitively and the leading character of the
/// suffix is uppercase (`E`), which by Rust naming convention means it can
/// only legitimately appear as the start of a CamelCase word. That alone is
/// enough to reject the substring traps:
///
/// - `Errand`, `Errata`, `Errno` — end with `and`, `ata`, `no` (lowercase),
///   so neither `ends_with("Error")` nor `ends_with("Err")` matches.
/// - `Bearer`, `Mirror`, `Terror` — end with lowercase `er` / `or` / `ror`;
///   no match against `Error` (which has capital `E`) or `Err`.
/// - `IoError` — matches `Error` and is correctly classified as an error type.
/// - `IoErr` — matches `Err` and is correctly classified as an error type.
///
/// We also explicitly check `Err` *after* `Error` so that pure `Error`-ending
/// names (e.g. `IoError`) aren't accidentally also tagged via the `Err`
/// branch — `IoError` ends in `ror`, not `Err`, so the order doesn't matter
/// in practice, but the explicit pair documents intent.
fn has_error_suffix(name: &str) -> bool {
    name.ends_with("Error") || name.ends_with("Err")
}

#[cfg(test)]
mod tests {
    use super::*;
    use locus_air::{
        AIR_SCHEMA_VERSION, AirFile, AirItem, AirPackage, AirSpan, AirType, AirWorkspace, TypeKind,
        Visibility,
    };

    fn ty(name: &str, visibility: Visibility) -> AirItem {
        AirItem::Type(AirType {
            kind: TypeKind::Enum,
            name: name.into(),
            symbol: format!("crate::errors::{name}"),
            visibility,
            fields: Vec::new(),
            variants: Vec::new(),
            derives: Vec::new(),
            attrs: Vec::new(),
            span: AirSpan::new("src/errors.rs", 1, 1),
            doc: None,
        })
    }

    fn pub_ty(name: &str) -> AirItem {
        ty(name, Visibility::Public)
    }

    fn air_with_file_items(file_path: &str, items: Vec<AirItem>) -> AirWorkspace {
        AirWorkspace {
            schema_version: AIR_SCHEMA_VERSION,
            packages: vec![AirPackage {
                name: "x".into(),
                version: "0".into(),
                root_dir: "/".into(),
                files: vec![AirFile {
                    path: file_path.into(),
                    module_path: Some("crate".into()),
                    items,
                    hints: Vec::new(),
                    parse_error: None,
                    line_count: 1,
                }],
            }],
        }
    }

    #[test]
    fn er001_fires_when_file_has_two_error_types() {
        let air = air_with_file_items(
            "src/errors.rs",
            vec![pub_ty("UserError"), pub_ty("CreateUserError")],
        );
        let diags = er001(&air, &ErSection::default(), CheckMode::Human);
        assert_eq!(diags.len(), 1, "two error types → one diagnostic on the second");
        assert_eq!(diags[0].rule_id, "ER001");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(
            diags[0].message.contains("CreateUserError"),
            "should flag the non-incumbent; got: {}",
            diags[0].message
        );
        assert!(
            diags[0].message.contains("UserError"),
            "should reference the incumbent; got: {}",
            diags[0].message
        );
        assert!(
            diags[0]
                .why
                .iter()
                .any(|line| line.contains("UserError") && line.contains("CreateUserError")),
            "why list should enumerate every error type in the file; got: {:?}",
            diags[0].why
        );
    }

    #[test]
    fn er001_emits_one_diag_per_extra_error_type() {
        let air = air_with_file_items(
            "src/errors.rs",
            vec![
                pub_ty("UserError"),
                pub_ty("CreateUserError"),
                pub_ty("UserServiceError"),
            ],
        );
        let diags = er001(&air, &ErSection::default(), CheckMode::Human);
        assert_eq!(diags.len(), 2, "three error types → two duplicate diagnostics");
        assert!(diags.iter().all(|d| d.rule_id == "ER001"));
        // Each extra error type gets flagged; the incumbent (UserError) is not.
        let flagged: Vec<&str> = diags
            .iter()
            .map(|d| {
                if d.message.contains("CreateUserError") {
                    "CreateUserError"
                } else if d.message.contains("UserServiceError") {
                    "UserServiceError"
                } else {
                    "(unknown)"
                }
            })
            .collect();
        assert!(flagged.contains(&"CreateUserError"));
        assert!(flagged.contains(&"UserServiceError"));
    }

    #[test]
    fn er001_quiet_when_file_has_one_error_type() {
        let air = air_with_file_items("src/errors.rs", vec![pub_ty("UserError")]);
        assert!(er001(&air, &ErSection::default(), CheckMode::Human).is_empty());
    }

    #[test]
    fn er001_quiet_when_file_has_zero_error_types() {
        let air = air_with_file_items(
            "src/model.rs",
            vec![pub_ty("User"), pub_ty("Team"), pub_ty("Account")],
        );
        assert!(er001(&air, &ErSection::default(), CheckMode::Human).is_empty());
    }

    #[test]
    fn er001_rejects_substring_matches_on_error() {
        // `Errand`, `Errata`, `Errno` end in lowercase tails — not the
        // CamelCase `Error` / `Err` suffix. A `Bearer` ends in `r`, not `Err`,
        // so it never even reaches the boundary check.
        let air = air_with_file_items(
            "src/words.rs",
            vec![
                pub_ty("Errand"),
                pub_ty("Errata"),
                pub_ty("Errno"),
                pub_ty("Bearer"),
            ],
        );
        assert!(
            er001(&air, &ErSection::default(), CheckMode::Human).is_empty(),
            "substring matches must not trip ER001"
        );
    }

    #[test]
    fn er001_detects_err_suffix_too() {
        // `IoErr` and `ParseErr` are full-word `Err` suffixes; both should
        // count as error types and trigger ER001 when they live together.
        let air = air_with_file_items(
            "src/io.rs",
            vec![pub_ty("IoErr"), pub_ty("ParseErr")],
        );
        let diags = er001(&air, &ErSection::default(), CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("ParseErr"));
        assert!(diags[0].message.contains("IoErr"));
    }

    #[test]
    fn er001_agent_strict_elevates_to_fatal() {
        let air = air_with_file_items(
            "src/errors.rs",
            vec![pub_ty("UserError"), pub_ty("CreateUserError")],
        );
        let diags = er001(&air, &ErSection::default(), CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    #[test]
    fn er001_skips_private_error_types() {
        // Two private error types and one public one: only one *public* error
        // type means no diagnostic. Private types are noise — likely internal
        // helper types, not part of the user-facing taxonomy.
        let air = air_with_file_items(
            "src/errors.rs",
            vec![
                pub_ty("UserError"),
                ty("PrivateError", Visibility::Private),
                ty("AlsoPrivateError", Visibility::Crate),
                ty("RestrictedError", Visibility::Restricted),
            ],
        );
        assert!(
            er001(&air, &ErSection::default(), CheckMode::Human).is_empty(),
            "only public error types should count"
        );
    }

    #[test]
    fn er001_isolated_files_do_not_cross_contaminate() {
        // Two files, each with a single error type → no diagnostic. ER001
        // operates per-file, not per-workspace.
        let air = AirWorkspace {
            schema_version: AIR_SCHEMA_VERSION,
            packages: vec![AirPackage {
                name: "x".into(),
                version: "0".into(),
                root_dir: "/".into(),
                files: vec![
                    AirFile {
                        path: "src/a.rs".into(),
                        module_path: Some("crate::a".into()),
                        items: vec![pub_ty("AError")],
                        hints: Vec::new(),
                        parse_error: None,
                        line_count: 1,
                    },
                    AirFile {
                        path: "src/b.rs".into(),
                        module_path: Some("crate::b".into()),
                        items: vec![pub_ty("BError")],
                        hints: Vec::new(),
                        parse_error: None,
                        line_count: 1,
                    },
                ],
            }],
        };
        assert!(er001(&air, &ErSection::default(), CheckMode::Human).is_empty());
    }

    // ---- has_error_suffix unit tests ----

    #[test]
    fn has_error_suffix_accepts_camel_case_words() {
        assert!(has_error_suffix("UserError"));
        assert!(has_error_suffix("CreateUserError"));
        assert!(has_error_suffix("Error")); // bare match is allowed
        assert!(has_error_suffix("Err"));
        assert!(has_error_suffix("IoErr"));
        assert!(has_error_suffix("ParseErr"));
        assert!(has_error_suffix("HTTPError"));
        assert!(has_error_suffix("io_Error")); // underscore separator is fine
    }

    #[test]
    fn has_error_suffix_rejects_substring_traps() {
        // Each of these would catch a sloppy "contains `error`" check, but
        // the case-sensitive CamelCase suffix avoids them all.
        assert!(!has_error_suffix("Errand")); // ends in `and`
        assert!(!has_error_suffix("Errata")); // ends in `ata`
        assert!(!has_error_suffix("Errno")); // ends in `no`
        assert!(!has_error_suffix("Bearer")); // ends in `er`, not `Err`
        assert!(!has_error_suffix("Terror")); // ends in `rror` (lowercase e)
        assert!(!has_error_suffix("Mirror")); // ends in `rror` (lowercase e)
        assert!(!has_error_suffix("User"));
        assert!(!has_error_suffix(""));
    }
}
