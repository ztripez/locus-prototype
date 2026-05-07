//! OT rules.
//!
//! Implemented:
//! - [`ot001`]: duplicate canonical for a single concept
//! - [`ot002`]: undeclared concept-shaped type (warning by default)
//! - [`ot006`]: unregistered conversion between accepted endpoints
//!
//! Future: OT003 (boundary leak), OT004 (direct canonical construction),
//! OT005 (missing converter), OT007 (adapter-to-adapter), OT008–OT012.

use std::collections::BTreeSet;

use locus_air::{AirItem, AirWorkspace};

use super::infer::{ConceptCluster, FIELD_OVERLAP_THRESHOLD, InferredRole};
use super::lockfile_schema::OtSection;
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

/// OT002 — undeclared concept-shaped type.
///
/// Fires when a cluster contains:
/// - at least one Canonical member (annotated `// ot: canonical`), and
/// - one or more Unknown members whose field overlap with the canonical
///   meets [`FIELD_OVERLAP_THRESHOLD`].
///
/// The Unknown members get a Warning by default; under `--agent-strict` they
/// are elevated to Fatal so agent-introduced shadow types can't sneak in.
pub fn ot002(clusters: &[ConceptCluster], mode: CheckMode) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for cluster in clusters {
        let canonical = cluster
            .members
            .iter()
            .find(|m| m.role == InferredRole::Canonical);
        let Some(canonical) = canonical else {
            continue; // no anchor → can't tell which is the shadow
        };

        for member in &cluster.members {
            if member.role != InferredRole::Unknown {
                continue;
            }
            if member.field_overlap < FIELD_OVERLAP_THRESHOLD {
                continue;
            }

            let mut why = vec![
                format!(
                    "overlaps {:.0}% with `{}` (canonical for `{}`)",
                    member.field_overlap * 100.0,
                    canonical.name,
                    cluster.concept_id
                ),
                format!("name shares stem `{}`", cluster.stem),
            ];
            why.extend(member.reasons.iter().cloned());

            let suggested_fix = format!(
                "annotate as boundary: `// ot: boundary {} <boundary-name>` above `{}`, \
                 or remove and use `{}` directly",
                cluster.concept_id, member.name, canonical.symbol
            );

            out.push(Diagnostic {
                rule_id: "OT002".to_string(),
                severity: mode.elevate(Severity::Warning),
                span: member.span.clone(),
                concept: Some(cluster.concept_id.clone()),
                message: format!(
                    "`{}` is concept-shaped but not accepted as canonical or boundary",
                    member.symbol
                ),
                why,
                suggested_fix: Some(suggested_fix),
            });
        }
    }
    out
}

/// OT001 — duplicate canonical concept.
///
/// Fires when two or more cluster members are tagged Canonical for the same
/// concept. Two ways this happens:
/// - multiple `// ot: canonical` annotations across types in the same stem
///   bucket;
/// - a hint and a lockfile acceptance disagreeing — the lockfile wins for the
///   role lookup, but the *other* annotated type still presents as Canonical
///   via its hint, producing a duplicate within the cluster.
///
/// Always Fatal: a concept can only have one canonical representation. There
/// is no "warning" path here — it's a structural contradiction.
pub fn ot001(clusters: &[ConceptCluster], _mode: CheckMode) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for cluster in clusters {
        let canonicals: Vec<_> = cluster
            .members
            .iter()
            .filter(|m| m.role == InferredRole::Canonical)
            .collect();
        if canonicals.len() < 2 {
            continue;
        }

        // Diagnostic per *extra* canonical — pin the first as the "incumbent"
        // and report each additional one. This makes the fixes obvious: drop
        // the redundant `// ot: canonical` annotation or rename the type.
        let primary = canonicals[0];
        for extra in &canonicals[1..] {
            out.push(Diagnostic {
                rule_id: "OT001".to_string(),
                severity: Severity::Fatal,
                span: extra.span.clone(),
                concept: Some(cluster.concept_id.clone()),
                message: format!(
                    "`{}` is a second canonical for concept `{}`; \
                     `{}` is already canonical",
                    extra.symbol, cluster.concept_id, primary.symbol
                ),
                why: vec![
                    format!(
                        "both members carry Canonical role for stem `{}`",
                        cluster.stem
                    ),
                    format!("incumbent canonical: `{}`", primary.symbol),
                ],
                suggested_fix: Some(format!(
                    "drop the `// ot: canonical` annotation on `{}` and either \
                     re-annotate it as `// ot: boundary {} <name>` or rename the type",
                    extra.name, cluster.concept_id
                )),
            });
        }
    }
    out
}

