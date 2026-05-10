//! ER rule implementations.
//!
//! Implemented:
//! - [`er001`]: multiple public error types in one file (taxonomy fork).
//! - [`er002`]: a `Result<_, E>` return type whose `E` matches a user-listed
//!   "string-shaped" / catch-all forbidden pattern (taxonomy collapse).
//! - [`er003`]: a domain error enum embeds a boundary error type as a
//!   variant field — structural taxonomy violation that buries the
//!   transport failure inside the domain vocabulary.
//! - [`er005`]: catch-all `Err(_)` arm body collapsing distinct errors
//!   into a single value (taxonomy-collapse view of the same shape FL007
//!   sees).
//! - [`er007`]: a variant name appears on two or more `*Error*` enums in
//!   the workspace — the taxonomy is drifting / duplicating.
//!
//! ER001 is heuristic and lockfile-free — it operates purely on AIR and the
//! `Error`/`Err` name suffix convention. ER002 is lockfile-driven via
//! [`ErSection::forbidden_error_types`]; it stays silent until that list is
//! populated. ER003 is lockfile-driven via [`ErSection::domain_paths`] +
//! [`ErSection::boundary_error_patterns`]; silent until both are populated.
//! ER005 is lockfile-driven via [`ErSection::error_collapse_owner_paths`];
//! silent until populated. ER007 is heuristic and lockfile-free.

use locus_air::{AirItem, AirMatchArm, AirWorkspace, ArmBodyShape, Visibility};

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

/// ER005 — catch-all errors hiding domain errors.
///
/// For every `AirItem::MatchArm` whose pattern is an `Err`-shaped
/// catch-all (`Err(_)`, `Err(MyError(_, _))`, etc. — any pattern
/// containing a wildcard binder *and* starting with `Err` or
/// containing `Err(`) **and** whose body shape is `Empty`, `Literal`,
/// or `Call` (i.e. a silent / default-producing arm), fire one
/// diagnostic. The arm collapses every distinct error variant into a
/// single value: the failure taxonomy is being flattened at this point
/// and callers can't pattern-match on the cause anymore.
///
/// Distinct from FL007: FL007 reads the same arm shape through the
/// "silent swallow / failure-lineage loss" lens; ER005 reads it through
/// the "error-taxonomy collapse" lens. Same fact, two paradigm angles.
///
/// Suppression: lockfile-driven via [`ErSection::error_collapse_owner_paths`].
/// A module is suppressed when either the file's `module_path` matches
/// or the enclosing function's symbol matches (the segment-anywhere
/// matcher catches both forms — inline `mod tests {}` carve-outs work
/// without a separate `containing_module_of` helper). Default empty;
/// ER005 stays silent until the user populates the list.
///
/// Severity: `mode.elevate(Severity::Warning)`. Same arm shape FL007
/// fires on, different angle — Warning is the right baseline; agent-strict
/// elevates to Fatal. ER005 is heuristic by construction (the `Call`
/// body shape covers `Default::default()` *and* `MyError::generic()` —
/// the rule can't tell them apart, and that's the entire point).
pub fn er005(air: &AirWorkspace, section: &ErSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.error_collapse_owner_paths.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            for item in &file.items {
                let AirItem::MatchArm(arm) = item else {
                    continue;
                };
                if !is_err_catchall_pattern(arm) {
                    continue;
                }
                if !is_collapse_body_shape(arm.body_shape) {
                    continue;
                }
                if arm_in_collapse_owner(
                    module_path,
                    arm.function.as_deref(),
                    &section.error_collapse_owner_paths,
                ) {
                    continue;
                }
                out.push(diagnostic_for_er005(arm, module_path, mode));
            }
        }
    }
    out
}

/// True when `arm` is a wildcard-bearing `Err(...)` pattern. Both
/// `pattern_has_wildcard` and an `Err`-shaped pattern text must hold —
/// a bare `_` arm (FL011's territory) is rejected here.
fn is_err_catchall_pattern(arm: &AirMatchArm) -> bool {
    if !arm.pattern_has_wildcard {
        return false;
    }
    let pat = arm.pattern.as_str();
    pat.starts_with("Err") || pat.contains("Err(")
}

/// True when the arm body produces a single generic value: unit / empty
/// block (`Empty`), bare literal (`Literal`), or a single function/method
/// call (`Call`). Anything else (`Return`, `Propagate`, `Block`, `Other`)
/// is doing real work and the rule shouldn't pre-judge.
fn is_collapse_body_shape(shape: ArmBodyShape) -> bool {
    matches!(
        shape,
        ArmBodyShape::Empty | ArmBodyShape::Literal | ArmBodyShape::Call
    )
}

/// File-level OR function-symbol-level collapse-owner match. The
/// segment-anywhere matcher (`*::tests::*`) lines up against both forms,
/// so inline `mod tests {}` carve-outs work without a separate
/// `containing_module_of` helper.
fn arm_in_collapse_owner(
    file_module: &str,
    function_symbol: Option<&str>,
    patterns: &[String],
) -> bool {
    if patterns.iter().any(|p| matches_pattern(p, file_module)) {
        return true;
    }
    if let Some(sym) = function_symbol
        && patterns.iter().any(|p| matches_pattern(p, sym))
    {
        return true;
    }
    false
}

fn diagnostic_for_er005(arm: &AirMatchArm, module_path: &str, mode: CheckMode) -> Diagnostic {
    let function_label = arm
        .function
        .as_deref()
        .unwrap_or("<unknown enclosing function>");
    let body_shape_label = match arm.body_shape {
        ArmBodyShape::Empty => "empty",
        ArmBodyShape::Literal => "literal",
        ArmBodyShape::Call => "call",
        // Defensive: filtered out earlier, but keep a label so the
        // diagnostic still renders if `is_collapse_body_shape` ever
        // grows.
        ArmBodyShape::Return => "return",
        ArmBodyShape::ErrorPropagation => "propagate",
        ArmBodyShape::Block => "block",
        ArmBodyShape::Other => "other",
    };
    Diagnostic {
        rule_id: "ER005".to_string(),
        severity: mode.elevate(Severity::Warning),
        span: arm.span.clone(),
        concept: None,
        message: format!(
            "catch-all `Err(_) => {body_shape_label}` in `{module_path}` (fn `{function_label}`) \
             collapses distinct error variants into a single value"
        ),
        why: vec![
            format!("module `{module_path}`"),
            format!("function `{function_label}`"),
            format!("arm pattern `{}` matches every `Err` variant", arm.pattern),
            format!("arm body is a `{body_shape_label}` — distinct error causes are erased"),
            "the error taxonomy is being flattened at this point".into(),
        ],
        suggested_fix: Some(format!(
            "enumerate the specific Err variants the caller cares about (`Err(MyError::A) => …, \
             Err(MyError::B) => …`), or wrap each into a typed error before mapping. If \
             `{module_path}` is a presentation/edge layer where collapsing is intentional, \
             accept it via `paradigms.ER.error_collapse_owner_paths`. For a one-off, suppress \
             with `// locus: allow ER005 reason=\"…\" expires=\"YYYY-MM-DD\"`"
        )),
    }
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
/// always-on (suppress per-callsite via `// locus: allow ER007`).
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
                                     suppress with `// locus: allow ER007 reason=\"…\" \
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
#[path = "rules_tests.rs"]
mod rules_tests;
