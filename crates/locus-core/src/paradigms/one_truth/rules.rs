//! OT rules.
//!
//! Implemented:
//! - [`ot001`]: duplicate canonical for a single concept
//! - [`ot002`]: undeclared concept-shaped type (warning by default)
//! - [`ot003`]: boundary type leaked into a non-boundary function signature
//! - [`ot004`]: direct canonical construction outside owner / accepted converter
//! - [`ot005`]: accepted boundary with no accepted converter
//! - [`ot006`]: unregistered conversion between accepted endpoints
//! - [`ot007`]: adapter-to-adapter conversion (both endpoints are boundaries)
//! - [`ot008`]: domain-shaped method on an accepted boundary
//! - [`ot009`]: scattered validation/normalization outside the canonical owner
//! - [`ot010`]: shadow enum overlapping an accepted canonical enum
//! - [`ot011`]: shadow newtype/value object overlapping a canonical value object
//! - [`ot012`]: primitive-typed field where a canonical value object is expected
//!
//! All rules except OT001/OT002 are lockfile-driven — they stay silent until
//! `locus init` (or `locus accept`) has populated the OT section. This is
//! deliberate: pre-onboarding, we don't have the data to distinguish
//! intent from drift.

use std::collections::{BTreeMap, BTreeSet};

use locus_air::{ActionKind, AirItem, AirSpan, AirWorkspace, HintKind, TypeKind};

use super::infer::{ConceptCluster, FIELD_OVERLAP_THRESHOLD, InferredRole};
use super::lockfile_schema::OtSection;
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

/// OT002 — undeclared concept-shaped type.
///
/// Fires when a cluster contains:
/// - at least one Canonical member (annotated `// locus: ot canonical`), and
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
                "annotate as boundary: `// locus: ot boundary {} <boundary-name>` above `{}`, \
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
/// - multiple `// locus: ot canonical` annotations across types in the same stem
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
        // the redundant `// locus: ot canonical` annotation or rename the type.
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
                    "drop the `// locus: ot canonical` annotation on `{}` and either \
                     re-annotate it as `// locus: ot boundary {} <name>` or rename the type",
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

/// OT003 — boundary adapter leak.
///
/// Fires when a function lives in a non-boundary file, isn't an accepted
/// converter, and has a parameter or return type that references an
/// accepted boundary type (by short name).
///
/// "Boundary file" = any file in the workspace that defines an accepted
/// boundary symbol. Boundary code is allowed to use boundary types freely;
/// only domain/application code must convert at the edge.
///
/// Always Fatal: boundary leaks are the headline OT violation per the spec.
pub fn ot003(air: &AirWorkspace, section: &OtSection, mode: CheckMode) -> Vec<Diagnostic> {
    let mut boundary_files: BTreeSet<String> = BTreeSet::new();
    let mut boundary_short_names: Vec<(String, String)> = Vec::new(); // (short, concept)
    for (concept_id, entry) in &section.concepts {
        for b in &entry.boundaries {
            if let Some(file_path) = file_of_symbol(air, &b.symbol) {
                boundary_files.insert(file_path);
            }
            if let Some(short) = b.symbol.rsplit("::").next() {
                boundary_short_names.push((short.to_string(), concept_id.clone()));
            }
        }
    }
    if boundary_short_names.is_empty() {
        return Vec::new();
    }
    let accepted_converters: BTreeSet<&str> = section
        .concepts
        .values()
        .flat_map(|e| e.converters.iter().map(|c| c.symbol.as_str()))
        .collect();

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            if boundary_files.contains(&file.path) {
                continue;
            }
            for item in &file.items {
                let AirItem::Function(f) = item else {
                    continue;
                };
                if accepted_converters.contains(f.symbol.as_str()) {
                    continue;
                }
                // Aggregate every boundary referenced in any signature slot,
                // emit one diagnostic per (function, boundary). Multiple
                // diagnostics for the same boundary in different params would
                // be noise; one is enough.
                let mut hits: BTreeMap<String, String> = BTreeMap::new(); // short → concept
                for (_, ty_text) in &f.params {
                    for (short, concept) in &boundary_short_names {
                        if type_text_references(ty_text, short) {
                            hits.entry(short.clone()).or_insert_with(|| concept.clone());
                        }
                    }
                }
                if let Some(ret) = &f.return_type {
                    for (short, concept) in &boundary_short_names {
                        if type_text_references(ret, short) {
                            hits.entry(short.clone()).or_insert_with(|| concept.clone());
                        }
                    }
                }
                for (short, concept) in hits {
                    out.push(Diagnostic {
                        rule_id: "OT003".to_string(),
                        severity: mode.elevate(Severity::Fatal),
                        span: f.span.clone(),
                        concept: Some(concept.clone()),
                        message: format!(
                            "function `{}` exposes boundary type `{}` in its signature; \
                             boundary types must be converted before crossing into \
                             domain/application code",
                            f.symbol, short
                        ),
                        why: vec![
                            format!("file `{}` is not a boundary file (no accepted boundary lives here)", f.span.file),
                            format!("`{short}` is the accepted boundary for concept `{concept}`"),
                            format!("`{}` is not an accepted converter", f.symbol),
                        ],
                        suggested_fix: Some(format!(
                            "convert `{short}` at the edge: \
                             `let domain = canonical_for_{concept}::try_from(value)?;`, \
                             then take the canonical type in this signature instead"
                        )),
                    });
                }
            }
        }
    }
    out
}

