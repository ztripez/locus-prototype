//! `locus init` for the OT paradigm.
//!
//! Walks the AIR workspace, groups types into clusters via [`super::infer`],
//! then promotes annotated canonicals + boundaries into a fresh [`OtSection`].
//! Converters are pulled from `AirItem::Conversion` whose endpoints both land
//! in the section's accepted symbols.
//!
//! Conservative by design: only annotated members (`// ot: canonical` /
//! `// ot: boundary`) are accepted automatically. Inferred-but-unannotated
//! members stay out, surfacing on the next `locus check` as OT002 candidates.
//! Phase 2.B will add `locus accept` for symbol-by-symbol promotion of
//! candidates that are correct but unannotated.

use std::collections::BTreeSet;

use locus_air::{AirConversion, AirHint, AirItem, AirWorkspace, HintKind};

use super::infer::{ClusterMember, InferredRole, cluster_concepts};
use super::lockfile_schema::{
    AcceptedBoundary, AcceptedCanonical, AcceptedConverter, ConceptEntry, OtSection, Source,
};

pub fn build_ot_section(air: &AirWorkspace) -> OtSection {
    let clusters = cluster_concepts(air);
    let mut section = OtSection::default();

    for cluster in &clusters {
        let canonical = cluster
            .members
            .iter()
            .find(|m| m.role == InferredRole::Canonical);
        let Some(canonical) = canonical else {
            continue; // no anchor → don't fabricate one
        };

        let boundaries: Vec<_> = cluster
            .members
            .iter()
            .filter(|m| m.role == InferredRole::Boundary)
            .map(|m| AcceptedBoundary {
                symbol: m.symbol.clone(),
                boundary: boundary_label(air, m),
                source: Source::Hint,
            })
            .collect();

        let accepted_symbols: BTreeSet<&str> = std::iter::once(canonical.symbol.as_str())
            .chain(boundaries.iter().map(|b| b.symbol.as_str()))
            .collect();
        let converters = collect_converters(air, &accepted_symbols);

        section.concepts.insert(
            cluster.concept_id.clone(),
            ConceptEntry {
                canonical: AcceptedCanonical {
                    symbol: canonical.symbol.clone(),
                    source: Source::Hint,
                },
                boundaries,
                converters,
            },
        );
    }
    section
}

/// Pull the `boundary` token from the type's `// ot: boundary` hint, if any.
fn boundary_label(air: &AirWorkspace, member: &ClusterMember) -> Option<String> {
    for pkg in &air.packages {
        for file in &pkg.files {
            if file.path != member.file_path {
                continue;
            }
            for hint in hints_in_span(file.hints.iter(), member) {
                if let HintKind::Boundary { boundary, .. } = &hint.kind {
                    return boundary.clone();
                }
            }
        }
    }
    None
}

fn hints_in_span<'a, I>(hints: I, member: &ClusterMember) -> Vec<&'a AirHint>
where
    I: Iterator<Item = &'a AirHint>,
{
    hints
        .filter(|h| {
            h.target_span.as_ref().is_some_and(|t| {
                t.line_start >= member.span.line_start && t.line_start <= member.span.line_end
            })
        })
        .collect()
}

fn collect_converters(
    air: &AirWorkspace,
    accepted_symbols: &BTreeSet<&str>,
) -> Vec<AcceptedConverter> {
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                let AirItem::Conversion(c) = item else {
                    continue;
                };
                if endpoints_accepted(c, accepted_symbols) {
                    out.push(AcceptedConverter {
                        from: c.from.clone(),
                        to: c.to.clone(),
                        symbol: c.symbol.clone(),
                        source: Source::Init,
                    });
                }
            }
        }
    }
    out
}

fn endpoints_accepted(c: &AirConversion, accepted: &BTreeSet<&str>) -> bool {
    // Conversion endpoints arrive as type-text strings. Match them against
    // the suffix of accepted symbols so `User` lines up with
    // `crate::identity::User`, `UserDto` with `crate::dto::UserDto`, etc.
    accepted_matches(&c.from, accepted) && accepted_matches(&c.to, accepted)
}

fn accepted_matches(needle: &str, accepted: &BTreeSet<&str>) -> bool {
    let trimmed = needle.trim();
    accepted.iter().any(|sym| {
        let tail = sym.rsplit("::").next().unwrap_or(sym);
        tail == trimmed || *sym == trimmed
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_accepted_matches_short_and_full_paths() {
        let mut s = BTreeSet::new();
        s.insert("crate::identity::User");
        s.insert("crate::dto::UserDto");
        let conv = AirConversion {
            from: "UserDto".into(),
            to: "User".into(),
            mechanism: locus_air::ConversionMechanism::TryFrom,
            symbol: "impl TryFrom<UserDto> for User".into(),
            span: locus_air::AirSpan::new("t.rs", 1, 1),
        };
        assert!(endpoints_accepted(&conv, &s));
    }

    #[test]
    fn endpoints_rejected_when_neither_side_accepted() {
        let mut s = BTreeSet::new();
        s.insert("crate::identity::User");
        let conv = AirConversion {
            from: "Foo".into(),
            to: "Bar".into(),
            mechanism: locus_air::ConversionMechanism::From,
            symbol: "?".into(),
            span: locus_air::AirSpan::new("t.rs", 1, 1),
        };
        assert!(!endpoints_accepted(&conv, &s));
    }
}