/// OT006 — unregistered conversion between accepted endpoints.
///
/// Fires when an `AirConversion`'s endpoints are both lockfile-accepted
/// (canonical or boundary) but the conversion symbol itself isn't recorded
/// under that concept's `converters`. This is the "agent added a new mapper"
/// case after `locus init` has been run: the lockfile encodes which
/// conversions are sanctioned; anything else is a candidate fork.
///
/// Severity: Warning by default; Fatal under `--agent-strict`.
pub fn ot006(air: &AirWorkspace, section: &OtSection, mode: CheckMode) -> Vec<Diagnostic> {
    // Build a per-concept (accepted-symbol, accepted-converter-symbol) map
    // upfront so the per-conversion lookup is cheap.
    let mut concept_for_symbol: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    let mut accepted_converter_symbols: std::collections::BTreeMap<String, BTreeSet<String>> =
        std::collections::BTreeMap::new();
    for (concept_id, entry) in &section.concepts {
        concept_for_symbol.insert(entry.canonical.symbol.clone(), concept_id.clone());
        for b in &entry.boundaries {
            concept_for_symbol.insert(b.symbol.clone(), concept_id.clone());
        }
        let set: BTreeSet<String> = entry.converters.iter().map(|c| c.symbol.clone()).collect();
        accepted_converter_symbols.insert(concept_id.clone(), set);
    }

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                let AirItem::Conversion(c) = item else {
                    continue;
                };
                let Some(from_concept) = lookup_concept(&concept_for_symbol, &c.from) else {
                    continue;
                };
                let Some(to_concept) = lookup_concept(&concept_for_symbol, &c.to) else {
                    continue;
                };
                if from_concept != to_concept {
                    // Adapter-to-adapter or cross-concept — that's OT007 territory,
                    // not OT006. OT006 only flags missing acceptance within one concept.
                    continue;
                }
                let accepted = accepted_converter_symbols
                    .get(from_concept)
                    .is_some_and(|set| set.contains(&c.symbol));
                if accepted {
                    continue;
                }
                out.push(Diagnostic {
                    rule_id: "OT006".to_string(),
                    severity: mode.elevate(Severity::Warning),
                    span: c.span.clone(),
                    concept: Some(from_concept.clone()),
                    message: format!(
                        "`{}` converts between accepted symbols of concept `{}` \
                         but is not recorded as an accepted converter",
                        c.symbol, from_concept
                    ),
                    why: vec![
                        format!("from `{}` (accepted)", c.from),
                        format!("to `{}` (accepted)", c.to),
                        format!("conversion symbol `{}` not in lockfile", c.symbol),
                    ],
                    suggested_fix: Some(
                        "rerun `locus init` to refresh the lockfile, or add the \
                         converter symbol manually under the concept's `converters` list"
                            .to_string(),
                    ),
                });
            }
        }
    }
    out
}

/// Resolve a conversion endpoint string against the concept_for_symbol map.
/// Endpoints in `AirConversion` are type-text like `User` or
/// `crate::dto::UserDto`; lockfile symbols are fully qualified. Match by
/// suffix on `::` segments, same logic as the `init` flow.
fn lookup_concept<'a>(
    concept_for_symbol: &'a std::collections::BTreeMap<String, String>,
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

#[cfg(test)]
mod tests {
    use super::super::infer::{ClusterMember, ConceptCluster, InferredRole};
    use super::*;
    use locus_air::AirSpan;

    fn member(
        name: &str,
        symbol: &str,
        role: InferredRole,
        overlap: f32,
        reasons: Vec<String>,
    ) -> ClusterMember {
        ClusterMember {
            symbol: symbol.into(),
            name: name.into(),
            role,
            span: AirSpan::new("t.rs", 1, 1),
            file_path: "t.rs".into(),
            field_overlap: overlap,
            fields: vec!["id".into(), "email".into()],
            reasons,
        }
    }