/// OT004 — direct canonical construction outside owner or accepted converter.
///
/// Walks every `Construct` truth-action in AIR. Fires when the constructed
/// type is an accepted canonical, the construction is *not* in the owner
/// file, and the enclosing function is *not* an accepted converter.
///
/// Always Fatal: per the spec, canonical types may only be constructed in
/// their owner module or in named, accepted converters. Anywhere else is
/// authority fragmentation.
pub fn ot004(air: &AirWorkspace, section: &OtSection, mode: CheckMode) -> Vec<Diagnostic> {
    // canonical short_name → (full symbol, owner file path, concept id)
    let mut canonicals: BTreeMap<String, (String, String, String)> = BTreeMap::new();
    for (concept_id, entry) in &section.concepts {
        let symbol = &entry.canonical.symbol;
        let Some(short) = symbol.rsplit("::").next() else {
            continue;
        };
        let Some(file_path) = file_of_symbol(air, symbol) else {
            continue;
        };
        canonicals.insert(
            short.to_string(),
            (symbol.clone(), file_path, concept_id.clone()),
        );
    }
    if canonicals.is_empty() {
        return Vec::new();
    }
    let accepted_converters: BTreeSet<&str> = section
        .concepts
        .values()
        .flat_map(|e| e.converters.iter().map(|c| c.symbol.as_str()))
        .collect();

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                let AirItem::TruthAction(a) = item else {
                    continue;
                };
                if a.action != ActionKind::Construct {
                    continue;
                }
                // `target` is the rendered constructed-path text (e.g. `User`
                // or `crate::dto::UserModel`). Use the last `::` segment so
                // path-prefixed literal forms still match.
                let short = a
                    .target
                    .rsplit("::")
                    .next()
                    .unwrap_or(a.target.as_str())
                    .trim();
                let Some((canonical_symbol, owner_file, concept_id)) = canonicals.get(short) else {
                    continue;
                };
                if &file.path == owner_file {
                    continue; // construction in owner module is fine
                }
                if let Some(fn_sym) = &a.function
                    && accepted_converters.contains(fn_sym.as_str())
                {
                    continue; // construction inside an accepted converter is fine
                }
                if section.converter_paths.iter().any(|p| {
                    a.function
                        .as_deref()
                        .is_some_and(|f| matches_symbol_pattern(f, p))
                        || matches_symbol_pattern(&file.path, p)
                }) {
                    continue; // accepted by OT.converter_paths authority
                }
                let function_label = a
                    .function
                    .as_deref()
                    .unwrap_or("(no enclosing function recorded)");
                out.push(Diagnostic {
                    rule_id: "OT004".to_string(),
                    severity: mode.elevate(Severity::Fatal),
                    span: a.span.clone(),
                    concept: Some(concept_id.clone()),
                    message: format!(
                        "direct construction of canonical `{canonical_symbol}` outside its owner module \
                         and outside any accepted converter"
                    ),
                    why: vec![
                        format!("constructed at `{}:{}`", a.span.file, a.span.line_start),
                        format!("owner module is `{owner_file}`"),
                        format!("enclosing function `{function_label}` is not an accepted converter"),
                    ],
                    suggested_fix: Some(format!(
                        "go through the accepted converter (e.g. `{canonical_symbol}::try_from(value)?`), \
                         or accept this function as a converter and rerun `locus init`"
                    )),
                });
            }
        }
    }
    out
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
fn matches_symbol_pattern(value: &str, pattern: &str) -> bool {
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

/// Look up the file path of the AIR type whose `symbol` matches `target`.
fn file_of_symbol(air: &AirWorkspace, target: &str) -> Option<String> {
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
fn span_of_symbol(air: &AirWorkspace, target: &str) -> Option<AirSpan> {
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
fn short_name(path: &str) -> &str {
    path.rsplit("::").next().unwrap_or(path).trim()
}

/// OT005 — missing converter for an accepted boundary.
///
/// Fires when a concept has accepted boundaries but no accepted converter
/// mentions a given boundary (in either direction). The spec eventually wants
/// this to track inbound vs outbound directions; for Phase 2 we only require
/// at least one converter touching the boundary.
///
/// Always Fatal: a boundary with no converter is a dead end — boundary data
/// either can't reach the canonical or can't leave it.
pub fn ot005(air: &AirWorkspace, section: &OtSection, mode: CheckMode) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (concept_id, entry) in &section.concepts {
        for boundary in &entry.boundaries {
            let short = short_name(&boundary.symbol);
            let has_converter = entry
                .converters
                .iter()
                .any(|c| short_name(&c.from) == short || short_name(&c.to) == short);
            if has_converter {
                continue;
            }
            let span = span_of_symbol(air, &boundary.symbol)
                .unwrap_or_else(|| AirSpan::new(boundary.symbol.clone(), 1, 1));
            out.push(Diagnostic {
                rule_id: "OT005".to_string(),
                severity: mode.elevate(Severity::Fatal),
                span,
                concept: Some(concept_id.clone()),
                message: format!(
                    "boundary `{}` (concept `{concept_id}`) has no accepted converter \
                     to/from the canonical",
                    boundary.symbol
                ),
                why: vec![
                    format!("canonical: `{}`", entry.canonical.symbol),
                    format!(
                        "no entry under `paradigms.OT.concepts.{concept_id}.converters` \
                         mentions `{short}` on either side"
                    ),
                ],
                suggested_fix: Some(format!(
                    "add an `impl TryFrom<{short}> for {}` (or its inverse) and rerun \
                     `locus init`; alternatively remove the boundary acceptance if it's \
                     no longer needed",
                    short_name(&entry.canonical.symbol),
                )),
            });
        }
    }
    out
}

/// OT007 — adapter-to-adapter conversion.
///
/// Fires on every `AirConversion` whose endpoints are both lockfile-accepted
/// boundaries (in any concept). Adapter-to-adapter conversions bypass the
/// canonical and create a hidden translation path; the preferred shape is
/// `adapter → canonical → adapter`.
///
/// Suppressed when a `// locus: ot protocol-translation reason="…"` hint binds to
/// the conversion's span — the explicit "yes I really mean this" escape hatch
/// from the spec.
///
/// Always Fatal otherwise.
pub fn ot007(air: &AirWorkspace, section: &OtSection, mode: CheckMode) -> Vec<Diagnostic> {
    let mut boundary_to_concept: BTreeMap<String, String> = BTreeMap::new();
    for (concept_id, entry) in &section.concepts {
        for b in &entry.boundaries {
            boundary_to_concept.insert(short_name(&b.symbol).to_string(), concept_id.clone());
        }
    }
    if boundary_to_concept.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                let AirItem::Conversion(c) = item else {
                    continue;
                };
                let from_short = short_name(&c.from);
                let to_short = short_name(&c.to);
                let Some(from_concept) = boundary_to_concept.get(from_short) else {
                    continue;
                };
                let Some(to_concept) = boundary_to_concept.get(to_short) else {
                    continue;
                };

                if conversion_has_protocol_translation_hint(&file.hints, &c.span) {
                    continue;
                }

                let cross_label = if from_concept == to_concept {
                    "within the same concept".to_string()
                } else {
                    format!("across concepts (`{from_concept}` → `{to_concept}`)")
                };
                out.push(Diagnostic {
                    rule_id: "OT007".to_string(),
                    severity: mode.elevate(Severity::Fatal),
                    span: c.span.clone(),
                    concept: Some(from_concept.clone()),
                    message: format!(
                        "adapter-to-adapter conversion `{}` ({} → {}) — both endpoints \
                         are accepted boundaries",
                        c.symbol, c.from, c.to
                    ),
                    why: vec![
                        format!("`{from_short}` is a boundary for `{from_concept}`"),
                        format!("`{to_short}` is a boundary for `{to_concept}`"),
                        format!("conversion routes {cross_label}"),
                        "preferred path: adapter → canonical → adapter".into(),
                    ],
                    suggested_fix: Some(
                        "go through the canonical (e.g. `Canonical::try_from(from)?` then \
                         `Other::from(canonical)`), or annotate the conversion with \
                         `// locus: ot protocol-translation reason=\"...\"` if it's an \
                         intentional shortcut"
                            .into(),
                    ),
                });
            }
        }
    }
    out
}

