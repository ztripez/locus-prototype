//! ER rule implementations.
//!
//! Implemented:
//! - [`er001`]: multiple public error types in one file (taxonomy fork).
//! - [`er002`]: a `Result<_, E>` return type whose `E` matches a user-listed
//!   "string-shaped" / catch-all forbidden pattern (taxonomy collapse).
//! - [`er003`]: a domain error enum embeds a boundary error type as a
//!   variant field — structural taxonomy violation that buries the
//!   transport failure inside the domain vocabulary.
//! - [`er007`]: a variant name appears on two or more `*Error*` enums in
//!   the workspace — the taxonomy is drifting / duplicating.
//!
//! ER001 is heuristic and lockfile-free — it operates purely on AIR and the
//! `Error`/`Err` name suffix convention. ER002 is lockfile-driven via
//! [`ErSection::forbidden_error_types`]; it stays silent until that list is
//! populated. ER003 is lockfile-driven via [`ErSection::domain_paths`] +
//! [`ErSection::boundary_error_patterns`]; silent until both are populated.
//! ER007 is heuristic and lockfile-free.

use locus_air::{AirItem, AirWorkspace, Visibility};

use super::lockfile_schema::{ErSection, matches_pattern};
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
/// elevation is deliberate). The `_section` parameter is unused — kept in
/// the signature for symmetry with future ER rules.
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

/// ER002 — string-shaped / catch-all error in a `Result<_, E>` return type.
///
/// For every `AirItem::Function` with a `return_type` that starts with
/// `Result<` (after trimming + an optional leading `::`), extract the `E`
/// position via the same top-level-comma logic FL001 uses. Match the trimmed
/// `E` (with one leading `&` peeled, so `&str` lines up with the `"&str"`
/// pattern) against every entry in [`ErSection::forbidden_error_types`].
/// Each match fires one diagnostic.
///
/// ER001 catches the *opposite* drift — too many error types in one file.
/// ER002 catches the inverse: collapsing the taxonomy to `String` /
/// `anyhow::Error` because the agent didn't want to define a typed variant.
///
/// Severity: **Fatal** in both modes. The match is exact-pattern and
/// lockfile-driven — there's no inference involved, so no `from_confidence`
/// and no Human-mode warning tier. Empty `forbidden_error_types` keeps the
/// rule fully silent.
pub fn er002(air: &AirWorkspace, section: &ErSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.forbidden_error_types.is_empty() {
        return Vec::new();
    }
    let _ = mode; // Severity is always Fatal; mode unused but kept for symmetry.
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                let AirItem::Function(func) = item else {
                    continue;
                };
                let Some(ret) = func.return_type.as_deref() else {
                    continue;
                };
                // Cheap pre-filter: only walk Result<...>-shaped returns.
                let trimmed_ret = ret.trim();
                let trimmed_ret = trimmed_ret.strip_prefix("::").unwrap_or(trimmed_ret);
                if !trimmed_ret.starts_with("Result<") {
                    continue;
                }
                let Some(err_ty_raw) = extract_result_error_type(ret) else {
                    continue;
                };
                // Normalise the extracted text: trim whitespace, peel one
                // leading `&` (so `&str` / `&MyErr` line up with bare
                // patterns). We keep the original `err_ty_raw` for the
                // diagnostic message so users see what their source actually
                // says, and apply the same normalisation to each pattern so
                // a literal `"&str"` pattern still matches.
                let err_ty = normalise_error_text(err_ty_raw);
                let Some(matched_pattern) = section
                    .forbidden_error_types
                    .iter()
                    .find(|pat| matches_error_pattern(&normalise_error_text(pat), &err_ty))
                else {
                    continue;
                };
                let display_err_ty = err_ty_raw.trim();
                out.push(Diagnostic {
                    rule_id: "ER002".to_string(),
                    severity: Severity::Fatal,
                    span: func.span.clone(),
                    concept: None,
                    message: format!(
                        "function `{}` returns `{}` whose error type `{}` matches \
                         forbidden pattern `{}`",
                        func.name, ret, display_err_ty, matched_pattern,
                    ),
                    why: vec![
                        format!("function `{}` (`{}`)", func.name, func.symbol),
                        format!("return type `{ret}`"),
                        format!(
                            "extracted error type `{display_err_ty}` matches forbidden \
                             pattern `{matched_pattern}`"
                        ),
                        "string-shaped / catch-all error returns collapse the project's \
                         error taxonomy: every failure mode is forced through one \
                         opaque variant, so callers can't pattern-match on the cause \
                         and the failure lineage is lost"
                            .into(),
                    ],
                    suggested_fix: Some(format!(
                        "define a typed error enum (e.g. `#[derive(thiserror::Error)] \
                         enum {}Error {{ … }}`) and map the failure modes currently \
                         flattened into `{display_err_ty}` onto its variants; return \
                         that typed error from `{}` instead",
                        capitalize_first(&func.name),
                        func.name,
                    )),
                });
            }
        }
    }
    out
}

