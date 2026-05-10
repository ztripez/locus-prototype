//! CL rule implementations.
//!
//! Implemented:
//! - [`cl001`]: doc comment cites an external reference (`#NN`, URL) but
//!   carries no local rationale. Heuristic: after stripping recognised
//!   reference tokens from the doc text, fewer than `MIN_RATIONALE_WORDS`
//!   word tokens remain.
//!
//! Future CL rules (CL002–CL006) need a richer text-claim AIR shape
//! covering free-floating comments, Markdown, script comments, and
//! generated-file headers. See `docs/superpowers/specs/2026-05-09-claim-ownership-paradigm.md`.

// locus: ot canonical

use locus_air::{AirItem, AirWorkspace};

use super::lockfile_schema::{ClSection, matches_pattern};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

/// Minimum non-reference word count for a doc block to count as having
/// "local rationale." Below this the references read as orphan citations.
const MIN_RATIONALE_WORDS: usize = 5;

/// CL001 — orphan external reference in a doc comment.
///
/// For each public type or function with a doc comment, scan the doc text
/// for recognised reference tokens (`#\d+` GitHub-style issue/PR refs,
/// `https?://...` URLs). If the text carries references but, after
/// stripping them, fewer than [`MIN_RATIONALE_WORDS`] word tokens remain,
/// fire CL001. The reference is then "orphan" — present but unbacked by
/// local explanation.
///
/// Returns no diagnostics when `section.require_local_rationale` is
/// `false` (the default). Files whose `module_path` matches any
/// `exempt_paths` entry skip the rule entirely.
///
/// Severity: Warning by default; Fatal under `--agent-strict` via
/// [`CheckMode::elevate`]. The toggle is the narrowing knob, so
/// elevation is straightforward — opting in IS the actionable signal.
///
/// Spec: `docs/superpowers/specs/2026-05-09-claim-ownership-paradigm.md`.
pub fn cl001(air: &AirWorkspace, section: &ClSection, mode: CheckMode) -> Vec<Diagnostic> {
    if !section.require_local_rationale {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            if let Some(mp) = file.module_path.as_deref()
                && section.exempt_paths.iter().any(|p| matches_pattern(p, mp))
            {
                continue;
            }
            for item in &file.items {
                let (doc, span, label) = match item {
                    AirItem::Type(t) => match &t.doc {
                        Some(d) => (d.as_str(), t.span.clone(), format!("type `{}`", t.symbol)),
                        None => continue,
                    },
                    AirItem::Function(f) => match &f.doc {
                        Some(d) => (
                            d.as_str(),
                            f.span.clone(),
                            format!("function `{}`", f.symbol),
                        ),
                        None => continue,
                    },
                    _ => continue,
                };
                let analysis = analyse_doc(doc);
                if analysis.references.is_empty() {
                    continue;
                }
                if analysis.non_reference_word_count >= MIN_RATIONALE_WORDS {
                    continue;
                }
                out.push(Diagnostic {
                    rule_id: "CL001".to_string(),
                    severity: mode.elevate(Severity::Warning),
                    span,
                    concept: None,
                    message: format!(
                        "{label} doc comment cites {ref_count} external reference(s) but \
                         carries no local rationale ({words} non-reference word(s); \
                         minimum {MIN_RATIONALE_WORDS})",
                        ref_count = analysis.references.len(),
                        words = analysis.non_reference_word_count,
                    ),
                    why: vec![
                        format!(
                            "doc text: `{}`",
                            doc.replace('\n', " ")
                                .trim()
                                .chars()
                                .take(120)
                                .collect::<String>(),
                        ),
                        format!(
                            "matched references: {}",
                            analysis
                                .references
                                .iter()
                                .map(|r| format!("`{r}`"))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                        "external references are traceability, not durable local \
                         rationale — readers shouldn't need to fetch the linked issue \
                         to understand why this code exists"
                            .into(),
                    ],
                    suggested_fix: Some(
                        "add a sentence in the doc comment explaining the local reason \
                         (`why this exists / why this shape`); the external reference \
                         can stay as a follow-up pointer"
                            .into(),
                    ),
                });
            }
        }
    }
    out
}

#[derive(Debug)]
struct DocAnalysis {
    references: Vec<String>,
    non_reference_word_count: usize,
}

/// Extract recognised reference tokens from a doc string and report how
/// much non-reference word content remains. Deterministic, no regex
/// engine: a hand-rolled UTF-8-aware scanner walks `char_indices` and
/// emits `(start, end)` byte ranges into the original string for
/// reference text and stripped (non-reference) text. Stays correct for
/// inputs with multi-byte characters like em dashes.
fn analyse_doc(doc: &str) -> DocAnalysis {
    let mut references: Vec<String> = Vec::new();
    let mut stripped = String::with_capacity(doc.len());

    let mut chars = doc.char_indices().peekable();
    let mut prev_char: Option<char> = None;
    while let Some(&(idx, ch)) = chars.peek() {
        // GitHub-style `#NNN` reference at a word boundary.
        if ch == '#' && prev_char.is_none_or(|c| !is_word_char(c)) {
            chars.next();
            let digits_start = idx + ch.len_utf8();
            let mut digits_end = digits_start;
            while let Some(&(_, c2)) = chars.peek()
                && c2.is_ascii_digit()
            {
                digits_end += c2.len_utf8();
                chars.next();
            }
            if digits_end > digits_start {
                references.push(format!("#{}", &doc[digits_start..digits_end]));
                prev_char = Some('#'); // any non-word char would do
                continue;
            }
            // No digits followed — `#` was not a reference. Keep it in
            // stripped text so the original word count isn't biased.
            stripped.push(ch);
            prev_char = Some(ch);
            continue;
        }
        // URL: `http://` or `https://` followed by non-whitespace.
        let remainder = &doc[idx..];
        let url_prefix_len = if remainder.starts_with("http://") {
            Some(7)
        } else if remainder.starts_with("https://") {
            Some(8)
        } else {
            None
        };
        if let Some(prefix_len) = url_prefix_len {
            // Advance past prefix.
            for _ in 0..remainder[..prefix_len].chars().count() {
                chars.next();
            }
            let mut url_end = idx + prefix_len;
            while let Some(&(_, c2)) = chars.peek() {
                if c2.is_whitespace() {
                    break;
                }
                url_end += c2.len_utf8();
                chars.next();
            }
            references.push(
                doc[idx..url_end]
                    .trim_end_matches(['.', ',', ')', '`'])
                    .to_string(),
            );
            prev_char = Some(' ');
            continue;
        }
        stripped.push(ch);
        prev_char = Some(ch);
        chars.next();
    }

    let non_reference_word_count = stripped
        .split_whitespace()
        .filter(|w| w.chars().any(|c| c.is_alphanumeric()))
        .count();

    DocAnalysis {
        references,
        non_reference_word_count,
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

#[cfg(test)]
#[path = "rules_tests.rs"]
mod rules_tests;
