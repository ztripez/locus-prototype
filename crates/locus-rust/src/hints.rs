//! Source-annotation scanner.
//!
//! `syn` strips comments, so Locus annotations are extracted from a raw line
//! scan. Each `// locus: ...` comment binds to the *next* non-blank,
//! non-comment line — that line's number is recorded as the target span.
//!
//! Public syntax is intentionally single-prefix:
//!
//! - `// locus: ot canonical`
//! - `// locus: ot boundary <concept> <boundary>`
//! - `// locus: ot converter`
//! - `// locus: ot protocol-translation reason="..."`
//! - `// locus: ot generated-boundary`
//! - `// locus: allow <RULE> reason="..." expires="YYYY-MM-DD"`
//! - `// locus: fact <fact_kind>`
//!
//! The old `// ot:` namespace is intentionally not supported. Locus has not
//! had a public release yet, so keeping dual syntax would only preserve a
//! misleading historical accident.

use locus_air::{AirHint, AirSpan, HintKind};

const HINT_PREFIX: &str = "// locus:";

pub fn scan_hints(source: &str, file: &str) -> Vec<AirHint> {
    let lines: Vec<&str> = source.lines().collect();
    let mut hints = Vec::new();
    // Skip multi-line raw-string blocks (`r#"..."#` / `r##"..."##` / …) so
    // `// locus:` text appearing inside a string literal — common in this
    // crate's own unit tests via `indoc!` — is not mistaken for a real hint.
    // Single-line raw strings open and close on the same line; we let those
    // through (rare and the line-start prefix check usually filters them).
    let mut in_raw_string = false;

    for (idx, raw_line) in lines.iter().enumerate() {
        if in_raw_string {
            if raw_line.contains("\"#") {
                in_raw_string = false;
            }
            continue;
        }
        if let Some(open) = raw_line.find("r#\"") {
            // If the same line also closes after the open, the raw string is
            // contained — don't enter raw-string mode. Otherwise enter it.
            if raw_line[open + 3..].contains("\"#") {
                // contained
            } else {
                in_raw_string = true;
                continue;
            }
        }

        let trimmed = raw_line.trim_start();
        if !trimmed.starts_with(HINT_PREFIX) {
            continue;
        }
        let body = trimmed[HINT_PREFIX.len()..].trim();
        let kind = parse_hint_body(body);
        let line = (idx as u32) + 1;
        let target_span = next_target_span(&lines, idx, file);
        hints.push(AirHint {
            kind,
            raw: trimmed.to_string(),
            span: AirSpan::new(file, line, line),
            target_span,
        });
    }

    hints
}

fn next_target_span(lines: &[&str], from_idx: usize, file: &str) -> Option<AirSpan> {
    // Skip blanks, line comments, and outer/inner attribute lines so that
    // `// locus: ot canonical` placed above `#[derive(...)] pub struct X` still
    // binds to the struct, not to the derive. Multi-line attrs are not
    // tracked across lines — keep them on one line, or place the hint after.
    for (i, line) in lines.iter().enumerate().skip(from_idx + 1) {
        let t = line.trim_start();
        if t.is_empty() || t.starts_with("//") || t.starts_with("#[") || t.starts_with("#![") {
            continue;
        }
        let line_no = (i as u32) + 1;
        return Some(AirSpan::new(file, line_no, line_no));
    }
    None
}

fn parse_hint_body(body: &str) -> HintKind {
    let mut tokens = body.split_whitespace();
    let Some(head) = tokens.next() else {
        return HintKind::Unknown;
    };

    match head {
        // Generic Locus annotations.
        "allow" => {
            let rule = tokens.next().unwrap_or("").to_string();
            let reason = parse_kv(body, "reason");
            let expires = parse_kv(body, "expires");
            HintKind::Allow {
                rule,
                reason,
                expires,
            }
        }
        "fact" => {
            let raw = tokens.next().unwrap_or("");
            let normalised = normalise_fact_kind(raw);
            HintKind::MarksFact {
                fact_kind: normalised,
            }
        }
        // Paradigm-scoped annotations. OT owns canonical/boundary/converter
        // semantics; the top-level `locus:` prefix only owns transport.
        "ot" => {
            let rest = body[head.len()..].trim_start();
            parse_ot_hint(rest)
        }
        _ => HintKind::Unknown,
    }
}

fn parse_ot_hint(body: &str) -> HintKind {
    let mut tokens = body.split_whitespace();
    let Some(head) = tokens.next() else {
        return HintKind::Unknown;
    };

    match head {
        "canonical" => HintKind::Canonical,
        "boundary" => {
            let concept = tokens.next().map(str::to_string);
            let boundary = tokens.next().map(str::to_string);
            HintKind::Boundary { concept, boundary }
        }
        "converter" => HintKind::Converter,
        "protocol-translation" => {
            let reason = parse_kv(body, "reason");
            HintKind::ProtocolTranslation { reason }
        }
        "generated-boundary" => HintKind::GeneratedBoundary,
        _ => HintKind::Unknown,
    }
}

/// Canonicalise the user-typed `fact_kind` token to its snake_case
/// spec name. Accepts kebab-case, PascalCase, and snake_case; unknown
/// inputs pass through unchanged so the loader can log them as
/// "unknown marker".
fn normalise_fact_kind(s: &str) -> String {
    // Lowercase + drop hyphens (kebab-case → snake_case). For
    // PascalCase like "HotPath", insert underscores before uppercase
    // letters. Then lowercase.
    let mut out = String::with_capacity(s.len() + 4);
    for (i, ch) in s.chars().enumerate() {
        if ch == '-' {
            out.push('_');
        } else if ch.is_ascii_uppercase() && i > 0 && !out.ends_with('_') {
            out.push('_');
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch.to_ascii_lowercase());
        }
    }
    out
}