/// ER003 — boundary error type embedded as a field on a domain error enum.
///
/// For every `AirFile` whose `module_path` matches a pattern in
/// `domain_paths`, walk each `AirItem::Type` with `kind == Enum` and
/// inspect every variant's field's `type_text`. Fire one diagnostic per
/// matching field whose rendered type text matches any pattern in
/// `boundary_error_patterns`.
///
/// The matcher is the FL/DG style (segment-aligned wildcards). It runs
/// against the variant field's *raw* `type_text` — same source-of-truth
/// rendering used by FL001 against `Result<T, E>` signatures. Common
/// patterns:
///
/// - `"reqwest::Error"` — exact match
/// - `"sqlx::*"` — anything in the `sqlx` crate
/// - `"http::*"` — anything in the `http` crate
/// - `"std::io::Error"` — exact match
///
/// Severity: **Fatal** in both modes. Embedding a transport error as a
/// variant field is the structural mirror of FL001 (same error leaking
/// through a *return type*): the layer edge that should have wrapped the
/// boundary failure didn't, the failure has lost its owner, and now the
/// loss is encoded in the type system. `mode.elevate` is still applied
/// for symmetry, even though it's a no-op on Fatal.
///
/// Silent until **both** `domain_paths` and `boundary_error_patterns`
/// are populated. Mirrors FL001's onboarding posture.
pub fn er003(air: &AirWorkspace, section: &ErSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.domain_paths.is_empty() || section.boundary_error_patterns.is_empty() {
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
                let AirItem::Type(ty) = item else {
                    continue;
                };
                if ty.kind != locus_air::TypeKind::Enum {
                    continue;
                }
                for variant in &ty.variants {
                    for field in &variant.fields {
                        let Some(boundary_pattern) = section
                            .boundary_error_patterns
                            .iter()
                            .find(|pat| matches_pattern(pat, &field.type_text))
                        else {
                            continue;
                        };
                        out.push(Diagnostic {
                            rule_id: "ER003".to_string(),
                            severity: mode.elevate(Severity::Fatal),
                            span: ty.span.clone(),
                            concept: None,
                            message: format!(
                                "domain error `{}::{}` field has boundary error type `{}` \
                                 (matched domain pattern `{}`, boundary pattern `{}`)",
                                ty.name,
                                variant.name,
                                field.type_text,
                                domain_pattern,
                                boundary_pattern,
                            ),
                            why: vec![
                                format!(
                                    "module `{module_path}` matches domain pattern \
                                     `{domain_pattern}`"
                                ),
                                format!("enum `{}` (`{}`)", ty.name, ty.symbol),
                                format!(
                                    "variant `{}` has field `{}: {}`",
                                    variant.name,
                                    if field.name.is_empty() {
                                        "_"
                                    } else {
                                        field.name.as_str()
                                    },
                                    field.type_text,
                                ),
                                format!(
                                    "field type `{}` matches boundary pattern \
                                     `{boundary_pattern}`",
                                    field.type_text
                                ),
                                "domain error enums must speak the domain's failure \
                                 vocabulary; embedding a transport / boundary error as a \
                                 variant field buries the layer edge that should have \
                                 wrapped it"
                                    .into(),
                            ],
                            suggested_fix: Some(format!(
                                "wrap `{}` in a domain-shaped variant — replace the field \
                                 with a structured value capturing only the domain-relevant \
                                 facts (e.g. `Network {{ url: String }}` instead of \
                                 `Network(reqwest::Error)`), or add a separate boundary \
                                 error type at the adapter layer that converts to `{}` \
                                 via `From`",
                                field.type_text, ty.name,
                            )),
                        });
                    }
                }
            }
        }
    }
    out
}

