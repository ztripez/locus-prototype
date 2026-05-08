//! OT inference: cluster types by name stem + field-name overlap, assign
//! each member an inferred role (canonical / accepted-boundary / unknown).
//!
//! This is deliberately conservative for Phase 2:
//! - Only `// ot:` hints grant *accepted* canonical / boundary status.
//! - A boundary-shaped suffix (e.g. `UserDto`) is a *signal*, not acceptance —
//!   it shows up as a `reason` on the diagnostic, not as a free pass.
//! - Confidence is tracked on each member so rules can pick their threshold.

use std::collections::{BTreeMap, HashSet};

use locus_air::{AirFile, AirHint, AirItem, AirSpan, AirType, AirWorkspace, HintKind, TypeKind};

use super::lockfile_schema::{LockedRole, OtSection};

/// Suffixes that signal "this is a boundary-shape variant of a domain concept."
/// Order matters for stem extraction — longest first.
const BOUNDARY_SUFFIXES: &[&str] = &[
    "Response", "Request", "Payload", "Schema", "Message", "Record", "Entity", "Model", "Reply",
    "View", "Body", "Resp", "Req", "Row", "Dto",
];

/// Minimum field-overlap (Jaccard on field-name sets) for two types in the
/// same name-stem bucket to count as the same concept.
pub const FIELD_OVERLAP_THRESHOLD: f32 = 0.5;

#[derive(Debug, Clone)]
pub struct ConceptCluster {
    pub concept_id: String,
    pub stem: String,
    pub members: Vec<ClusterMember>,
    /// Confidence the cluster represents one concept (0.0..=1.0). Computed
    /// from per-member field overlap with the canonical/reference member,
    /// presence of a `From`/`TryFrom` between any two members, and base
    /// stem-match strength. Suggestion-tiering reads this; the existing
    /// init code (which only promotes hint-tagged members) ignores it.
    pub confidence: f32,
}