/// Pull `key="value"` out of a free-form hint body.
fn parse_kv(body: &str, key: &str) -> Option<String> {
    let needle = format!("{key}=\"");
    let start = body.find(&needle)? + needle.len();
    let rest = &body[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;

    #[test]
    fn canonical_hint_binds_to_next_item() {
        let src = indoc! {r#"
            // locus: ot canonical
            pub struct User {
                pub id: String,
            }
        "#};
        let hints = scan_hints(src, "test.rs");
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].kind, HintKind::Canonical);
        assert_eq!(hints[0].span.line_start, 1);
        let target = hints[0].target_span.as_ref().unwrap();
        assert_eq!(
            target.line_start, 2,
            "should bind to `pub struct User` line"
        );
    }

    #[test]
    fn boundary_hint_parses_concept_and_boundary() {
        let src = "// locus: ot boundary identity.user api.v1\nstruct UserDto;\n";
        let hints = scan_hints(src, "t.rs");
        match &hints[0].kind {
            HintKind::Boundary { concept, boundary } => {
                assert_eq!(concept.as_deref(), Some("identity.user"));
                assert_eq!(boundary.as_deref(), Some("api.v1"));
            }
            other => panic!("expected Boundary, got {other:?}"),
        }
    }

    #[test]
    fn allow_hint_extracts_rule_reason_expires() {
        let src = r#"// locus: allow FL003 reason="legacy import" expires="2026-07-01"
fn x() {}
"#;
        let hints = scan_hints(src, "t.rs");
        match &hints[0].kind {
            HintKind::Allow {
                rule,
                reason,
                expires,
            } => {
                assert_eq!(rule, "FL003");
                assert_eq!(reason.as_deref(), Some("legacy import"));
                assert_eq!(expires.as_deref(), Some("2026-07-01"));
            }
            other => panic!("expected Allow, got {other:?}"),
        }
    }

    #[test]
    fn no_hint_in_plain_source() {
        let hints = scan_hints("fn main() {}\n", "t.rs");
        assert!(hints.is_empty());
    }

    #[test]
    fn unrecognized_hint_keyword_falls_back_to_unknown() {
        let hints = scan_hints("// locus: not-a-real-kind\nfn x() {}\n", "t.rs");
        assert_eq!(hints[0].kind, HintKind::Unknown);
    }

    #[test]
    fn legacy_ot_prefix_is_not_supported() {
        let hints = scan_hints("// ot: canonical\nstruct User;\n", "t.rs");
        assert!(
            hints.is_empty(),
            "legacy `// ot:` annotations should not be accepted before public release"
        );
    }

    #[test]
    fn hint_inside_raw_string_is_ignored() {
        // Simulate the dogfood case: scanning a Rust file that contains
        // `// locus:` text inside an `indoc! {r#"..."#}` block.
        let src = "let s = r#\"\n// locus: ot canonical\nstruct Fake;\n\"#;\nstruct Real;\n";
        let hints = scan_hints(src, "t.rs");
        assert!(
            hints.is_empty(),
            "hint inside raw-string literal must not be picked up; got {hints:?}"
        );
    }

    #[test]
    fn fact_hint_records_normalised_fact_kind() {
        let cases = [
            ("// locus: fact hot_path\nfn x() {}\n", "hot_path"),
            ("// locus: fact hot-path\nfn x() {}\n", "hot_path"),
            ("// locus: fact HotPath\nfn x() {}\n", "hot_path"),
            (
                "// locus: fact request_context\nfn x() {}\n",
                "request_context",
            ),
            (
                "// locus: fact BoundaryEntry\nfn x() {}\n",
                "boundary_entry",
            ),
            (
                "// locus: fact BackgroundWorker\nfn x() {}\n",
                "background_worker",
            ),
        ];
        for (src, expected) in cases {
            let hints = scan_hints(src, "t.rs");
            assert_eq!(hints.len(), 1, "src `{src}` produced {hints:?}");
            match &hints[0].kind {
                HintKind::MarksFact { fact_kind } => {
                    assert_eq!(fact_kind, expected, "src `{src}`");
                }
                other => panic!("expected MarksFact for `{src}`, got {other:?}"),
            }
        }
    }

    #[test]
    fn fact_hint_with_unknown_fact_kind_degrades_to_lowercased_text() {
        // Unknown / future fact kinds round-trip as lowercase snake_case
        // so the loader can log them rather than the scanner silently
        // dropping them.
        let hints = scan_hints("// locus: fact PolicyDecision\nfn x() {}\n", "t.rs");
        match &hints[0].kind {
            HintKind::MarksFact { fact_kind } => {
                assert_eq!(fact_kind, "policy_decision");
            }
            other => panic!("expected MarksFact, got {other:?}"),
        }
    }

    #[test]
    fn hint_above_derive_binds_to_struct_not_attr() {
        let src = indoc! {r#"
            // locus: ot canonical
            #[derive(Debug, Clone)]
            pub struct User {
                pub id: String,
            }
        "#};
        let hints = scan_hints(src, "t.rs");
        assert_eq!(hints.len(), 1);
        let target = hints[0].target_span.as_ref().unwrap();
        assert_eq!(
            target.line_start, 3,
            "should skip the `#[derive(...)]` line and bind to `pub struct User`"
        );
    }
}