/// True if any `// locus: ot protocol-translation` hint in the file has a
/// `target_span` that lands within the conversion's span.
fn conversion_has_protocol_translation_hint(hints: &[locus_air::AirHint], span: &AirSpan) -> bool {
    hints.iter().any(|h| {
        matches!(h.kind, HintKind::ProtocolTranslation { .. })
            && h.target_span
                .as_ref()
                .is_some_and(|t| t.line_start >= span.line_start && t.line_start <= span.line_end)
    })
}

/// Whole-identifier match: returns true if `name` appears in `text` not as a
/// substring of a longer identifier. `Result<UserDto, …>` references `UserDto`
/// but `UserDtoVec` does not.
fn type_text_references(text: &str, name: &str) -> bool {
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

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// OT008 — domain logic on a boundary adapter.
///
/// Fires when an inherent `impl AcceptedBoundary { ... }` declares a method
/// whose name is *not* in the boundary-shape allowlist (`from`, `try_from`,
/// `into`, `serialize`, `fmt`, …). Domain queries / behaviours
/// (`is_active`, `validate`, `apply_to`, `total_price`, …) belong on the
/// canonical, not the wire/storage shape.
///
/// Confidence 0.85 — name-only heuristic; the method body could be a pure
/// projection and we can't tell from AIR. Per the spec's severity table
/// (`docs/PARADIGMS.md` §"Severity tiers"), this is warning by default and
/// fatal under `--agent-strict`. [`Severity::from_confidence`] does the
/// mapping.
pub fn ot008(air: &AirWorkspace, section: &OtSection, mode: CheckMode) -> Vec<Diagnostic> {
    let mut boundary_short_to_concept: BTreeMap<String, String> = BTreeMap::new();
    for (concept_id, entry) in &section.concepts {
        for b in &entry.boundaries {
            boundary_short_to_concept.insert(short_name(&b.symbol).to_string(), concept_id.clone());
        }
    }
    if boundary_short_to_concept.is_empty() {
        return Vec::new();
    }

    let confidence = 0.85;
    let Some(severity) = Severity::from_confidence(confidence, mode) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                let AirItem::Impl(im) = item else {
                    continue;
                };
                if im.interface.is_some() {
                    // Trait impls (`impl From<X> for Y`, `impl Display for Y`,
                    // serde derives, etc.) are projection by construction —
                    // they're how boundary types translate, not domain logic.
                    continue;
                }
                let self_short = short_name(&im.target_type);
                let Some(concept_id) = boundary_short_to_concept.get(self_short) else {
                    continue;
                };
                for method in &im.method_names {
                    if is_boundary_shape_method(method) {
                        continue;
                    }
                    out.push(Diagnostic {
                        rule_id: "OT008".to_string(),
                        severity,
                        span: im.span.clone(),
                        concept: Some(concept_id.clone()),
                        message: format!(
                            "boundary `{self_short}` carries domain-shaped method \
                             `{method}` — boundary adapters should only translate, \
                             not reason about, the concept"
                        ),
                        why: vec![
                            format!("`{self_short}` is the accepted boundary for `{concept_id}`"),
                            format!(
                                "`{method}` is not in the boundary-shape allowlist \
                                 (from/try_from/into/as_*/to_*/serialize/deserialize/fmt/new/default/builder)"
                            ),
                            format!("inference confidence: {confidence:.2}"),
                        ],
                        suggested_fix: Some(format!(
                            "move `{method}` onto the canonical for `{concept_id}` \
                             (where domain behaviour lives), or rename it into the \
                             boundary-shape allowlist if it really is pure translation"
                        )),
                    });
                }
            }
        }
    }
    out
}

