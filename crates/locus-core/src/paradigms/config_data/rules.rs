//! CF rules.
//!
//! Implemented:
//! - [`cf001`]: environment-variable read outside the config layer. Reads
//!   the workspace-level `AirFact` list — specifically `FactKind::ConfigRead`
//!   facts produced by the std-rt loader (or any other loader that knows
//!   about env-read patterns) — and pairs each with the file the targeted
//!   function lives in.
//! - [`cf002`]: magic decision constant in scrutinee outside the config
//!   layer. Scans `AirItem::ScrutineeLiteral` items emitted by the visitor
//!   for literal-pattern match arms and `==`/`!=` comparisons; fires on
//!   `Str | Int | Float` literals (configurable via
//!   `forbidden_literal_kinds`) when the enclosing module isn't a declared
//!   config owner. The historical filesystem-walk concept stays as a
//!   *future* direction — `config_file_patterns` /
//!   `accepted_config_files` lockfile fields are kept so that allowlist
//!   survives if a filesystem-aware loader ever lands.
//! - [`cf003`]: hardcoded provider/model/topic ID outside the config
//!   layer. A more-specific shape than CF002: fires only on `Str`
//!   literals whose (unquoted) value matches a user-declared
//!   `forbidden_id_patterns` allowlist.

use locus_air::{
    AirFact, AirItem, AirScrutineeLiteral, AirWorkspace, FactKind, FactTarget, LiteralContext,
    LiteralKind,
};

use super::lockfile_schema::{CfSection, matches_pattern};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

/// CF001 — environment-variable read outside the config layer.
///
/// For every `FactKind::ConfigRead` fact produced by a loader, look up the
/// targeted function's file and fire when the file's `module_path` does
/// *not* match any pattern in `config_paths`.
///
/// Always Fatal: ownership of decision-data is structural — an env read in
/// a handler is hidden config ownership, the exact failure mode the
/// paradigm exists to prevent. Files that legitimately load configuration
/// declare themselves via `config_paths`.
///
/// Silent until `config_paths` is populated: like DG/UT/BO, CF is a user
/// assertion, not an inference. No `config_paths` means the user hasn't
/// declared a config layer yet, and the rule has nothing to reason about.
pub fn cf001(air: &AirWorkspace, section: &CfSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.config_paths.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for fact in &air.facts {
        if fact.kind != FactKind::ConfigRead {
            continue;
        }
        let FactTarget::Function { symbol } = &fact.target else {
            // Non-Function targets aren't paired to a module path here;
            // CF001 needs a module to evaluate against `config_paths`.
            continue;
        };
        let Some((module_path, fn_span)) = lookup_function(air, symbol) else {
            continue;
        };
        if section
            .config_paths
            .iter()
            .any(|pat| matches_pattern(pat, module_path))
        {
            continue;
        }
        out.push(diagnostic_for(fact, symbol, module_path, fn_span, mode));
    }
    out
}

/// CF002 — magic decision constant in scrutinee outside the config layer.
///
/// Scans every `AirItem::ScrutineeLiteral` (emitted by the visitor for
/// literal-pattern match arms and binary `==`/`!=` comparisons against
/// literals). Fires when:
///
/// 1. The literal's `kind` is in `section.forbidden_literal_kinds`
///    (defaults to `str`/`int`/`float` — `bool` is excluded by default
///    because `if x == true` patterns are noise, not magic decision
///    constants).
/// 2. The literal's enclosing file has a `module_path` that does *not*
///    match any pattern in `section.config_paths`.
///
/// Severity: Warning (elevated to Fatal under `--agent-strict`).
/// Magic-constant decisions in handler code are smelly but not always
/// wrong (test fixtures, hardcoded protocol constants). Suppress
/// individual cases with `// locus: allow CF002 reason="…"` rather than
/// broadening the lockfile.
///
/// Silent when `config_paths` is empty (no declared config layer to
/// reason about) or `forbidden_literal_kinds` is empty (user explicitly
/// disabled the rule).
pub fn cf002(air: &AirWorkspace, section: &CfSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.config_paths.is_empty() || section.forbidden_literal_kinds.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            if section
                .config_paths
                .iter()
                .any(|pat| matches_pattern(pat, module_path))
            {
                continue;
            }
            for item in &file.items {
                let AirItem::ScrutineeLiteral(lit) = item else {
                    continue;
                };
                let kind_label = literal_kind_label(lit.kind);
                if !section
                    .forbidden_literal_kinds
                    .iter()
                    .any(|k| k.eq_ignore_ascii_case(kind_label))
                {
                    continue;
                }
                out.push(cf002_diagnostic(lit, module_path, kind_label, mode));
            }
        }
    }
    out
}