/// ER007 — variant name shared across two or more `*Error*` enums.
///
/// Walks every `AirItem::Type` with `kind == Enum` whose name passes
/// [`has_error_suffix`] (matching ER001's `Error` / `Err` whole-word
/// suffix discipline). Records every `(file_path, type_name,
/// variant_name)` triple, then for each variant name appearing on
/// **two or more distinct error types** (different `type_name` *or*
/// different `file_path`) emits one diagnostic per occurrence beyond
/// the first, citing the incumbent error type that introduced the name.
///
/// The drift this catches: `enum UserError { NotFound, Invalid }`
/// living next to `enum OrderError { NotFound, Invalid }` — both
/// "not found" failures in the workspace look identical to a
/// caller, but they're modelled as two unrelated variants. The fix
/// is usually to extract a shared `enum DomainError` (or to give
/// each variant a more specific name).
///
/// Severity: `mode.elevate(Severity::Warning)`. The spec frames this
/// as drift, not a structural violation — Warning by default, Fatal
/// under `--agent-strict`. No lockfile fields; ER007 is heuristic and
/// always-on (suppress per-callsite via `// ot: allow ER007`).
///
/// Determinism note: results are gathered in workspace iteration
/// order (package → file → item → variant), and the "first" occurrence
/// is whichever the iterator yielded first. AIR ordering is stable
/// within a run, so a given `AirWorkspace` always produces the same
/// (incumbent, duplicates) split.
pub fn er007(air: &AirWorkspace, mode: CheckMode) -> Vec<Diagnostic> {
    use std::collections::HashMap;

    /// First-seen occurrence of a variant name in an `*Error*` enum.
    struct Incumbent<'a> {
        type_name: &'a str,
        file_path: &'a str,
    }

    let mut first_seen: HashMap<&str, Incumbent<'_>> = HashMap::new();
    let mut out = Vec::new();

    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                let AirItem::Type(ty) = item else {
                    continue;
                };
                if ty.kind != locus_air::TypeKind::Enum {
                    continue;
                }
                if !has_error_suffix(&ty.name) {
                    continue;
                }
                for variant in &ty.variants {
                    match first_seen.get(variant.name.as_str()) {
                        None => {
                            first_seen.insert(
                                variant.name.as_str(),
                                Incumbent {
                                    type_name: ty.name.as_str(),
                                    file_path: file.path.as_str(),
                                },
                            );
                        }
                        Some(incumbent) => {
                            // Only fire when the duplicate sits on a *different*
                            // error type (different type name or different file).
                            // Same enum redeclaring the same variant name isn't
                            // legal Rust and will be caught by `rustc`; we just
                            // skip it defensively.
                            if incumbent.type_name == ty.name.as_str()
                                && incumbent.file_path == file.path.as_str()
                            {
                                continue;
                            }
                            out.push(Diagnostic {
                                rule_id: "ER007".to_string(),
                                severity: mode.elevate(Severity::Warning),
                                span: ty.span.clone(),
                                concept: None,
                                message: format!(
                                    "duplicate error variant `{}` on `{}` — already \
                                     declared on `{}` in `{}`",
                                    variant.name, ty.name, incumbent.type_name, incumbent.file_path,
                                ),
                                why: vec![
                                    format!("variant `{}` declared on `{}`", variant.name, ty.name),
                                    format!(
                                        "incumbent: `{}::{}` in `{}`",
                                        incumbent.type_name, variant.name, incumbent.file_path,
                                    ),
                                    format!("current declaration in `{}`", file.path),
                                    "duplicate variants across error enums signal a drifting \
                                     taxonomy: the same failure mode is being modelled twice \
                                     under different types, so callers can't pattern-match \
                                     across the workspace's error surface"
                                        .into(),
                                ],
                                suggested_fix: Some(format!(
                                    "extract `{}` into a shared error type (`enum \
                                     DomainError {{ {} ,… }}`) and re-export it from both \
                                     `{}` and `{}`, or rename one of them to clarify the \
                                     distinct semantics. For an intentional duplication, \
                                     suppress with `// ot: allow ER007 reason=\"…\" \
                                     expires=\"YYYY-MM-DD\"`",
                                    variant.name, variant.name, incumbent.type_name, ty.name,
                                )),
                            });
                        }
                    }
                }
            }
        }
    }
    out
}

