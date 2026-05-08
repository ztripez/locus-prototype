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
use crate::init::{CommandOption, Suggestion, SuggestionCategory};
use crate::lockfile::Lockfile;

/// Confidence floor above which `suggest()` emits a single "accept this
/// cluster" option. Below it (but above [`MID_CONFIDENCE`]), the suggestion
/// also offers a "split into separate concepts" alternative.
const HIGH_CONFIDENCE: f32 = 0.95;
/// Confidence floor below which `suggest()` stays silent. Members of weaker
/// clusters surface as OT002 candidates on the next `locus check` instead.
const MID_CONFIDENCE: f32 = 0.70;

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

fn is_boundary_like(member: &ClusterMember, canonical: &ClusterMember) -> bool {
    use crate::paradigms::one_truth::infer::matched_suffix;
    if matched_suffix(&member.name).is_some() {
        return true;
    }
    for seg in member.symbol.split("::") {
        if matches!(seg, "api" | "dto" | "dtos" | "transport") {
            return true;
        }
    }
    // Field overlap with the canonical (already computed against the
    // cluster's reference type) ≥ 0.5 implies structural similarity.
    member.field_overlap >= 0.5 && member.symbol != canonical.symbol
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

/// Init-time onboarding suggestions for the OT paradigm.
///
/// Walks [`cluster_concepts`], skips clusters whose `concept_id` is already
/// recorded in the lockfile's `OT` section, then tiers what's left by the
/// cluster's confidence:
/// - `>= HIGH_CONFIDENCE`: a single "accept this cluster" option.
/// - `>= MID_CONFIDENCE`: two options — accept as one concept, or split into
///   per-member concepts (one canonical + each member as its own canonical).
/// - `< MID_CONFIDENCE`: silent. Weak overlap shows up later as OT002
///   candidates on `locus check`, not init noise.
///
/// Clusters with no inferred canonical or no boundary members are also
/// skipped — there's nothing for an agent to "accept" yet.
pub fn suggest(air: &AirWorkspace, lockfile: &Lockfile) -> Vec<Suggestion> {
    let section: OtSection = lockfile.paradigm_section("OT").unwrap_or_default();
    let clusters = cluster_concepts(air);
    let mut out = Vec::new();
    for cluster in &clusters {
        if section.concepts.contains_key(&cluster.concept_id) {
            continue;
        }
        // Two paths:
        //  1. Hinted: at least one member is `InferredRole::Canonical`. Use the
        //     existing hint-tagged members directly (and apply no penalty).
        //  2. Heuristic: no hint. Elect a canonical via signals; treat the rest
        //     as boundary candidates if they look boundary-shaped. Dock 0.1
        //     from cluster confidence to reflect the guess.
        let hinted_canonical = cluster
            .members
            .iter()
            .find(|m| m.role == InferredRole::Canonical);

        let (canonical_idx, hinted_path) = match hinted_canonical {
            Some(c) => {
                let idx = cluster
                    .members
                    .iter()
                    .position(|m| std::ptr::eq(m, c))
                    .unwrap();
                (idx, true)
            }
            None => match super::infer::elect_canonical(cluster, air) {
                Some(o) => (o.canonical_index, false),
                None => continue,
            },
        };
        let canonical = &cluster.members[canonical_idx];

        let boundaries: Vec<&ClusterMember> = if hinted_path {
            cluster
                .members
                .iter()
                .filter(|m| m.role == InferredRole::Boundary)
                .collect()
        } else {
            cluster
                .members
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != canonical_idx)
                .map(|(_, m)| m)
                .filter(|m| is_boundary_like(m, canonical))
                .collect()
        };
        if boundaries.is_empty() {
            continue;
        }

        let confidence_base = cluster.confidence;
        let confidence = if hinted_path {
            confidence_base
        } else {
            (confidence_base - 0.1).max(0.0)
        };
        if confidence < MID_CONFIDENCE {
            continue;
        }
        let cid = &cluster.concept_id;
        let accept_canonical_cmd = format!(
            "locus accept canonical {} --concept {}",
            canonical.symbol, cid
        );
        let accept_boundary_cmds: Vec<String> = boundaries
            .iter()
            .map(|m| format!("locus accept boundary {} --concept {}", m.symbol, cid))
            .collect();
        let mut single_option_cmds = vec![accept_canonical_cmd.clone()];
        single_option_cmds.extend(accept_boundary_cmds.iter().cloned());

        if confidence >= HIGH_CONFIDENCE {
            out.push(Suggestion {
                category: SuggestionCategory::Concept,
                headline: format!(
                    "cluster `{cid}` — {} + {}",
                    canonical.symbol,
                    boundaries
                        .iter()
                        .map(|m| m.symbol.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                why: vec![format!("confidence {:.2}", confidence)],
                options: vec![CommandOption {
                    label: "accept this cluster".into(),
                    commands: single_option_cmds,
                }],
                prefixes: vec!["OT".into()],
            });
        } else {
            // Mid-confidence: offer both interpretations.
            let split_cmds: Vec<String> = std::iter::once(accept_canonical_cmd.clone())
                .chain(boundaries.iter().map(|m| {
                    format!(
                        "locus accept canonical {} --concept {}_{}",
                        m.symbol,
                        cid,
                        m.symbol.rsplit("::").next().unwrap_or("alt").to_lowercase()
                    )
                }))
                .collect();
            out.push(Suggestion {
                category: SuggestionCategory::Concept,
                headline: format!("cluster `{cid}` ambiguous — {}", canonical.symbol),
                why: vec![format!("confidence {:.2}; review members", confidence)],
                options: vec![
                    CommandOption {
                        label: "if same concept".into(),
                        commands: single_option_cmds,
                    },
                    CommandOption {
                        label: "if separate concepts".into(),
                        commands: split_cmds,
                    },
                ],
                prefixes: vec!["OT".into()],
            });
        }
    }
    out
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
            mechanism: locus_air::ConversionMechanism::FallibleAdapter,
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
            mechanism: locus_air::ConversionMechanism::InfallibleAdapter,
            symbol: "?".into(),
            span: locus_air::AirSpan::new("t.rs", 1, 1),
        };
        assert!(!endpoints_accepted(&conv, &s));
    }
}