/// CF003 — hardcoded provider/model/topic ID outside the config layer.
///
/// More specific than CF002: only fires on `Str`-kind scrutinee literals
/// whose (unquoted) value matches a user-declared `forbidden_id_patterns`
/// glob. Use it when CF002 is too noisy and you only want to police
/// model / provider / topic / queue IDs.
///
/// Severity: Warning (elevated to Fatal under `--agent-strict`).
///
/// Silent until BOTH `config_paths` and `forbidden_id_patterns` are
/// populated. Pattern matching uses the paradigm-local
/// [`matches_pattern`] helper, which falls through to a
/// character-glob (`gpt-*`, `*-events`, `*topic*`) for non-`::` ID
/// shapes.
pub fn cf003(air: &AirWorkspace, section: &CfSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.config_paths.is_empty() || section.forbidden_id_patterns.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            if section
                .config_paths
                .iter()
                .any(|pat| matches_pattern(pat, module_path))
            {
                continue;
            }
            for item in &file.items {
                let AirItem::ScrutineeLiteral(lit) = item else {
                    continue;
                };
                if lit.kind != LiteralKind::Str {
                    continue;
                }
                let unquoted = strip_string_quotes(&lit.value);
                let Some(matched_pattern) = section
                    .forbidden_id_patterns
                    .iter()
                    .find(|pat| matches_pattern(pat, unquoted))
                else {
                    continue;
                };
                out.push(cf003_diagnostic(lit, module_path, matched_pattern, mode));
            }
        }
    }
    out
}

/// Find the `(module_path, function_span)` for the function with this
/// symbol. Returns `None` when the symbol isn't found, when the file has
/// no resolved module path, or when neither is available. Walks every
/// package/file/item — fine at the scale we operate on; a precomputed
/// index can replace this if it ever shows up hot in profiling.
fn lookup_function<'a>(
    air: &'a AirWorkspace,
    symbol: &str,
) -> Option<(&'a str, locus_air::AirSpan)> {
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                if let AirItem::Function(f) = item
                    && f.symbol == symbol
                {
                    let module = file.module_path.as_deref()?;
                    return Some((module, f.span.clone()));
                }
            }
        }
    }
    None
}

fn diagnostic_for(
    fact: &AirFact,
    symbol: &str,
    module_path: &str,
    fn_span: locus_air::AirSpan,
    mode: CheckMode,
) -> Diagnostic {
    // FactTarget::Span carries a precise call-site span; otherwise fall
    // back to the enclosing function's span (still useful for fix
    // targeting).
    let span = match &fact.target {
        FactTarget::Span(s) => s.clone(),
        FactTarget::Function { .. } | FactTarget::File { .. } => fn_span,
    };
    let function_label = symbol;
    let why_reasons = if fact.reasons.is_empty() {
        vec!["loader detected env-read pattern".to_string()]
    } else {
        fact.reasons.clone()
    };
    Diagnostic {
        rule_id: "CF001".to_string(),
        severity: mode.elevate(Severity::Fatal),
        span,
        concept: None,
        message: format!(
            "module `{module_path}` reads an environment variable from \
             `{function_label}` outside the config layer"
        ),
        why: {
            let mut w = vec![format!(
                "module `{module_path}` does not match any \
                 `paradigms.CF.config_paths` pattern"
            )];
            for r in why_reasons {
                w.push(r);
            }
            w.push(format!("enclosing function: `{function_label}`"));
            w
        },
        suggested_fix: Some(
            "move the env read into a config-layer module (one accepted \
             loader) and pass the resolved value through dependency \
             injection; if this file is the legitimate config owner, \
             add its module pattern to `paradigms.CF.config_paths` in \
             `locus.lock`"
                .into(),
        ),
    }
}