/// Extract the `E` from a top-level `Result<T, E>` rendered as a string.
///
/// Copied verbatim from the FL paradigm's helper of the same name — paradigms
/// share `locus-core`'s diagnostic + lockfile infrastructure but never
/// depend on each other (CLAUDE.md: paradigms must not import from siblings).
/// Keep the two implementations in sync if either has to evolve.
///
/// Returns `None` if the string isn't a top-level `Result<...>`, the angle
/// brackets don't balance, or the `<...>` body doesn't have a top-level
/// comma (e.g. `Result<T>` from a custom `Result` alias with one type
/// parameter — not what ER002 reasons about).
fn extract_result_error_type(rendered: &str) -> Option<&str> {
    let s = rendered.trim();
    let s = s.strip_prefix("::").unwrap_or(s);
    let inner = s.strip_prefix("Result<")?.strip_suffix('>')?;
    let mut depth: i32 = 0;
    let mut split_at: Option<usize> = None;
    for (idx, ch) in inner.char_indices() {
        match ch {
            '<' => depth += 1,
            '>' => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
            }
            ',' if depth == 0 => {
                split_at = Some(idx);
                break;
            }
            _ => {}
        }
    }
    let split_at = split_at?;
    let err_ty = inner[split_at + 1..].trim();
    if err_ty.is_empty() {
        None
    } else {
        Some(err_ty)
    }
}

/// Normalise an error-type rendering or pattern: trim whitespace and peel a
/// single leading `&` (so `&str` and `&MyErr` line up with bare patterns,
/// and a literal `"&str"` pattern still matches a `&str` return). Lifetimes
/// are deliberately left in place — a pattern like `"&'static str"` can be
/// spelled out if the user wants that level of specificity.
fn normalise_error_text(s: &str) -> String {
    let s = s.trim();
    let s = s.strip_prefix('&').unwrap_or(s).trim_start();
    s.to_string()
}

/// Single-`*` glob matcher used by ER002.
///
/// Splits the pattern on the first `*` and accepts any input that begins
/// with the prefix *and* ends with the suffix. A pattern without `*` must
/// match exactly. This is intentionally simpler than the `::`-segment
/// matcher used by FL/DG — error type renderings carry punctuation that
/// segment-based matchers stumble on (`Box<dyn Error>`, `&str`), and a
/// plain glob handles every recommended pattern shape (`"*::Error"`,
/// `"anyhow::*"`, `"Box<dyn *>"`, `"String"`, `"&str"`).
fn matches_error_pattern(pattern: &str, input: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == input,
        Some((prefix, suffix)) => {
            input.len() >= prefix.len() + suffix.len()
                && input.starts_with(prefix)
                && input.ends_with(suffix)
        }
    }
}