/// True for method names that are part of the *translation* surface of a
/// boundary adapter (and so allowed by OT008). The list is conservative —
/// when in doubt prefer false-positive (a flag) over false-negative
/// (a missed domain leak), then expand the allowlist if the user pushes back.
fn is_boundary_shape_method(name: &str) -> bool {
    // Exact-match conversions, accessors, factories, and stdlib trait shims.
    const EXACT: &[&str] = &[
        "from",
        "try_from",
        "into",
        "try_into",
        "serialize",
        "deserialize",
        "fmt",
        "display",
        "new",
        "default",
        "builder",
        "build",
        "clone",
        "as_ref",
        "as_mut",
        "as_str",
        "as_bytes",
        "into_inner",
        "inner",
        "len",
        "is_empty",
    ];
    if EXACT.contains(&name) {
        return true;
    }
    // Conventional translation prefixes.
    name.starts_with("as_")
        || name.starts_with("to_")
        || name.starts_with("into_")
        || name.starts_with("from_")
        || name.starts_with("try_")
        || name.starts_with("with_")
}

/// OT009 — scattered validation/normalization.
///
/// Fires when a function lives outside the canonical's owner file *and*
/// outside any accepted converter, but its *name* and *signature* both look
/// like validation/normalization of a known canonical (e.g. `validate_email`
/// returning a `Result<EmailAddress, _>`, or `normalize_user_id(s: &str)
/// -> UserId`). Both signals are required so generic helpers
/// (`fn validate_input(...)`) don't trip the rule.
///
/// Confidence 0.75. The spec lists this as "warning by default; fatal under
/// `--agent-strict` for high-confidence cases" — `from_confidence(0.75,
/// AgentStrict)` returns `Fatal`, `(0.75, Human)` returns `Warning`.
pub fn ot009(air: &AirWorkspace, section: &OtSection, mode: CheckMode) -> Vec<Diagnostic> {
    let mut canonicals: BTreeMap<String, (String, String)> = BTreeMap::new(); // short → (concept, owner_file)
    for (concept_id, entry) in &section.concepts {
        let symbol = &entry.canonical.symbol;
        let Some(short) = symbol.rsplit("::").next() else {
            continue;
        };
        let Some(file_path) = file_of_symbol(air, symbol) else {
            continue;
        };
        canonicals.insert(short.to_string(), (concept_id.clone(), file_path));
    }
    if canonicals.is_empty() {
        return Vec::new();
    }
    let accepted_converters: BTreeSet<&str> = section
        .concepts
        .values()
        .flat_map(|e| e.converters.iter().map(|c| c.symbol.as_str()))
        .collect();

    let confidence = 0.75;
    let Some(severity) = Severity::from_confidence(confidence, mode) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                let AirItem::Function(f) = item else {
                    continue;
                };
                if accepted_converters.contains(f.symbol.as_str()) {
                    continue;
                }
                let Some(prefix) = matched_validate_prefix(&f.name) else {
                    continue;
                };
                // Find a canonical referenced in params/return — that's the
                // "operates on a known concept" part of the signal.
                let mut concept_match: Option<(&str, &str)> = None;
                for (short, (concept, owner)) in &canonicals {
                    let signature_hits =
                        f.params.iter().any(|(_, t)| type_text_references(t, short))
                            || f.return_type
                                .as_deref()
                                .is_some_and(|t| type_text_references(t, short));
                    if signature_hits {
                        concept_match = Some((concept.as_str(), owner.as_str()));
                        break;
                    }
                }
                let Some((concept_id, owner_file)) = concept_match else {
                    continue;
                };
                if file.path == owner_file {
                    continue; // validator inside the canonical's own module is fine
                }
                out.push(Diagnostic {
                    rule_id: "OT009".to_string(),
                    severity,
                    span: f.span.clone(),
                    concept: Some(concept_id.to_string()),
                    message: format!(
                        "`{}` looks like {prefix} for canonical `{concept_id}` but lives \
                         outside the owner module and outside any accepted converter",
                        f.symbol
                    ),
                    why: vec![
                        format!(
                            "function name starts with `{prefix}` (validation/normalization shape)"
                        ),
                        format!("signature references canonical for `{concept_id}`"),
                        format!("owner module: `{owner_file}`"),
                        format!("inference confidence: {confidence:.2}"),
                    ],
                    suggested_fix: Some(format!(
                        "move this into the owner of `{concept_id}` (so the canonical \
                         enforces its own invariants), or accept this function as a \
                         converter via `locus init` if it's the legitimate edge"
                    )),
                });
            }
        }
    }
    out
}