#[derive(Debug, Clone)]
pub struct ClusterMember {
    pub symbol: String,
    pub name: String,
    pub role: InferredRole,
    pub span: AirSpan,
    pub file_path: String,
    /// Jaccard overlap of this member's field names against the canonical
    /// (or, if no canonical, the largest member). 1.0 if it's the canonical.
    pub field_overlap: f32,
    pub fields: Vec<String>,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InferredRole {
    /// Hinted `// ot: canonical` (or, in future, accepted in lockfile).
    Canonical,
    /// Hinted `// ot: boundary` (or, in future, accepted in lockfile).
    Boundary,
    /// No acceptance recorded. Some types in this state are diagnostic targets
    /// (OT002); others are simply unrelated types we couldn't classify.
    Unknown,
}

pub fn cluster_concepts(air: &AirWorkspace) -> Vec<ConceptCluster> {
    cluster_concepts_with_lockfile(air, &OtSection::default())
}

/// Same as [`cluster_concepts`] but also consults the lockfile for
/// already-accepted symbols, which override hint-derived roles.
pub fn cluster_concepts_with_lockfile(
    air: &AirWorkspace,
    section: &OtSection,
) -> Vec<ConceptCluster> {
    // Collect every struct/enum across the workspace, paired with its file
    // (we need the file's hint list to assign roles).
    let mut by_stem: BTreeMap<String, Vec<TypeRef<'_>>> = BTreeMap::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                let AirItem::Type(ty) = item else { continue };
                if !is_clusterable(ty) {
                    continue;
                }
                let stem = stem_of(&ty.name);
                by_stem.entry(stem).or_default().push(TypeRef { ty, file });
            }
        }
    }

    let mut out = Vec::new();
    for (stem, members) in by_stem {
        if members.len() < 2 {
            continue; // single-member buckets aren't a cluster yet
        }

        // Pick a "reference" type (the canonical if present, else the one with
        // the most fields — the most likely canonical shape).
        let ref_idx = pick_reference(&members, section);
        let reference_fields = field_set(members[ref_idx].ty);

        let mut cluster_members = Vec::with_capacity(members.len());
        for m in &members {
            let fields = field_set(m.ty);
            let overlap = jaccard(&fields, &reference_fields);
            let role = role_for_type(m.ty, m.file, section);
            let mut reasons = Vec::new();
            if !m.ty.name.eq_ignore_ascii_case(&stem)
                && let Some(suf) = matched_suffix(&m.ty.name)
            {
                reasons.push(format!("name suffix `{suf}`"));
            }
            reasons.push(format!(
                "field overlap with `{}`: {:.0}%",
                members[ref_idx].ty.name,
                overlap * 100.0
            ));
            // OT inference treats serde derives as a "boundary-shaped"
            // signal. After AIR v13 the field is `decorators` with a
            // `source` tag; we filter to Rust derives only so non-Rust
            // adapters that surface decorators differently don't
            // accidentally light up the same heuristic.
            if m.ty.decorators.iter().any(|d| {
                matches!(d.source, locus_air::DecoratorSource::Derive)
                    && (d.name == "Serialize" || d.name == "Deserialize")
            }) {
                reasons.push("derives Serialize/Deserialize".into());
            }

            cluster_members.push(ClusterMember {
                symbol: m.ty.symbol.clone(),
                name: m.ty.name.clone(),
                role,
                span: m.ty.span.clone(),
                file_path: m.file.path.clone(),
                field_overlap: overlap,
                fields: fields.into_iter().collect(),
                reasons,
            });
        }

        // Drop the cluster if no member meets the overlap threshold (other
        // than the reference itself). Stems alone aren't enough.
        let has_real_overlap = cluster_members
            .iter()
            .filter(|m| m.field_overlap >= FIELD_OVERLAP_THRESHOLD)
            .count()
            >= 2;
        if !has_real_overlap {
            continue;
        }

        let confidence = compute_cluster_confidence(&cluster_members);
        // Boost confidence when a converter exists between any two members.
        // We re-walk the workspace because converter symbols live outside
        // the cluster's `members` list.
        let confidence = if has_converter_between_members_via_air(air, &cluster_members) {
            (confidence + 0.2).min(1.0)
        } else {
            confidence
        };

        out.push(ConceptCluster {
            concept_id: concept_id_from_stem(&stem),
            stem,
            members: cluster_members,
            confidence,
        });
    }
    out
}

struct TypeRef<'a> {
    ty: &'a AirType,
    file: &'a AirFile,
}

fn is_clusterable(ty: &AirType) -> bool {
    // Phase 2 only clusters structs and enums. Aliases/unions skipped — too
    // noisy until we resolve the alias targets.
    matches!(ty.kind, TypeKind::Struct | TypeKind::Enum) && !ty.fields.is_empty()
        || matches!(ty.kind, TypeKind::Enum) && !ty.variants.is_empty()
}

fn stem_of(name: &str) -> String {
    if let Some(suf) = matched_suffix(name) {
        let stem = &name[..name.len() - suf.len()];
        if !stem.is_empty() {
            return stem.to_string();
        }
    }
    name.to_string()
}

fn matched_suffix(name: &str) -> Option<&'static str> {
    BOUNDARY_SUFFIXES
        .iter()
        .copied()
        .find(|suf| name.len() > suf.len() && name.ends_with(suf))
}

/// Given a type name (e.g. `UserDto`, `User`, `EmailAddress`), produce the
/// concept id Locus would assign to it (`user`, `user`, `email-address`).
/// Used by both inference and `locus accept` so they agree.
pub fn stem_concept_id(name: &str) -> String {
    concept_id_from_stem(&stem_of(name))
}

fn concept_id_from_stem(stem: &str) -> String {
    // CamelCase → kebab-case-ish concept id. `User` → `user`; `EmailAddress`
    // → `email-address`. Concept namespacing (e.g. `identity.user`) is a
    // lockfile decision, not an inference output — we just provide the stem.
    let mut out = String::with_capacity(stem.len());
    for (i, c) in stem.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                out.push('-');
            }
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

