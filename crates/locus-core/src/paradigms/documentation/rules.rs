//! DC rule implementations.
//!
//! Implemented:
//! - [`dc001`]: public type or function has no doc comment. Heuristic
//!   baseline for documentation ownership — a public symbol with no
//!   `///` / `#[doc = "..."]` is an undocumented API surface, which the
//!   spec calls out as a failure of documentation ownership
//!   (`docs/PARADIGMS.md` §"Paradigm 17: Documentation / Comment
//!   Ownership").
//! - [`dc002`]: public type or function carries doc text containing a
//!   forbidden phrase from the lockfile's `forbidden_doc_phrases` list —
//!   high-signal LLM-transcript residue (`"as discussed"`,
//!   `"the prompt"`, …) and stale planning markers (`"TODO"`,
//!   `"for now"`, …). Inference-shaped: per-phrase confidence drives
//!   `Severity::from_confidence`.
//! - [`dc004`]: public type or function's doc text contains a
//!   `TODO`/`FIXME`/`HACK`/`XXX` marker without a parenthesised owner
//!   reference. Distinct from DC002 (which targets LLM-transcript residue
//!   phrases): DC004 only fires on markers that are *unowned* — present
//!   in the doc but with no `(name)` / `(#issue)` follow-up handle, which
//!   makes the reminder a permanent ghost no human can resolve.
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

/// DC002 — public item's doc comment contains a forbidden phrase.
///
/// Walks every `AirItem::Type` and `AirItem::Function` in the workspace
/// whose `doc.is_some()` and matches each entry in
/// `section.forbidden_doc_phrases` as a case-insensitive substring of the
/// doc text. One diagnostic per (item, phrase). The phrase's `confidence`
/// drives `Severity::from_confidence(confidence, mode)`; if that returns
/// `None` (confidence < 0.50) the diagnostic is skipped — supports
/// user-configured low-confidence demotions.
///
/// Stays silent when `forbidden_doc_phrases` is empty: clearing the list
/// is the documented opt-out. The default seed list is non-empty so users
/// get coverage out of the box.
///
/// Unlike DC001 this rule does not consult `require_public_docs` or
/// `exempt_paths` — DC001 catches the *absence* of docs (a project policy
/// choice), DC002 catches LLM-transcript residue *presence* in the doc
/// text users have already written, which is always a problem.
pub fn dc002(air: &AirWorkspace, section: &DcSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.forbidden_doc_phrases.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let module_label = file.module_path.as_deref().unwrap_or(&file.path);
            for item in &file.items {
                let (kind_label, name, symbol, doc, span, vis) = match item {
                    AirItem::Type(ty) => (
                        "type",
                        &ty.name,
                        &ty.symbol,
                        ty.doc.as_deref(),
                        ty.span.clone(),
                        ty.visibility,
                    ),
                    AirItem::Function(func) => (
                        "function",
                        &func.name,
                        &func.symbol,
                        func.doc.as_deref(),
                        func.span.clone(),
                        func.visibility,
                    ),
                    _ => continue,
                };
                if vis != Visibility::Public {
                    continue;
                }
                let Some(doc_text) = doc else {
                    continue;
                };
                let doc_lower = doc_text.to_lowercase();
                for forbidden in &section.forbidden_doc_phrases {
                    let Some(matched_alias) = matched_phrasing(&doc_lower, forbidden) else {
                        continue;
                    };
                    let Some(severity) = Severity::from_confidence(forbidden.confidence, mode)
                    else {
                        continue;
                    };
                    let primary = &forbidden.phrase;
                    let alias_note = if matched_alias == *primary {
                        String::new()
                    } else {
                        format!(" (alias of `{primary}`)")
                    };
                    out.push(Diagnostic {
                        rule_id: "DC002".to_string(),
                        severity,
                        span: span.clone(),
                        concept: None,
                        message: format!(
                            "public {kind_label} `{name}` in `{module_label}` has a doc \
                             comment containing forbidden phrase `{matched_alias}`{alias_note}"
                        ),
                        why: vec![
                            format!("{kind_label} `{name}` (`{symbol}`)"),
                            format!("matched phrase `{matched_alias}`{alias_note}"),
                            format!("phrase confidence {:.2}", forbidden.confidence),
                            "doc text contains phrase suggesting LLM transcript residue \
                             or stale planning notes"
                                .into(),
                        ],
                        suggested_fix: Some(format!(
                            "rewrite the doc comment on `{name}` to describe what the \
                             {kind_label} *is* and what invariant it carries — not the \
                             conversation it came from. If the marker is intentional \
                             (e.g. a tracked `TODO`) and you want to keep it, demote or \
                             remove the matching entry from \
                             `paradigms.DC.forbidden_doc_phrases`."
                        )),
                    });
                }
            }
        }
    }
    out
}