/// Returns the matched prefix if `name` starts with one of the
/// validation/normalization shape prefixes recognised by OT009.
fn matched_validate_prefix(name: &str) -> Option<&'static str> {
    const PREFIXES: &[&str] = &[
        "validate_",
        "is_valid_",
        "check_",
        "verify_",
        "ensure_",
        "normalize_",
        "sanitize_",
        "canonicalize_",
        "parse_",
        "clean_",
    ];
    PREFIXES.iter().copied().find(|p| name.starts_with(p))
}

/// OT010 — shadow enum.
///
/// Fires for each enum that:
/// 1. Is not lockfile-accepted (canonical or boundary), and
/// 2. Shares ≥ 50% of its variant names with an accepted canonical enum.
///
/// 50% is the same Jaccard threshold OT002 uses for struct field overlap
/// (`FIELD_OVERLAP_THRESHOLD`). Confidence is 0.85 — variant-name overlap is
/// a fairly specific signal but not bullet-proof (`Active`/`Inactive` shows
/// up everywhere).
pub fn ot010(air: &AirWorkspace, section: &OtSection, mode: CheckMode) -> Vec<Diagnostic> {
    // Collect every accepted canonical enum's variant set.
    let mut canonical_enums: Vec<(String, String, BTreeSet<String>)> = Vec::new(); // (concept, symbol, variants)
    for (concept_id, entry) in &section.concepts {
        let symbol = &entry.canonical.symbol;
        let Some((variants, kind)) = type_variants_and_kind(air, symbol) else {
            continue;
        };
        if kind != TypeKind::Enum {
            continue;
        }
        canonical_enums.push((concept_id.clone(), symbol.clone(), variants));
    }
    if canonical_enums.is_empty() {
        return Vec::new();
    }
    let confidence = 0.85;
    let Some(severity) = Severity::from_confidence(confidence, mode) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                let AirItem::Type(ty) = item else {
                    continue;
                };
                if ty.kind != TypeKind::Enum {
                    continue;
                }
                if section.role_of(&ty.symbol).is_some() {
                    continue; // already accepted
                }
                let candidate_variants: BTreeSet<String> =
                    ty.variants.iter().map(|v| v.name.clone()).collect();
                if candidate_variants.is_empty() {
                    continue;
                }
                for (concept_id, canonical_symbol, canonical_variants) in &canonical_enums {
                    if &ty.symbol == canonical_symbol {
                        continue;
                    }
                    let overlap = jaccard_str(&candidate_variants, canonical_variants);
                    if overlap < FIELD_OVERLAP_THRESHOLD {
                        continue;
                    }
                    out.push(Diagnostic {
                        rule_id: "OT010".to_string(),
                        severity,
                        span: ty.span.clone(),
                        concept: Some(concept_id.clone()),
                        message: format!(
                            "enum `{}` overlaps {:.0}% with accepted canonical `{canonical_symbol}` \
                             but is not accepted as canonical or boundary",
                            ty.symbol,
                            overlap * 100.0
                        ),
                        why: vec![
                            format!("variants: {{{}}}", join_sorted(&candidate_variants)),
                            format!(
                                "canonical `{canonical_symbol}` variants: {{{}}}",
                                join_sorted(canonical_variants)
                            ),
                            format!("Jaccard overlap: {:.2}", overlap),
                            format!("inference confidence: {confidence:.2}"),
                        ],
                        suggested_fix: Some(format!(
                            "remove `{}` and use `{canonical_symbol}` directly, or accept \
                             this enum as a boundary for `{concept_id}` via \
                             `// locus: ot boundary {concept_id} <name>` then rerun `locus init`",
                            ty.name
                        )),
                    });
                    break;
                }
            }
        }
    }
    out
}