    #[test]
    fn fires_on_unknown_with_canonical_present() {
        let cluster = ConceptCluster {
            concept_id: "user".into(),
            stem: "User".into(),
            members: vec![
                member("User", "crate::User", InferredRole::Canonical, 1.0, vec![]),
                member(
                    "UserModel",
                    "crate::dto::UserModel",
                    InferredRole::Unknown,
                    1.0,
                    vec!["name suffix `Model`".into()],
                ),
            ],
        };
        let diags = ot002(&[cluster], CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "OT002");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert_eq!(diags[0].concept.as_deref(), Some("user"));
    }

    #[test]
    fn does_not_fire_when_no_canonical() {
        let cluster = ConceptCluster {
            concept_id: "user".into(),
            stem: "User".into(),
            members: vec![
                member(
                    "UserDto",
                    "crate::UserDto",
                    InferredRole::Boundary,
                    1.0,
                    vec![],
                ),
                member(
                    "UserModel",
                    "crate::UserModel",
                    InferredRole::Unknown,
                    1.0,
                    vec![],
                ),
            ],
        };
        let diags = ot002(&[cluster], CheckMode::Human);
        assert!(diags.is_empty(), "no canonical anchor → no OT002");
    }

    #[test]
    fn does_not_fire_on_accepted_boundary() {
        let cluster = ConceptCluster {
            concept_id: "user".into(),
            stem: "User".into(),
            members: vec![
                member("User", "crate::User", InferredRole::Canonical, 1.0, vec![]),
                member(
                    "UserDto",
                    "crate::UserDto",
                    InferredRole::Boundary,
                    1.0,
                    vec![],
                ),
            ],
        };
        assert!(ot002(&[cluster], CheckMode::Human).is_empty());
    }