fn pick_reference(members: &[TypeRef<'_>], section: &OtSection) -> usize {
    // Prefer a member with `// ot: canonical` or a lockfile-accepted canonical.
    // Otherwise the member with the most fields (most likely the canonical shape).
    if let Some(idx) = members.iter().position(|m| {
        matches!(
            role_for_type(m.ty, m.file, section),
            InferredRole::Canonical
        )
    }) {
        return idx;
    }
    members
        .iter()
        .enumerate()
        .max_by_key(|(_, m)| m.ty.fields.len())
        .map(|(i, _)| i)
        .unwrap_or(0)
}

fn field_set(ty: &AirType) -> HashSet<String> {
    if !ty.fields.is_empty() {
        return ty.fields.iter().map(|f| f.name.clone()).collect();
    }
    // For enums with no top-level fields, use variant names as the "shape."
    ty.variants.iter().map(|v| v.name.clone()).collect()
}

fn jaccard(a: &HashSet<String>, b: &HashSet<String>) -> f32 {
    if a.is_empty() && b.is_empty() {
        return 0.0;
    }
    let inter = a.intersection(b).count();
    let union = a.union(b).count();
    if union == 0 {
        0.0
    } else {
        inter as f32 / union as f32
    }
}

fn role_for_type(ty: &AirType, file: &AirFile, section: &OtSection) -> InferredRole {
    // Lockfile is authoritative — it represents accepted ownership, which can
    // outlive or override source hints (e.g. a hint was removed but the
    // acceptance still stands).
    if let Some((role, _)) = section.role_of(&ty.symbol) {
        return match role {
            LockedRole::Canonical => InferredRole::Canonical,
            LockedRole::Boundary => InferredRole::Boundary,
        };
    }
    let hits = hints_for_type(file, ty);
    if hits.iter().any(|h| matches!(h.kind, HintKind::Canonical)) {
        return InferredRole::Canonical;
    }
    if hits
        .iter()
        .any(|h| matches!(h.kind, HintKind::Boundary { .. }))
    {
        return InferredRole::Boundary;
    }
    InferredRole::Unknown
}

fn hints_for_type<'a>(file: &'a AirFile, ty: &AirType) -> Vec<&'a AirHint> {
    // syn's span over an item includes its attributes, so a hint placed
    // above `#[derive(...)] pub struct X` may resolve (via the scanner's
    // attribute-skip) to the struct line — which is *inside* the syn span,
    // not at its start. Match against the full span range for robustness.
    file.hints
        .iter()
        .filter(|h| {
            let Some(t) = h.target_span.as_ref() else {
                return false;
            };
            t.line_start >= ty.span.line_start && t.line_start <= ty.span.line_end
        })
        .collect()
}

fn compute_cluster_confidence(members: &[ClusterMember]) -> f32 {
    let canonical = members.iter().find(|m| m.role == InferredRole::Canonical);
    // Base score: stems already match (cluster_concepts only emits clusters
    // with members sharing a stem), so 0.4 baseline.
    let mut score = 0.4f32;
    if let Some(canonical) = canonical {
        // Mean field-overlap across non-canonical members against the
        // canonical's perspective. (Existing per-member overlap is computed
        // against the *reference* type, which is the canonical when one
        // exists — see `pick_reference`.)
        let _ = canonical; // canonical present; existing field_overlap already references it
        let others: Vec<&ClusterMember> = members
            .iter()
            .filter(|m| m.role != InferredRole::Canonical)
            .collect();
        if !others.is_empty() {
            let mean_overlap: f32 =
                others.iter().map(|m| m.field_overlap).sum::<f32>() / others.len() as f32;
            score += 0.4 * mean_overlap;
        }
    } else {
        // No canonical (no `// ot:` hint): be more conservative; rely on
        // the average field overlap across all members against the reference.
        if !members.is_empty() {
            let mean_overlap: f32 =
                members.iter().map(|m| m.field_overlap).sum::<f32>() / members.len() as f32;
            score += 0.3 * mean_overlap;
        }
    }
    score.min(1.0)
}