/// OT011 — shadow newtype / value object.
///
/// Fires for each single-field struct (a "newtype") whose **name** matches
/// an accepted canonical (by short name) but whose symbol isn't accepted.
/// Common shape: `pub struct UserId(pub String);` defined in two places.
///
/// Confidence 0.80 — name-match is a strong signal; the field-type check
/// keeps us off generic `Wrapper<T>`-style structs.
pub fn ot011(air: &AirWorkspace, section: &OtSection, mode: CheckMode) -> Vec<Diagnostic> {
    let mut canonical_short: BTreeMap<String, (String, String)> = BTreeMap::new(); // short → (concept, full)
    for (concept_id, entry) in &section.concepts {
        let symbol = &entry.canonical.symbol;
        if let Some(short) = symbol.rsplit("::").next() {
            canonical_short.insert(short.to_string(), (concept_id.clone(), symbol.clone()));
        }
    }
    if canonical_short.is_empty() {
        return Vec::new();
    }
    let confidence = 0.80;
    let Some(severity) = Severity::from_confidence(confidence, mode) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                let AirItem::Type(ty) = item else {
                    continue;
                };
                if ty.kind != TypeKind::Struct || ty.fields.len() != 1 {
                    continue;
                }
                if section.role_of(&ty.symbol).is_some() {
                    continue;
                }
                let Some((concept_id, canonical_symbol)) = canonical_short.get(ty.name.as_str())
                else {
                    continue;
                };
                if &ty.symbol == canonical_symbol {
                    continue; // canonical itself, just not accepted under that concept yet
                }
                out.push(Diagnostic {
                    rule_id: "OT011".to_string(),
                    severity,
                    span: ty.span.clone(),
                    concept: Some(concept_id.clone()),
                    message: format!(
                        "newtype `{}` shadows accepted canonical `{canonical_symbol}` \
                         (concept `{concept_id}`)",
                        ty.symbol
                    ),
                    why: vec![
                        format!("single-field struct named `{}`", ty.name),
                        format!("canonical for `{concept_id}`: `{canonical_symbol}`"),
                        format!("inference confidence: {confidence:.2}"),
                    ],
                    suggested_fix: Some(format!(
                        "remove `{}` and import `{canonical_symbol}` instead; if this \
                         really is a parallel boundary representation, accept it via \
                         `// locus: ot boundary {concept_id} <name>` then rerun `locus init`",
                        ty.symbol
                    )),
                });
            }
        }
    }
    out
}