// locus: allow DC004 reason="docstring deliberately quotes the bare marker syntax DC004 fires on" expires="2099-01-01"
// locus: allow DC002 reason="docstring deliberately quotes residue-shaped planning markers as examples" expires="2099-01-01"
/// DC004 — public item's doc carries an owner-less follow-up marker.
///
/// For every `AirItem::Type` and `AirItem::Function` whose `doc.is_some()`
/// and `visibility == Public`, scan the doc text for occurrences of each
/// marker in `section.unowned_marker_patterns`. The marker match is
/// case-insensitive on the marker word itself; the owner check requires
/// an immediate `(` (no whitespace) after the marker text. Anything else
/// — bare marker followed by `:`, ` `, `,`, newline, end of doc — is
/// owner-less and fires DC004.
///
/// One diagnostic per (item, occurrence). Two bare markers in one doc
/// fire twice; an owned marker followed by an unowned one fires once.
/// The rule is deterministic — pattern-driven, no fuzzy matching.
///
/// Stays silent when `unowned_marker_patterns` is empty; clearing the
/// list is the documented opt-out.
///
/// Severity: Warning by default; Fatal under `--agent-strict` via
/// [`CheckMode::elevate`]. The "unowned reminder" tier is a softer signal
/// than DC002's residue phrases — an owned marker like `TODO(alice):` is
/// fine, the rule only flags the unowned shape.
pub fn dc004(air: &AirWorkspace, section: &DcSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.unowned_marker_patterns.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let module_label = file.module_path.as_deref().unwrap_or(&file.path);
            for item in &file.items {
                let (kind_label, name, symbol, doc, span, vis) = match item {
                    AirItem::Type(ty) => (
                        "type",
                        &ty.name,
                        &ty.symbol,
                        ty.doc.as_deref(),
                        ty.span.clone(),
                        ty.visibility,
                    ),
                    AirItem::Function(func) => (
                        "function",
                        &func.name,
                        &func.symbol,
                        func.doc.as_deref(),
                        func.span.clone(),
                        func.visibility,
                    ),
                    _ => continue,
                };
                if vis != Visibility::Public {
                    continue;
                }
                let Some(doc_text) = doc else {
                    continue;
                };
                for marker in &section.unowned_marker_patterns {
                    for occurrence in find_unowned_marker_occurrences(doc_text, marker) {
                        out.push(Diagnostic {
                            rule_id: "DC004".to_string(),
                            severity: mode.elevate(Severity::Warning),
                            span: span.clone(),
                            concept: None,
                            message: format!(
                                "public {kind_label} `{name}` in `{module_label}` has \
                                 a `{marker}` marker without an owner reference"
                            ),
                            why: vec![
                                format!("{kind_label} `{name}` (`{symbol}`)"),
                                format!("matched marker `{marker}` (case-insensitive)"),
                                format!(
                                    "marker is followed by `{}` — no `(owner)` handle",
                                    occurrence.trailing_preview
                                ),
                                "owner-less follow-up markers have no path to \
                                 resolution and accumulate as architectural debt"
                                    .into(),
                            ],
                            suggested_fix: Some(format!(
                                "rewrite the marker on `{name}` with an owner reference \
                                 (e.g. `{marker}(alice): ...` or `{marker}(#123): ...`) \
                                 so the reminder has a path to resolution; or remove the \
                                 marker if it's stale. To opt out for a region, add the \
                                 marker word to `paradigms.DC.unowned_marker_patterns` to \
                                 demote it (clearing the list disables DC004 entirely)."
                            )),
                        });
                    }
                }
            }
        }
    }
    out
}

/// One unowned-marker occurrence inside a doc string. `trailing_preview`
/// is a short snippet of what followed the marker (used in the
/// diagnostic's `why` so the user can see *why* the marker was flagged
/// — `TODO:` shows `:`, `TODO ` shows a space, end-of-doc shows
/// `<end>`).
struct UnownedMarker {
    trailing_preview: String,
}

/// Find every owner-less occurrence of `marker` inside `doc_text`. The
/// match is case-insensitive on the marker text; "owner-less" means the
/// character immediately after the marker is **not** `(`. End-of-string
/// counts as owner-less. Owned occurrences (`TODO(alice):`) are skipped
/// silently.
fn find_unowned_marker_occurrences(doc_text: &str, marker: &str) -> Vec<UnownedMarker> {
    let mut out = Vec::new();
    if marker.is_empty() {
        return out;
    }
    let marker_lower = marker.to_lowercase();
    let doc_lower = doc_text.to_lowercase();
    let marker_len = marker_lower.len();
    let mut start = 0;
    while let Some(rel) = doc_lower[start..].find(&marker_lower) {
        let abs = start + rel;
        let after = abs + marker_len;
        // Owned shape: marker immediately followed by `(`. No whitespace
        // tolerance — the spec is "(name) immediately after".
        let owned = doc_text.as_bytes().get(after) == Some(&b'(');
        if !owned {
            let trailing_preview = if after >= doc_text.len() {
                "<end>".to_string()
            } else {
                // Show up to the next 8 bytes (chars rounded), or until
                // a newline. Use char_indices to avoid splitting a UTF-8
                // boundary.
                let tail = &doc_text[after..];
                let stop = tail
                    .char_indices()
                    .take_while(|(byte_idx, ch)| *byte_idx < 8 && *ch != '\n')
                    .last()
                    .map(|(idx, ch)| idx + ch.len_utf8())
                    .unwrap_or(0);
                tail[..stop].to_string()
            };
            out.push(UnownedMarker { trailing_preview });
        }
        // Always advance past this match so we don't loop on it.
        start = after;
        if start >= doc_lower.len() {
            break;
        }
    }
    out
}

/// Try the primary `phrase` first, then each alias, against the
/// already-lowercased doc text. Returns the matched phrasing (in its
/// original casing) so the diagnostic can surface what the user wrote,
/// not just the seeded primary. `None` when nothing matched.
///
/// The alias mechanism is the no-LLM, deterministic substitute for
/// embedding-based paraphrase detection: every accepted variant is in
/// the lockfile, every match is a literal substring, every diagnostic
/// is reproducible from inputs alone.
fn matched_phrasing(
    doc_lower: &str,
    forbidden: &super::lockfile_schema::ForbiddenPhrase,
) -> Option<String> {
    if doc_lower.contains(&forbidden.phrase.to_lowercase()) {
        return Some(forbidden.phrase.clone());
    }
    for alias in &forbidden.aliases {
        if doc_lower.contains(&alias.to_lowercase()) {
            return Some(alias.clone());
        }
    }
    None
}

#[cfg(test)]
#[path = "rules_tests.rs"]
mod rules_tests;