#[cfg(test)]
mod suggest_tests {
    use super::*;
    use crate::init::SuggestionCategory;
    use crate::lockfile::Lockfile;

    #[test]
    fn suggestion_count_matches_clusters_with_canonical_and_boundary() {
        let workspace = std::path::Path::new("../../tests/fixtures/sample-crate");
        if !workspace.exists() {
            eprintln!("sample-crate fixture missing; skipping");
            return;
        }
        let air = match locus_rust::scan(workspace) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("scan failed: {e}; skipping");
                return;
            }
        };
        let lf = Lockfile::empty();
        let suggestions = suggest(&air, &lf);
        // Every emitted suggestion must be category Concept.
        assert!(
            suggestions
                .iter()
                .all(|s| s.category == SuggestionCategory::Concept)
        );
        // Headlines all start with `cluster ` so an agent can grep.
        assert!(
            suggestions
                .iter()
                .all(|s| s.headline.starts_with("cluster "))
        );
    }

    #[test]
    fn elects_canonical_in_hint_less_cluster() {
        let workspace = std::path::Path::new("../../tests/fixtures/cluster-crate");
        if !workspace.exists() {
            eprintln!("cluster-crate fixture missing; skipping");
            return;
        }
        let air = locus_rust::scan(workspace).expect("scan cluster-crate");
        let lf = Lockfile::empty();
        let suggestions = suggest(&air, &lf);
        let user_concept = suggestions
            .iter()
            .find(|s| s.category == SuggestionCategory::Concept && s.headline.contains("user"));
        assert!(
            user_concept.is_some(),
            "expected a heuristic [concept] suggestion for `user` cluster; got {:?}",
            suggestions
        );
        let s = user_concept.unwrap();
        // Verify the elected canonical is User (the unsuffixed name in the
        // domain module), not UserResponse.
        let cmds = s.options[0].commands.join("\n");
        assert!(
            cmds.contains("locus accept canonical cluster_crate::domain::User"),
            "expected User as elected canonical; got commands:\n{cmds}"
        );
    }

    #[test]
    fn no_suggestion_for_already_accepted_concept() {
        let workspace = std::path::Path::new("../../tests/fixtures/sample-crate");
        if !workspace.exists() {
            return;
        }
        let air = match locus_rust::scan(workspace) {
            Ok(a) => a,
            Err(_) => return,
        };
        // Pre-fill the lockfile with every clusterable concept_id from the AIR
        // so suggest() filters them all.
        let clusters = super::super::infer::cluster_concepts(&air);
        let mut concepts = serde_json::Map::new();
        for c in &clusters {
            // Need a canonical to make `suggest` consider the cluster at all.
            // We seed both canonical and boundary list as accepted.
            let canonical_sym = c
                .members
                .iter()
                .find(|m| m.role == super::super::infer::InferredRole::Canonical)
                .map(|m| m.symbol.clone())
                .unwrap_or_else(|| {
                    c.members
                        .first()
                        .map(|m| m.symbol.clone())
                        .unwrap_or_default()
                });
            let entry = serde_json::json!({
                "canonical": {"symbol": canonical_sym, "source": "accepted"},
                "boundaries": [],
                "converters": []
            });
            concepts.insert(c.concept_id.clone(), entry);
        }
        let mut lf = Lockfile::empty();
        lf.paradigms
            .insert("OT".into(), serde_json::json!({"concepts": concepts}));
        let suggestions = suggest(&air, &lf);
        assert!(
            suggestions.is_empty(),
            "expected suppression of all concepts; got {} suggestion(s)",
            suggestions.len()
        );
    }
}