/// OT012 — primitive obsession around a known canonical.
///
/// Fires for each struct field whose:
/// - name (snake_case) maps to an accepted canonical (PascalCase) by short name,
/// - type-text is a primitive (`String`, `&str`, integer, bool, …), and
/// - enclosing struct is not lockfile-accepted (i.e. not a boundary adapter).
///
/// Boundary adapters are the legitimate place for primitive-typed fields
/// because they mirror the wire shape. Application/domain types should
/// carry the canonical value object instead.
///
/// Confidence 0.70. Per the spec's agent-strict severity table this is
/// fatal under `--agent-strict` and warning otherwise.
pub fn ot012(air: &AirWorkspace, section: &OtSection, mode: CheckMode) -> Vec<Diagnostic> {
    let mut canonical_short: BTreeMap<String, String> = BTreeMap::new();
    for (concept_id, entry) in &section.concepts {
        if let Some(short) = entry.canonical.symbol.rsplit("::").next() {
            canonical_short.insert(short.to_string(), concept_id.clone());
        }
    }
    if canonical_short.is_empty() {
        return Vec::new();
    }
    let confidence = 0.70;
    let Some(severity) = Severity::from_confidence(confidence, mode) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                let AirItem::Type(ty) = item else {
                    continue;
                };
                if ty.kind != TypeKind::Struct {
                    continue;
                }
                if section.role_of(&ty.symbol).is_some() {
                    continue; // accepted boundary or canonical — primitives OK here
                }
                for field in &ty.fields {
                    let Some(canonical_short_name) = snake_to_pascal(&field.name) else {
                        continue;
                    };
                    let Some(concept_id) = canonical_short.get(&canonical_short_name) else {
                        continue;
                    };
                    if !is_primitive_type_text(&field.type_text) {
                        continue;
                    }
                    out.push(Diagnostic {
                        rule_id: "OT012".to_string(),
                        severity,
                        span: ty.span.clone(),
                        concept: Some(concept_id.clone()),
                        message: format!(
                            "field `{}::{}: {}` is a primitive substitute for canonical \
                             `{canonical_short_name}` (concept `{concept_id}`)",
                            ty.symbol, field.name, field.type_text
                        ),
                        why: vec![
                            format!(
                                "field name `{}` maps to canonical `{canonical_short_name}`",
                                field.name
                            ),
                            format!("type `{}` is a primitive", field.type_text),
                            format!("enclosing type `{}` is not an accepted boundary", ty.symbol),
                            format!("inference confidence: {confidence:.2}"),
                        ],
                        suggested_fix: Some(format!(
                            "use `{canonical_short_name}` instead of `{}` for `{}`, or \
                             accept `{}` as a boundary via `// locus: ot boundary {concept_id} \
                             <name>` if it's a wire-shape adapter",
                            field.type_text, field.name, ty.symbol
                        )),
                    });
                }
            }
        }
    }
    out
}

/// `user_id` → `UserId`; `email` → `Email`. Returns `None` if the input
/// is empty or has consecutive underscores producing empty segments —
/// either way we don't have a clean mapping to PascalCase.
fn snake_to_pascal(snake: &str) -> Option<String> {
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
fn is_primitive_type_text(text: &str) -> bool {
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

/// `(variants, kind)` for the type whose `symbol` matches `target`.
fn type_variants_and_kind(
    air: &AirWorkspace,
    target: &str,
) -> Option<(BTreeSet<String>, TypeKind)> {
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                if let AirItem::Type(ty) = item
                    && ty.symbol == target
                {
                    return Some((
                        ty.variants.iter().map(|v| v.name.clone()).collect(),
                        ty.kind,
                    ));
                }
            }
        }
    }
    None
}

fn jaccard_str(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f32 {
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

fn join_sorted(set: &BTreeSet<String>) -> String {
    set.iter().cloned().collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
#[path = "rules_tests.rs"]
mod rules_tests;