    #[test]
    fn agent_strict_elevates_to_fatal() {
        let cluster = ConceptCluster {
            concept_id: "user".into(),
            stem: "User".into(),
            members: vec![
                member("User", "crate::User", InferredRole::Canonical, 1.0, vec![]),
                member(
                    "UserModel",
                    "crate::UserModel",
                    InferredRole::Unknown,
                    1.0,
                    vec![],
                ),
            ],
        };
        let diags = ot002(&[cluster], CheckMode::AgentStrict);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    #[test]
    fn weak_overlap_below_threshold_is_dropped() {
        let cluster = ConceptCluster {
            concept_id: "user".into(),
            stem: "User".into(),
            members: vec![
                member("User", "crate::User", InferredRole::Canonical, 1.0, vec![]),
                member(
                    "UserModel",
                    "crate::UserModel",
                    InferredRole::Unknown,
                    0.2,
                    vec![],
                ),
            ],
        };
        assert!(ot002(&[cluster], CheckMode::Human).is_empty());
    }

    // ---- OT001 ----

    #[test]
    fn ot001_fires_on_two_canonicals_in_one_cluster() {
        let cluster = ConceptCluster {
            concept_id: "user".into(),
            stem: "User".into(),
            members: vec![
                member(
                    "User",
                    "crate::a::User",
                    InferredRole::Canonical,
                    1.0,
                    vec![],
                ),
                member(
                    "User",
                    "crate::b::User",
                    InferredRole::Canonical,
                    1.0,
                    vec![],
                ),
            ],
        };
        let diags = ot001(&[cluster], CheckMode::Human);
        assert_eq!(diags.len(), 1, "one extra canonical → one diagnostic");
        assert_eq!(diags[0].rule_id, "OT001");
        assert_eq!(diags[0].severity, Severity::Fatal);
        assert!(
            diags[0].message.contains("crate::b::User"),
            "should flag the second canonical; got {}",
            diags[0].message
        );
        assert!(
            diags[0].message.contains("crate::a::User"),
            "should reference the incumbent; got {}",
            diags[0].message
        );
    }

    #[test]
    fn ot001_emits_one_diag_per_extra_canonical() {
        let cluster = ConceptCluster {
            concept_id: "user".into(),
            stem: "User".into(),
            members: vec![
                member("U1", "crate::U1", InferredRole::Canonical, 1.0, vec![]),
                member("U2", "crate::U2", InferredRole::Canonical, 1.0, vec![]),
                member("U3", "crate::U3", InferredRole::Canonical, 1.0, vec![]),
            ],
        };
        let diags = ot001(&[cluster], CheckMode::Human);
        assert_eq!(
            diags.len(),
            2,
            "three canonicals → two duplicate diagnostics"
        );
    }

    #[test]
    fn ot001_silent_on_single_canonical() {
        let cluster = ConceptCluster {
            concept_id: "user".into(),
            stem: "User".into(),
            members: vec![
                member("User", "crate::User", InferredRole::Canonical, 1.0, vec![]),
                member(
                    "UserDto",
                    "crate::UserDto",
                    InferredRole::Boundary,
                    1.0,
                    vec![],
                ),
            ],
        };
        assert!(ot001(&[cluster], CheckMode::Human).is_empty());
    }

    // ---- OT006 ----

    use locus_air::{
        AIR_SCHEMA_VERSION, AirConversion, AirFile, AirPackage, AirWorkspace, ConversionMechanism,
    };
    use std::collections::BTreeMap;

    fn air_with_conversion(symbol: &str, from: &str, to: &str) -> AirWorkspace {
        AirWorkspace {
            schema_version: AIR_SCHEMA_VERSION,
            packages: vec![AirPackage {
                name: "x".into(),
                version: "0".into(),
                root_dir: "/".into(),
                files: vec![AirFile {
                    path: "t.rs".into(),
                    module_path: Some("crate".into()),
                    items: vec![AirItem::Conversion(AirConversion {
                        from: from.into(),
                        to: to.into(),
                        mechanism: ConversionMechanism::TryFrom,
                        symbol: symbol.into(),
                        span: AirSpan::new("t.rs", 1, 1),
                    })],
                    hints: Vec::new(),
                    parse_error: None,
                }],
            }],
        }
    }

    fn ot_section_with_user_concept(extra_converters: &[&str]) -> OtSection {
        use super::super::lockfile_schema::{
            AcceptedBoundary, AcceptedCanonical, AcceptedConverter, ConceptEntry, Source,
        };
        let mut concepts = BTreeMap::new();
        concepts.insert(
            "user".to_string(),
            ConceptEntry {
                canonical: AcceptedCanonical {
                    symbol: "crate::identity::User".into(),
                    source: Source::Hint,
                },
                boundaries: vec![AcceptedBoundary {
                    symbol: "crate::dto::UserDto".into(),
                    boundary: Some("api.v1".into()),
                    source: Source::Hint,
                }],
                converters: extra_converters
                    .iter()
                    .map(|sym| AcceptedConverter {
                        from: "UserDto".into(),
                        to: "User".into(),
                        symbol: (*sym).to_string(),
                        source: Source::Init,
                    })
                    .collect(),
            },
        );
        OtSection { concepts }
    }

    #[test]
    fn ot006_fires_on_unaccepted_conversion_between_accepted_endpoints() {
        let air = air_with_conversion("crate::dto::sneaky_map", "UserDto", "User");
        let section = ot_section_with_user_concept(&[]);
        let diags = ot006(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "OT006");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(diags[0].message.contains("crate::dto::sneaky_map"));
    }

    #[test]
    fn ot006_quiet_on_accepted_conversion() {
        let air = air_with_conversion(
            "crate::dto::impl TryFrom<UserDto> for User",
            "UserDto",
            "User",
        );
        let section = ot_section_with_user_concept(&["crate::dto::impl TryFrom<UserDto> for User"]);
        assert!(ot006(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ot006_quiet_when_endpoint_not_accepted() {
        // `Random` isn't in the lockfile → OT006 doesn't fire (this isn't its job)
        let air = air_with_conversion("crate::dto::weird", "UserDto", "Random");
        let section = ot_section_with_user_concept(&[]);
        assert!(ot006(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ot006_quiet_on_cross_concept_conversion() {
        // If endpoints belong to different accepted concepts, this is OT007
        // territory, not OT006.
        use super::super::lockfile_schema::{AcceptedCanonical, ConceptEntry, Source};
        let mut section = ot_section_with_user_concept(&[]);
        section.concepts.insert(
            "team".to_string(),
            ConceptEntry {
                canonical: AcceptedCanonical {
                    symbol: "crate::team::Team".into(),
                    source: Source::Hint,
                },
                boundaries: Vec::new(),
                converters: Vec::new(),
            },
        );
        let air = air_with_conversion("crate::cross", "User", "Team");
        assert!(ot006(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ot006_agent_strict_elevates_to_fatal() {
        let air = air_with_conversion("crate::dto::sneaky_map", "UserDto", "User");
        let section = ot_section_with_user_concept(&[]);
        let diags = ot006(&air, &section, CheckMode::AgentStrict);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }
}
