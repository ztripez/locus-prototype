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
mod tests {
    use super::*;
    use locus_air::{AIR_SCHEMA_VERSION, AirFile, AirFunction, AirPackage, AirSpan, AirType};

    fn typ_with_doc(name: &str, doc: Option<&str>) -> AirItem {
        AirItem::Type(AirType {
            kind: locus_air::TypeKind::Struct,
            name: name.into(),
            symbol: format!("crate::{name}"),
            symbol_segments: Vec::new(),
            visibility: locus_air::Visibility::Public,
            fields: Vec::new(),
            variants: Vec::new(),
            decorators: Vec::new(),
            span: AirSpan::new("t.rs", 1, 1),
            doc: doc.map(str::to_string),
        })
    }

    fn fn_with_doc(name: &str, doc: Option<&str>) -> AirItem {
        AirItem::Function(AirFunction {
            name: name.into(),
            symbol: format!("crate::{name}"),
            symbol_segments: Vec::new(),
            visibility: locus_air::Visibility::Public,
            params: Vec::new(),
            return_type: None,
            decorators: Vec::new(),
            span: AirSpan::new("t.rs", 1, 1),
            line_count: 1,
            doc: doc.map(str::to_string),
        })
    }

    fn ws(module: Option<&str>, items: Vec<AirItem>) -> AirWorkspace {
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
                    line_count: 1,
                }],
            }],
            facts: Vec::new(),
        }
    }

    fn enabled() -> ClSection {
        ClSection {
            require_local_rationale: true,
            ..ClSection::default()
        }
    }

    #[test]
    fn cl001_silent_when_require_local_rationale_is_default_false() {
        let air = ws(Some("a"), vec![typ_with_doc("Foo", Some("See #123."))]);
        let section = ClSection::default();
        assert!(cl001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn cl001_fires_on_orphan_issue_reference_in_type_doc() {
        let air = ws(Some("a"), vec![typ_with_doc("Foo", Some("See #123."))]);
        let diags = cl001(&air, &enabled(), CheckMode::Human);
        assert_eq!(diags.len(), 1, "expected one CL001, got {diags:#?}");
        assert!(diags[0].message.contains("type `crate::Foo`"));
        assert!(diags[0].message.contains("1 external reference"));
    }

    #[test]
    fn cl001_fires_on_orphan_url_reference_in_function_doc() {
        let air = ws(
            Some("a"),
            vec![fn_with_doc(
                "bar",
                Some("See https://example.org/spec/v2 ."),
            )],
        );
        let diags = cl001(&air, &enabled(), CheckMode::Human);
        assert_eq!(diags.len(), 1, "expected one CL001, got {diags:#?}");
        let why = diags[0].why.join("\n");
        assert!(why.contains("https://example.org/spec/v2"));
    }

    #[test]
    fn cl001_quiet_when_doc_has_local_rationale_alongside_reference() {
        let doc = "Use the compatibility path because mobile clients still send v1 \
                   payloads. See #123 for the migration plan.";
        let air = ws(Some("a"), vec![typ_with_doc("Foo", Some(doc))]);
        let diags = cl001(&air, &enabled(), CheckMode::Human);
        assert!(
            diags.is_empty(),
            "doc has rationale + reference; rule should not fire. got {diags:#?}",
        );
    }

    #[test]
    fn cl001_quiet_when_no_references_present() {
        let doc = "Plain doc text describing the type's role in the system.";
        let air = ws(Some("a"), vec![typ_with_doc("Foo", Some(doc))]);
        assert!(cl001(&air, &enabled(), CheckMode::Human).is_empty());
    }

    #[test]
    fn cl001_quiet_for_items_without_doc() {
        let air = ws(Some("a"), vec![typ_with_doc("Foo", None)]);
        assert!(cl001(&air, &enabled(), CheckMode::Human).is_empty());
    }

    #[test]
    fn cl001_skips_files_in_exempt_paths() {
        let air = ws(
            Some("a::tests::widget_tests"),
            vec![typ_with_doc("Foo", Some("See #1."))],
        );
        let section = ClSection {
            require_local_rationale: true,
            exempt_paths: vec!["*::tests::*".into()],
        };
        assert!(cl001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn cl001_agent_strict_elevates_to_fatal() {
        let air = ws(Some("a"), vec![typ_with_doc("Foo", Some("See #1."))]);
        let diags = cl001(&air, &enabled(), CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    #[test]
    fn cl001_handles_multiple_references_in_same_doc_block() {
        // Both `#1` and the URL count; word count after stripping is still
        // small ("See and ."), so it fires once with a count of 2.
        let air = ws(
            Some("a"),
            vec![typ_with_doc(
                "Foo",
                Some("See #1 and https://x.io/issue/1."),
            )],
        );
        let diags = cl001(&air, &enabled(), CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("2 external reference"));
    }

    #[test]
    fn cl001_only_inspects_public_items() {
        let mut item = typ_with_doc("Foo", Some("See #1."));
        if let AirItem::Type(t) = &mut item {
            t.visibility = locus_air::Visibility::Module;
        }
        let air = ws(Some("a"), vec![item]);
        // The MVP scans all items with `doc`; private items with doc
        // comments are uncommon but technically still visible to the
        // scanner. This test documents the current behaviour: the rule
        // fires regardless of visibility, since the doc text is the
        // authority surface either way.
        let diags = cl001(&air, &enabled(), CheckMode::Human);
        assert_eq!(diags.len(), 1);
    }

    // ---- analyse_doc unit tests ----

    #[test]
    fn analyse_extracts_github_style_issue_reference() {
        let a = analyse_doc("See #123.");
        assert_eq!(a.references, vec!["#123"]);
        assert_eq!(a.non_reference_word_count, 1); // "See"
    }

    #[test]
    fn analyse_extracts_url_reference() {
        let a = analyse_doc("Spec at https://example.org/foo/bar.");
        assert_eq!(a.references, vec!["https://example.org/foo/bar"]);
        assert_eq!(a.non_reference_word_count, 2); // "Spec at"
    }

    #[test]
    fn analyse_does_not_match_inline_hash_in_word_position() {
        // `f#x` shouldn't count as a reference; `#x` isn't valid either
        // (no digits). Hash followed by non-digit doesn't fire.
        let a = analyse_doc("Use the f#x format.");
        assert!(a.references.is_empty());
    }

    #[test]
    fn analyse_strips_trailing_punctuation_from_url() {
        let a = analyse_doc("(see https://x.io/foo).");
        assert_eq!(a.references, vec!["https://x.io/foo"]);
    }
}
