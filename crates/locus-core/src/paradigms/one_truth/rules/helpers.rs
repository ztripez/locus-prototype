//! Shared helpers for OT rule implementations.
//!
//! These are internal utilities used by two or more rule modules.

use std::collections::BTreeMap;

use locus_air::{AirConversion, AirItem, AirSpan, AirWorkspace, FactProvenance};

/// Deduplicate conversions that point at the same impl block, keeping
/// the highest-rank [`FactProvenance`].
///
/// Two records are considered "the same impl block" when they share
/// `(file, line_start, line_end, mechanism, from, to)`. The endpoint
/// types are part of the key so that valid Rust like
/// `impl From<A> for B {} impl From<C> for D {}` on one line is NOT
/// wrongly collapsed (Codex P1 on #115). This means a semantic backend
/// that wants its records to overlay on top of the syntactic adapter's
/// must emit matching `from` / `to` strings for the same impl — see
/// `docs/superpowers/specs/2026-05-13-rustc-semantic-spike.md` for the
/// phase-2 design discussion on canonical-path normalisation.
///
/// `None` provenance is treated as `Heuristic` for ranking — that's the
/// default backwards-compatible interpretation for v13 wire data.
pub(crate) fn prefer_higher_provenance<'a>(
    items: impl IntoIterator<Item = &'a AirItem>,
) -> Vec<&'a AirConversion> {
    let mut best: BTreeMap<ConvKey, &AirConversion> = BTreeMap::new();
    for item in items {
        let AirItem::Conversion(c) = item else {
            continue;
        };
        let key = ConvKey {
            file: c.span.file.clone(),
            line_start: c.span.line_start,
            line_end: c.span.line_end,
            mechanism: format!("{:?}", c.mechanism),
            from: c.from.clone(),
            to: c.to.clone(),
        };
        let cur_rank = effective_rank(c.provenance.as_ref());
        let keep = match best.get(&key) {
            Some(existing) => cur_rank > effective_rank(existing.provenance.as_ref()),
            None => true,
        };
        if keep {
            best.insert(key, c);
        }
    }
    best.into_values().collect()
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ConvKey {
    file: String,
    line_start: u32,
    line_end: u32,
    mechanism: String,
    from: String,
    to: String,
}

fn effective_rank(p: Option<&FactProvenance>) -> u8 {
    p.map(FactProvenance::rank)
        .unwrap_or_else(|| FactProvenance::Heuristic.rank())
}

/// Resolve a conversion endpoint string against the concept_for_symbol map.
/// Endpoints in `AirConversion` are type-text like `User` or
/// `crate::dto::UserDto`; lockfile symbols are fully qualified. Match by
/// suffix on `::` segments, same logic as the `init` flow.
pub(super) fn lookup_concept<'a>(
    concept_for_symbol: &'a BTreeMap<String, String>,
    needle: &str,
) -> Option<&'a String> {
    let trimmed = needle.trim();
    for (sym, concept) in concept_for_symbol {
        if sym == trimmed {
            return Some(concept);
        }
        if sym.rsplit("::").next() == Some(trimmed) {
            return Some(concept);
        }
    }
    None
}

/// Look up the file path of the AIR type whose `symbol` matches `target`.
pub(super) fn file_of_symbol(air: &AirWorkspace, target: &str) -> Option<String> {
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                if let AirItem::Type(ty) = item
                    && ty.symbol == target
                {
                    return Some(file.path.clone());
                }
            }
        }
    }
    None
}

/// Look up the span of the AIR type whose `symbol` matches `target`.
pub(super) fn span_of_symbol(air: &AirWorkspace, target: &str) -> Option<AirSpan> {
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                if let AirItem::Type(ty) = item
                    && ty.symbol == target
                {
                    return Some(ty.span.clone());
                }
            }
        }
    }
    None
}

/// Last `::`-segment of a path-like identifier (`crate::dto::UserDto` →
/// `UserDto`). Trims whitespace from the result so it can match against
/// `AirConversion` endpoints, which sometimes carry leading `& ` from refs.
pub(super) fn short_name(path: &str) -> &str {
    path.rsplit("::").next().unwrap_or(path).trim()
}

/// Whole-identifier match: returns true if `name` appears in `text` not as a
/// substring of a longer identifier. `Result<UserDto, …>` references `UserDto`
/// but `UserDtoVec` does not.
pub(crate) fn type_text_references(text: &str, name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let bytes = text.as_bytes();
    let needle = name.as_bytes();
    let mut i = 0;
    while i + needle.len() <= bytes.len() {
        if &bytes[i..i + needle.len()] == needle {
            let before_ok = i == 0 || !is_ident_byte(bytes[i - 1]);
            let after_ok =
                i + needle.len() == bytes.len() || !is_ident_byte(bytes[i + needle.len()]);
            if before_ok && after_ok {
                return true;
            }
        }
        i += 1;
    }
    false
}