fn cf002_diagnostic(
    lit: &AirScrutineeLiteral,
    module_path: &str,
    kind_label: &str,
    mode: CheckMode,
) -> Diagnostic {
    let function_label = lit.function.as_deref().unwrap_or("<unknown>");
    let context_label = literal_context_label(lit.context);
    Diagnostic {
        rule_id: "CF002".to_string(),
        severity: mode.elevate(Severity::Warning),
        span: lit.span.clone(),
        concept: None,
        message: format!(
            "magic {kind_label} literal `{value}` in `{module_path}` (fn \
             `{function_label}`) — decision constants belong in the config \
             layer",
            value = lit.value
        ),
        why: vec![
            format!("module: `{module_path}`"),
            format!("function: `{function_label}`"),
            format!("literal value: `{}`", lit.value),
            format!("context: {context_label}"),
            format!(
                "module `{module_path}` does not match \
                 `paradigms.CF.config_paths`"
            ),
            "behavior-shaping decision data should live in declared \
             configuration, not embedded as a literal in execution code"
                .into(),
        ],
        suggested_fix: Some(format!(
            "move the literal into a config struct loaded by the config \
             layer; or, if `{module_path}` is a legitimate config owner, \
             add it to `paradigms.CF.config_paths` in `locus.lock`. For \
             one-off intentional uses (test fixtures, hardcoded protocol \
             constants), suppress with `// locus: allow CF002 reason=\"…\" \
             expires=\"YYYY-MM-DD\"`"
        )),
    }
}

fn cf003_diagnostic(
    lit: &AirScrutineeLiteral,
    module_path: &str,
    matched_pattern: &str,
    mode: CheckMode,
) -> Diagnostic {
    let function_label = lit.function.as_deref().unwrap_or("<unknown>");
    let context_label = literal_context_label(lit.context);
    Diagnostic {
        rule_id: "CF003".to_string(),
        severity: mode.elevate(Severity::Warning),
        span: lit.span.clone(),
        concept: None,
        message: format!(
            "hardcoded ID `{value}` in `{module_path}` (fn \
             `{function_label}`) matches forbidden pattern \
             `{matched_pattern}` — provider/model/topic IDs belong in \
             config",
            value = lit.value
        ),
        why: vec![
            format!("module: `{module_path}`"),
            format!("function: `{function_label}`"),
            format!("literal value: `{}`", lit.value),
            format!("context: {context_label}"),
            format!(
                "value matches `paradigms.CF.forbidden_id_patterns` \
                 entry `{matched_pattern}`"
            ),
            format!(
                "module `{module_path}` does not match \
                 `paradigms.CF.config_paths`"
            ),
            "provider/model/topic identifiers are deployment-shaped \
             configuration; embedding them in execution code couples \
             the wrong layers"
                .into(),
        ],
        suggested_fix: Some(format!(
            "load the ID from a config struct owned by the config layer; \
             or, if `{module_path}` legitimately owns the value, add it \
             to `paradigms.CF.config_paths` in `locus.lock`. For one-off \
             intentional uses, suppress with `// locus: allow CF003 \
             reason=\"…\" expires=\"YYYY-MM-DD\"`"
        )),
    }
}

fn literal_kind_label(kind: LiteralKind) -> &'static str {
    match kind {
        LiteralKind::Str => "str",
        LiteralKind::Int => "int",
        LiteralKind::Float => "float",
        LiteralKind::Bool => "bool",
    }
}

fn literal_context_label(ctx: LiteralContext) -> &'static str {
    match ctx {
        LiteralContext::MatchArm => "MatchArm",
        LiteralContext::BinaryCompare => "BinaryCompare",
    }
}

/// String-literal AIR values keep their surrounding quote characters so
/// `"42"` stays distinguishable from `42`. CF003 pattern-matches against
/// the *content*, not the quoted form, so strip a single leading and
/// trailing `"` if both are present. Anything else is returned as-is.
fn strip_string_quotes(value: &str) -> &str {
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

#[cfg(test)]
#[path = "rules_tests.rs"]
mod rules_tests;