fn has_converter_between_members_via_air(air: &AirWorkspace, members: &[ClusterMember]) -> bool {
    use std::collections::BTreeSet;
    let names: BTreeSet<&str> = members.iter().map(|m| m.symbol.as_str()).collect();
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                let AirItem::Conversion(c) = item else {
                    continue;
                };
                if symbol_matches_any(&c.from, &names) && symbol_matches_any(&c.to, &names) {
                    return true;
                }
            }
        }
    }
    false
}

fn symbol_matches_any(needle: &str, accepted: &std::collections::BTreeSet<&str>) -> bool {
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
    fn stem_strips_known_suffixes() {
        assert_eq!(stem_of("User"), "User");
        assert_eq!(stem_of("UserDto"), "User");
        assert_eq!(stem_of("UserModel"), "User");
        assert_eq!(stem_of("UserResponse"), "User");
        assert_eq!(stem_of("UserId"), "UserId"); // `Id` is not a boundary suffix
    }

    #[test]
    fn jaccard_matches_set_ratio() {
        let a: HashSet<_> = ["id", "email"].iter().map(|s| s.to_string()).collect();
        let b: HashSet<_> = ["id", "email", "name"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let j = jaccard(&a, &b);
        // |a ∩ b| = 2, |a ∪ b| = 3 → 2/3
        assert!((j - 0.666_666_7).abs() < 1e-3, "got {j}");
    }

    #[test]
    fn concept_id_kebabs_camelcase() {
        assert_eq!(concept_id_from_stem("User"), "user");
        assert_eq!(concept_id_from_stem("EmailAddress"), "email-address");
    }

    #[test]
    fn confidence_is_baseline_when_no_canonical_no_overlap_signal() {
        // Build a 2-member cluster with no hints and zero overlap (different
        // field names) — but Jaccard on identical-stem same-shape passes
        // FIELD_OVERLAP_THRESHOLD via `pick_reference` semantics. Use
        // identical fields so cluster fires; confidence stays in baseline
        // band (no canonical hint = at most 0.4 + 0.3*1.0 = 0.7).
        // Easiest path: rely on the corpus or a focused fixture; here we
        // just assert the non-canonical-path bound from a built fixture.
        // Test that when there's no canonical, score <= 0.7.
        let members = vec![
            ClusterMember {
                symbol: "X::A".into(),
                name: "A".into(),
                role: InferredRole::Unknown,
                span: AirSpan::new("a.rs", 1, 1),
                file_path: "a.rs".into(),
                field_overlap: 1.0,
                fields: vec!["x".into()],
                reasons: Vec::new(),
            },
            ClusterMember {
                symbol: "X::B".into(),
                name: "B".into(),
                role: InferredRole::Unknown,
                span: AirSpan::new("b.rs", 1, 1),
                file_path: "b.rs".into(),
                field_overlap: 1.0,
                fields: vec!["x".into()],
                reasons: Vec::new(),
            },
        ];
        let c = compute_cluster_confidence(&members);
        assert!(c <= 0.7 + f32::EPSILON, "got {c}");
    }

    #[test]
    fn confidence_with_canonical_and_full_overlap_is_high() {
        let members = vec![
            ClusterMember {
                symbol: "X::A".into(),
                name: "A".into(),
                role: InferredRole::Canonical,
                span: AirSpan::new("a.rs", 1, 1),
                file_path: "a.rs".into(),
                field_overlap: 1.0,
                fields: vec!["x".into()],
                reasons: Vec::new(),
            },
            ClusterMember {
                symbol: "X::B".into(),
                name: "B".into(),
                role: InferredRole::Boundary,
                span: AirSpan::new("b.rs", 1, 1),
                file_path: "b.rs".into(),
                field_overlap: 1.0,
                fields: vec!["x".into()],
                reasons: Vec::new(),
            },
        ];
        let c = compute_cluster_confidence(&members);
        // 0.4 baseline + 0.4 * 1.0 mean overlap = 0.8.
        assert!((c - 0.8).abs() < 0.01, "got {c}");
    }
}