/// Capitalize the first character of `s` (ASCII-aware). Used to suggest a
/// `FooError` typed-enum name in ER002's `suggested_fix` for a function
/// called `foo`. Names that don't start with an ASCII letter are returned
/// unchanged.
fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {
            let mut out = String::with_capacity(s.len());
            out.push(c.to_ascii_uppercase());
            out.extend(chars);
            out
        }
        _ => s.to_string(),
    }
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
            facts: Vec::new(),
        }
    }

    #[test]
    fn er001_fires_when_file_has_two_error_types() {
        let air = air_with_file_items(
            "src/errors.rs",
            vec![pub_ty("UserError"), pub_ty("CreateUserError")],
        );
        let diags = er001(&air, &ErSection::default(), CheckMode::Human);
        assert_eq!(
            diags.len(),
            1,
            "two error types → one diagnostic on the second"
        );
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
        assert_eq!(
            diags.len(),
            2,
            "three error types → two duplicate diagnostics"
        );
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
        let air = air_with_file_items("src/io.rs", vec![pub_ty("IoErr"), pub_ty("ParseErr")]);
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
            facts: Vec::new(),
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

    // ---- ER002 tests ----

    fn func(name: &str, return_type: Option<&str>) -> AirItem {
        AirItem::Function(locus_air::AirFunction {
            name: name.into(),
            symbol: format!("x::ops::{name}"),
            visibility: Visibility::Public,
            params: Vec::new(),
            return_type: return_type.map(str::to_string),
            span: AirSpan::new("src/ops.rs", 10, 20),
            line_count: 5,
            doc: None,
        })
    }

    fn er002_section(patterns: &[&str]) -> ErSection {
        ErSection {
            forbidden_error_types: patterns.iter().map(|p| (*p).into()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn er002_fires_on_string_error_when_string_is_forbidden() {
        let air = air_with_file_items("src/ops.rs", vec![func("save", Some("Result<(), String>"))]);
        let section = er002_section(&["String"]);
        let diags = er002(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "ER002");
        assert_eq!(diags[0].severity, Severity::Fatal);
        assert!(diags[0].message.contains("save"));
        assert!(diags[0].message.contains("String"));
        assert!(
            diags[0]
                .why
                .iter()
                .any(|w| w.contains("Result<(), String>")),
            "why list should include the rendered return type; got: {:?}",
            diags[0].why
        );
        assert!(
            diags[0]
                .suggested_fix
                .as_deref()
                .unwrap_or("")
                .contains("thiserror::Error"),
            "suggested fix should mention the typed-enum pattern; got: {:?}",
            diags[0].suggested_fix
        );
    }

    #[test]
    fn er002_fires_on_anyhow_error_via_wildcard_pattern() {
        let air = air_with_file_items(
            "src/ops.rs",
            vec![func("load", Some("Result<User, anyhow::Error>"))],
        );
        let section = er002_section(&["anyhow::*"]);
        let diags = er002(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("anyhow::Error"));
        assert!(diags[0].message.contains("anyhow::*"));
    }

    #[test]
    fn er002_quiet_on_typed_error_not_in_forbidden_list() {
        let air = air_with_file_items(
            "src/ops.rs",
            vec![func("load", Some("Result<User, MyError>"))],
        );
        let section = er002_section(&["String", "anyhow::*", "Box<dyn *>"]);
        assert!(er002(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn er002_silent_when_forbidden_list_is_empty() {
        // Default ErSection has no forbidden patterns → ER002 must stay
        // entirely quiet, even on the most string-shaped function in the
        // workspace. This is the mandatory "silent-on-default" contract.
        let air = air_with_file_items(
            "src/ops.rs",
            vec![
                func("save", Some("Result<(), String>")),
                func("load", Some("Result<User, anyhow::Error>")),
            ],
        );
        assert!(er002(&air, &ErSection::default(), CheckMode::Human).is_empty());
    }

    #[test]
    fn er002_agent_strict_keeps_severity_fatal() {
        // Already Fatal in Human mode; AgentStrict must not change anything.
        let air = air_with_file_items("src/ops.rs", vec![func("save", Some("Result<(), String>"))]);
        let section = er002_section(&["String"]);
        let human = er002(&air, &section, CheckMode::Human);
        let strict = er002(&air, &section, CheckMode::AgentStrict);
        assert_eq!(human.len(), 1);
        assert_eq!(strict.len(), 1);
        assert_eq!(human[0].severity, Severity::Fatal);
        assert_eq!(strict[0].severity, Severity::Fatal);
    }

    #[test]
    fn er002_handles_nested_generics_in_ok_position() {
        // `Result<Vec<T>, String>` — naive comma split would land on the
        // `T>, String` fragment. The angle-bracket-aware extractor must
        // recover `String` as the error type.
        let air = air_with_file_items(
            "src/ops.rs",
            vec![func("collect_all", Some("Result<Vec<User>, String>"))],
        );
        let section = er002_section(&["String"]);
        let diags = er002(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert!(
            diags[0].message.contains("`String`"),
            "extracted error type should be String, not the Vec fragment; got: {}",
            diags[0].message
        );
    }

    #[test]
    fn er002_matches_box_dyn_error_via_wildcard() {
        // `"Box<dyn *>"` is the recommended pattern for any type-erased
        // `dyn Error`, including `Box<dyn std::error::Error + Send + Sync>`.
        let air = air_with_file_items(
            "src/ops.rs",
            vec![func(
                "run",
                Some("Result<(), Box<dyn std::error::Error + Send + Sync>>"),
            )],
        );
        let section = er002_section(&["Box<dyn *>"]);
        let diags = er002(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("Box<dyn *>"));
    }

    #[test]
    fn er002_strips_leading_ampersand_for_str_match() {
        // A function returning `Result<(), &str>` should match the literal
        // `"&str"` pattern (the leading `&` is preserved in the rendering)
        // and also a bare `"str"` pattern after the `&` is peeled.
        let air = air_with_file_items("src/ops.rs", vec![func("save", Some("Result<(), &str>"))]);
        let amp_section = er002_section(&["&str"]);
        assert_eq!(er002(&air, &amp_section, CheckMode::Human).len(), 1);
        let bare_section = er002_section(&["str"]);
        assert_eq!(er002(&air, &bare_section, CheckMode::Human).len(), 1);
    }

    #[test]
    fn er002_skips_non_result_returns() {
        let air = air_with_file_items(
            "src/ops.rs",
            vec![
                func("count", Some("u64")),
                func("noop", None),
                // Custom `Result<T>` alias with one type parameter — top-level
                // comma absent, so ER002 must skip it.
                func("custom_alias", Some("Result<User>")),
            ],
        );
        let section = er002_section(&["String", "anyhow::*", "*::Error"]);
        assert!(er002(&air, &section, CheckMode::Human).is_empty());
    }

    // ---- extract_result_error_type / matcher unit tests ----

    #[test]
    fn extract_result_error_type_basic_and_nested() {
        assert_eq!(
            extract_result_error_type("Result<User, String>"),
            Some("String")
        );
        assert_eq!(
            extract_result_error_type("Result<HashMap<UserId, User>, anyhow::Error>"),
            Some("anyhow::Error")
        );
        assert_eq!(extract_result_error_type("Result<User>"), None);
        assert_eq!(extract_result_error_type("u64"), None);
    }

    #[test]
    fn matches_error_pattern_exact_and_glob() {
        // No `*` → exact match only.
        assert!(matches_error_pattern("String", "String"));
        assert!(!matches_error_pattern("String", "Strings"));
        assert!(!matches_error_pattern("String", "MyString"));

        // Suffix wildcard.
        assert!(matches_error_pattern("anyhow::*", "anyhow::Error"));
        assert!(matches_error_pattern("anyhow::*", "anyhow::Result"));
        assert!(!matches_error_pattern("anyhow::*", "eyre::Report"));

        // Prefix wildcard. `"*::Error"` requires `"::Error"` as a literal
        // suffix, so a bare `MyError` (no `::`) does not match.
        assert!(matches_error_pattern("*::Error", "std::io::Error"));
        assert!(matches_error_pattern("*::Error", "x::Error"));
        assert!(!matches_error_pattern("*::Error", "MyError"));
        assert!(!matches_error_pattern("*::Error", "Error"));

        // Mid-pattern wildcard.
        assert!(matches_error_pattern("Box<dyn *>", "Box<dyn Error>"));
        assert!(matches_error_pattern(
            "Box<dyn *>",
            "Box<dyn std::error::Error + Send + Sync>"
        ));
        assert!(!matches_error_pattern("Box<dyn *>", "Arc<dyn Error>"));
    }

    // ---- ER003 helpers + tests ----

    fn enum_with_variants(name: &str, variants: Vec<(&str, Vec<&str>)>) -> AirItem {
        let air_variants: Vec<locus_air::AirVariant> = variants
            .into_iter()
            .map(|(vname, field_types)| locus_air::AirVariant {
                name: vname.into(),
                fields: field_types
                    .into_iter()
                    .enumerate()
                    .map(|(i, t)| locus_air::AirField {
                        name: format!("f{i}"),
                        type_text: t.into(),
                        visibility: Visibility::Public,
                    })
                    .collect(),
            })
            .collect();
        AirItem::Type(AirType {
            kind: TypeKind::Enum,
            name: name.into(),
            symbol: format!("crate::errors::{name}"),
            visibility: Visibility::Public,
            fields: Vec::new(),
            variants: air_variants,
            derives: Vec::new(),
            attrs: Vec::new(),
            span: AirSpan::new("src/errors.rs", 1, 1),
            doc: None,
        })
    }

    fn er003_section() -> ErSection {
        ErSection {
            domain_paths: vec!["x::domain::*".into()],
            boundary_error_patterns: vec![
                "reqwest::Error".into(),
                "sqlx::*".into(),
                "std::io::Error".into(),
            ],
            ..Default::default()
        }
    }

    fn air_with_module(file_path: &str, module: &str, items: Vec<AirItem>) -> AirWorkspace {
        AirWorkspace {
            schema_version: AIR_SCHEMA_VERSION,
            packages: vec![AirPackage {
                name: "x".into(),
                version: "0".into(),
                root_dir: "/".into(),
                files: vec![AirFile {
                    path: file_path.into(),
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
    fn er003_fires_on_boundary_field_in_domain_enum() {
        let air = air_with_module(
            "src/domain/user.rs",
            "x::domain::user",
            vec![enum_with_variants(
                "UserError",
                vec![("Network", vec!["reqwest::Error"]), ("NotFound", vec![])],
            )],
        );
        let diags = er003(&air, &er003_section(), CheckMode::Human);
        assert_eq!(diags.len(), 1, "expected one diag, got {diags:?}");
        assert_eq!(diags[0].rule_id, "ER003");
        assert_eq!(diags[0].severity, Severity::Fatal);
        assert!(diags[0].message.contains("UserError"));
        assert!(diags[0].message.contains("Network"));
        assert!(diags[0].message.contains("reqwest::Error"));
    }

    #[test]
    fn er003_fires_via_wildcard_boundary_pattern() {
        // `sqlx::*` matches `sqlx::postgres::PgError`.
        let air = air_with_module(
            "src/domain/orders.rs",
            "x::domain::orders",
            vec![enum_with_variants(
                "OrderError",
                vec![("Db", vec!["sqlx::postgres::PgError"])],
            )],
        );
        let diags = er003(&air, &er003_section(), CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("sqlx::postgres::PgError"));
        assert!(diags[0].message.contains("sqlx::*"));
    }

    #[test]
    fn er003_quiet_on_domain_only_field_types() {
        let air = air_with_module(
            "src/domain/user.rs",
            "x::domain::user",
            vec![enum_with_variants(
                "UserError",
                vec![("NotFound", vec![]), ("Invalid", vec!["String"])],
            )],
        );
        assert!(er003(&air, &er003_section(), CheckMode::Human).is_empty());
    }

    #[test]
    fn er003_quiet_outside_domain_paths() {
        // Same boundary error, but living in an adapter module — fine.
        let air = air_with_module(
            "src/adapters/http.rs",
            "x::adapters::http",
            vec![enum_with_variants(
                "HttpError",
                vec![("Network", vec!["reqwest::Error"])],
            )],
        );
        assert!(er003(&air, &er003_section(), CheckMode::Human).is_empty());
    }

    #[test]
    fn er003_silent_when_lockfile_lists_empty() {
        let air = air_with_module(
            "src/domain/user.rs",
            "x::domain::user",
            vec![enum_with_variants(
                "UserError",
                vec![("Network", vec!["reqwest::Error"])],
            )],
        );
        // domain only
        let only_domain = ErSection {
            domain_paths: vec!["x::domain::*".into()],
            ..Default::default()
        };
        assert!(er003(&air, &only_domain, CheckMode::Human).is_empty());
        // boundary only
        let only_boundary = ErSection {
            boundary_error_patterns: vec!["reqwest::Error".into()],
            ..Default::default()
        };
        assert!(er003(&air, &only_boundary, CheckMode::Human).is_empty());
        // default (both empty)
        assert!(er003(&air, &ErSection::default(), CheckMode::Human).is_empty());
    }

    #[test]
    fn er003_skips_struct_kinds() {
        // Only enum variants are inspected. A struct field with a boundary
        // type is out of ER003's scope (it would be flagged by other rules).
        let mut item = enum_with_variants("UserError", vec![("Network", vec!["reqwest::Error"])]);
        if let AirItem::Type(ref mut ty) = item {
            ty.kind = TypeKind::Struct;
        }
        let air = air_with_module("src/domain/user.rs", "x::domain::user", vec![item]);
        assert!(er003(&air, &er003_section(), CheckMode::Human).is_empty());
    }

    #[test]
    fn er003_emits_one_diag_per_offending_field() {
        let air = air_with_module(
            "src/domain/user.rs",
            "x::domain::user",
            vec![enum_with_variants(
                "UserError",
                vec![
                    ("Network", vec!["reqwest::Error"]),
                    ("Db", vec!["sqlx::Error"]),
                    ("Io", vec!["std::io::Error"]),
                    ("NotFound", vec![]),
                ],
            )],
        );
        let diags = er003(&air, &er003_section(), CheckMode::Human);
        assert_eq!(diags.len(), 3);
        let messages: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
        assert!(messages.iter().any(|m| m.contains("Network")));
        assert!(messages.iter().any(|m| m.contains("Db")));
        assert!(messages.iter().any(|m| m.contains("Io")));
    }

    // ---- ER007 tests ----

    fn er007_air(files: Vec<(&str, Vec<AirItem>)>) -> AirWorkspace {
        AirWorkspace {
            schema_version: AIR_SCHEMA_VERSION,
            packages: vec![AirPackage {
                name: "x".into(),
                version: "0".into(),
                root_dir: "/".into(),
                files: files
                    .into_iter()
                    .map(|(path, items)| AirFile {
                        path: path.into(),
                        module_path: Some("crate".into()),
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

    #[test]
    fn er007_fires_when_variant_name_repeats_across_error_enums() {
        let air = er007_air(vec![(
            "src/errors.rs",
            vec![
                enum_with_variants("UserError", vec![("NotFound", vec![]), ("Invalid", vec![])]),
                enum_with_variants(
                    "OrderError",
                    vec![("NotFound", vec![]), ("Cancelled", vec![])],
                ),
            ],
        )]);
        let diags = er007(&air, CheckMode::Human);
        // `NotFound` appears on UserError (incumbent) and OrderError → 1 diag.
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "ER007");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(diags[0].message.contains("NotFound"));
        assert!(diags[0].message.contains("OrderError"));
        assert!(diags[0].message.contains("UserError"));
    }

    #[test]
    fn er007_quiet_when_each_variant_unique() {
        let air = er007_air(vec![(
            "src/errors.rs",
            vec![
                enum_with_variants("UserError", vec![("NotFound", vec![])]),
                enum_with_variants("OrderError", vec![("Cancelled", vec![])]),
                enum_with_variants("BillingError", vec![("Declined", vec![])]),
            ],
        )]);
        assert!(er007(&air, CheckMode::Human).is_empty());
    }

    #[test]
    fn er007_skips_non_error_enums() {
        // `Status` enum shares variant names with `UserError` but isn't an
        // error type — must not trip ER007.
        let air = er007_air(vec![(
            "src/types.rs",
            vec![
                enum_with_variants("Status", vec![("Active", vec![]), ("NotFound", vec![])]),
                enum_with_variants("UserError", vec![("NotFound", vec![])]),
            ],
        )]);
        // Only `UserError::NotFound` is observed (Status is skipped); single
        // occurrence → no diagnostic.
        assert!(er007(&air, CheckMode::Human).is_empty());
    }

    #[test]
    fn er007_detects_duplicates_across_files() {
        let air = er007_air(vec![
            (
                "src/users.rs",
                vec![enum_with_variants("UserError", vec![("Invalid", vec![])])],
            ),
            (
                "src/orders.rs",
                vec![enum_with_variants("OrderError", vec![("Invalid", vec![])])],
            ),
        ]);
        let diags = er007(&air, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("Invalid"));
        assert!(
            diags[0].why.iter().any(|w| w.contains("src/users.rs")),
            "why list should reference the incumbent file; got: {:?}",
            diags[0].why,
        );
    }

    #[test]
    fn er007_emits_one_diag_per_extra_occurrence() {
        // `NotFound` appears on three error types → two extra occurrences,
        // one diagnostic each.
        let air = er007_air(vec![(
            "src/errors.rs",
            vec![
                enum_with_variants("UserError", vec![("NotFound", vec![])]),
                enum_with_variants("OrderError", vec![("NotFound", vec![])]),
                enum_with_variants("BillingError", vec![("NotFound", vec![])]),
            ],
        )]);
        let diags = er007(&air, CheckMode::Human);
        assert_eq!(diags.len(), 2);
        assert!(diags.iter().all(|d| d.rule_id == "ER007"));
    }

    #[test]
    fn er007_agent_strict_elevates_to_fatal() {
        let air = er007_air(vec![(
            "src/errors.rs",
            vec![
                enum_with_variants("UserError", vec![("NotFound", vec![])]),
                enum_with_variants("OrderError", vec![("NotFound", vec![])]),
            ],
        )]);
        let diags = er007(&air, CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }
}