pub(super) fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// `user_id` → `UserId`; `email` → `Email`. Returns `None` if the input
/// is empty or has consecutive underscores producing empty segments —
/// either way we don't have a clean mapping to PascalCase.
pub(crate) fn snake_to_pascal(snake: &str) -> Option<String> {
    if snake.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(snake.len());
    for seg in snake.split('_') {
        if seg.is_empty() {
            return None;
        }
        let mut chars = seg.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    Some(out)
}

/// True for type-text strings the OT module considers primitive substitutes
/// for value objects. References (`&str`, `&String`) and `Option<…>` of a
/// primitive count too — the field is still primitive-typed downstream.
pub(crate) fn is_primitive_type_text(text: &str) -> bool {
    let t = text.trim().trim_start_matches('&').trim();
    const PRIMS: &[&str] = &[
        "String", "str", "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64",
        "u128", "usize", "f32", "f64", "bool", "char",
    ];
    if PRIMS.contains(&t) {
        return true;
    }
    if let Some(inner) = t.strip_prefix("Option<").and_then(|s| s.strip_suffix('>')) {
        return is_primitive_type_text(inner);
    }
    false
}

/// Match a symbol path against an OT pattern. Supports the same shapes as
/// the DG matcher (`crates/locus-core/src/paradigms/dependency_graph/lockfile_schema.rs::matches_pattern`):
///
/// - `*` matches any path.
/// - `prefix::*` matches `prefix` and any descendant (`prefix::a`, `prefix::a::b`).
/// - `*::suffix` matches any path ending in `::suffix`, segment-aligned.
/// - `*::middle::*` matches any path with `middle` as a segment anywhere
///   (e.g., `*::tests::*` covers inline `mod tests {}` blocks at any depth).
/// - Otherwise an exact-string match.
///
/// Used by OT004's `converter_paths` authority. The leading- and
/// segment-anywhere wildcards are how `*::tests::*` covers test code that
/// legitimately constructs canonicals across crates without forcing the
/// user to enumerate every test module.
pub(super) fn matches_symbol_pattern(value: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    let leading_wild = pattern.starts_with("*::");
    let trailing_wild = pattern.ends_with("::*");
    let stripped = match (leading_wild, trailing_wild) {
        (true, true) => &pattern[3..pattern.len() - 3],
        (true, false) => &pattern[3..],
        (false, true) => &pattern[..pattern.len() - 3],
        (false, false) => pattern,
    };
    if stripped.is_empty() {
        // `*::` or `::*` alone with no body is malformed. Don't quietly
        // match every path — the user wanting that should write `*`.
        return false;
    }
    match (leading_wild, trailing_wild) {
        (true, true) => {
            let mid = format!("::{stripped}::");
            let starts = format!("{stripped}::");
            let ends = format!("::{stripped}");
            value == stripped
                || value.contains(&mid)
                || value.starts_with(&starts)
                || value.ends_with(&ends)
        }
        (true, false) => value == stripped || value.ends_with(&format!("::{stripped}")),
        (false, true) => value == stripped || value.starts_with(&format!("{stripped}::")),
        (false, false) => pattern == value,
    }
}

#[cfg(test)]
mod prefer_higher_provenance_tests {
    use super::*;
    use locus_air::{AirConversion, AirItem, AirSpan, ConversionMechanism};

    fn conv(file: &str, line: u32, from: &str, to: &str, p: Option<FactProvenance>) -> AirItem {
        AirItem::Conversion(AirConversion {
            from: from.into(),
            to: to.into(),
            mechanism: ConversionMechanism::InfallibleAdapter,
            symbol: format!("{from}::to_{to}"),
            span: AirSpan::new(file, line, line),
            provenance: p,
        })
    }

    #[test]
    fn semantic_resolved_wins_over_heuristic_on_same_impl() {
        let items = vec![
            conv("t.rs", 5, "Foo", "Bar", Some(FactProvenance::Heuristic)),
            conv(
                "t.rs",
                5,
                "Foo",
                "Bar",
                Some(FactProvenance::SemanticResolved {
                    backend: locus_air::SemanticBackend::RustAnalyzer,
                }),
            ),
        ];
        let kept = prefer_higher_provenance(&items);
        assert_eq!(kept.len(), 1, "same-impl records must dedupe; got {kept:?}");
        assert!(
            matches!(
                kept[0].provenance,
                Some(FactProvenance::SemanticResolved { .. })
            ),
            "semantic-resolved should win; got {:?}",
            kept[0].provenance,
        );
    }

    #[test]
    fn two_distinct_impls_on_same_line_are_not_collapsed() {
        // Regression for the Codex P1 on #115: `impl From<A> for B {}
        // impl From<C> for D {}` on one line emits two AirConversion
        // records with the same `(file, line, mechanism)`. Including
        // the endpoint types in the dedup key keeps them separate.
        let items = vec![
            conv("t.rs", 5, "A", "B", Some(FactProvenance::Heuristic)),
            conv("t.rs", 5, "C", "D", Some(FactProvenance::Heuristic)),
        ];
        let kept = prefer_higher_provenance(&items);
        assert_eq!(
            kept.len(),
            2,
            "distinct conversions on the same line must NOT be collapsed; got {kept:?}",
        );
    }

    #[test]
    fn none_provenance_is_treated_as_heuristic_for_ranking() {
        // Either record may win — both are rank 0 — but only one
        // survives. The "v13 wire data deserialises as None" contract
        // is what this pins.
        let items = vec![
            conv("t.rs", 5, "Foo", "Bar", None),
            conv("t.rs", 5, "Foo", "Bar", Some(FactProvenance::Heuristic)),
        ];
        let kept = prefer_higher_provenance(&items);
        assert_eq!(kept.len(), 1);
    }
}
