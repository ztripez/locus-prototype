//! TA rule implementations.
//!
//! Implemented:
//! - [`ta001`]: test module defines a public domain-shaped type. Test
//!   fixtures duplicating domain concepts as public types is the "we made our
//!   own `User` struct in tests" pattern the spec calls out — domain truth
//!   should live on the canonical path, not in a test-local public clone.
//! - [`ta002`]: test type whose name overlaps an accepted canonical concept.
//!   The user lists their accepted canonical type names in
//!   `canonical_name_patterns`; any type defined inside `test_paths` whose
//!   name matches is flagged regardless of visibility.
//! - [`ta003`]: test struct whose name *and* field-name set both echo a
//!   canonical concept. Cross-checks `canonical_name_patterns` (looser
//!   "contains" match) with `canonical_field_sets` Jaccard overlap >= 0.5.
//! - [`ta004`]: a port-trait `impl` landing inside test code outside the
//!   declared `accepted_test_adapter_paths` — agent-introduced fake
//!   adapters that bypass the project's accepted test-adapter home.
//!
//! Mirrors UT001 in shape (lockfile-driven module pattern match, fires on
//! public types) but with a different fix narrative: demote to non-`pub`,
//! lift to a real production module, or accept as a shared fixture surface
//! (future TA mechanism).

use std::collections::BTreeSet;

use locus_air::{AirItem, AirWorkspace, TypeKind, Visibility};

use super::lockfile_schema::{TaSection, matches_pattern};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

/// TA001 — test module defines a public domain-shaped type.
///
/// For every `AirFile` whose `module_path` matches any pattern in
/// `test_paths`, fire one diagnostic per public `AirItem::Type`.
///
/// Severity: Warning by default; Fatal under `--agent-strict`. Test modules
/// can legitimately hold private fixture types, so the structural fail-fast
/// tier isn't a fit — a public type is the heuristic signal that domain
/// concepts are being shadowed in test code.
pub fn ta001(air: &AirWorkspace, section: &TaSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.test_paths.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            let Some(pattern) = section
                .test_paths
                .iter()
                .find(|pat| matches_pattern(pat, module_path))
            else {
                continue;
            };
            for item in &file.items {
                let AirItem::Type(ty) = item else {
                    continue;
                };
                if ty.visibility != Visibility::Public {
                    continue;
                }
                out.push(Diagnostic {
                    rule_id: "TA001".to_string(),
                    severity: mode.elevate(Severity::Warning),
                    span: ty.span.clone(),
                    concept: None,
                    message: format!(
                        "test module `{module_path}` defines public type `{}` \
                         (matched test pattern `{pattern}`)",
                        ty.name
                    ),
                    why: vec![
                        format!("module `{module_path}` matches test pattern `{pattern}`"),
                        format!(
                            "public type `{}` (`{}`, visibility `{:?}`)",
                            ty.name, ty.symbol, ty.visibility
                        ),
                        "test modules must not create new domain truth; a public type \
                         in test code is typically a shadow of a domain concept that \
                         should live on the canonical production path"
                            .into(),
                    ],
                    suggested_fix: Some(format!(
                        "demote `{}` to non-`pub` if it's only used inside this test \
                         module; or move it out of the test module if it's actually \
                         shared production code; or accept this test module as a \
                         legitimate public-fixture surface (future TA mechanism)",
                        ty.name
                    )),
                });
            }
        }
    }
    out
}

/// TA002 — test type whose name overlaps an accepted canonical concept.
///
/// For every `AirItem::Type` whose enclosing file's `module_path` matches a
/// pattern in `test_paths`, fire when the type's name matches any pattern
/// in `canonical_name_patterns`. Name match uses the same wildcard syntax
/// as the path matcher — `User`, `*User`, `User*`, `*User*` are all valid
/// — but the typical authoring shape is the bare canonical name (`User`,
/// `Email`, `Order`).
///
/// Visibility is intentionally not gated: a *private* test struct named
/// `User` is still a domain shadow worth flagging, even though TA001
/// would skip it. The two rules complement: TA001 is the public-surface
/// signal, TA002 is the named-shadow signal.
///
/// Severity: Warning by default; Fatal under `--agent-strict`.
pub fn ta002(air: &AirWorkspace, section: &TaSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.test_paths.is_empty() || section.canonical_name_patterns.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            let Some(test_pattern) = section
                .test_paths
                .iter()
                .find(|pat| matches_pattern(pat, module_path))
            else {
                continue;
            };
            for item in &file.items {
                let AirItem::Type(ty) = item else {
                    continue;
                };
                let Some(name_pattern) = section
                    .canonical_name_patterns
                    .iter()
                    .find(|pat| name_matches(pat, &ty.name))
                else {
                    continue;
                };
                out.push(Diagnostic {
                    rule_id: "TA002".to_string(),
                    severity: mode.elevate(Severity::Warning),
                    span: ty.span.clone(),
                    concept: None,
                    message: format!(
                        "test module `{module_path}` defines type `{}` whose name \
                         matches accepted canonical pattern `{name_pattern}`",
                        ty.name
                    ),
                    why: vec![
                        format!("module `{module_path}` matches test pattern `{test_pattern}`"),
                        format!(
                            "type name `{}` matches `paradigms.TA.canonical_name_patterns` \
                             entry `{name_pattern}`",
                            ty.name
                        ),
                        "test types that re-use canonical names shadow the production \
                         concept; even private duplicates drift over time and obscure \
                         where the real definition lives"
                            .into(),
                    ],
                    suggested_fix: Some(format!(
                        "rename `{}` to a test-scoped identifier (e.g. `Test{0}` or \
                         `{0}Fixture`), import the canonical type instead of redefining \
                         it, or — if this name is genuinely unrelated to the domain \
                         concept — narrow `paradigms.TA.canonical_name_patterns` so it \
                         no longer matches",
                        ty.name
                    )),
                });
            }
        }
    }
    out
}

/// TA003 — test struct whose name and field shape both echo a canonical concept.
///
/// For every `AirItem::Type` with `kind == Struct` inside `test_paths`,
/// fire when:
/// - The type's name *contains* the stripped form of any pattern in
///   `canonical_name_patterns` (looser than TA002's exact-name match — a
///   test struct called `TestUser` or `UserFixture` is a candidate).
/// - The type's field-name set has Jaccard overlap >= 0.5 with any entry
///   in `canonical_field_sets`.
///
/// Both gates must trip; either alone is too noisy. TA003 is the
/// shape-shadow signal — even renamed (TA002 wouldn't fire on `TestUser`),
/// a struct that mirrors the canonical's field set is still duplicating
/// domain truth.
///
/// Severity: Warning by default; Fatal under `--agent-strict`.
pub fn ta003(air: &AirWorkspace, section: &TaSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.test_paths.is_empty()
        || section.canonical_name_patterns.is_empty()
        || section.canonical_field_sets.is_empty()
    {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            let Some(test_pattern) = section
                .test_paths
                .iter()
                .find(|pat| matches_pattern(pat, module_path))
            else {
                continue;
            };
            for item in &file.items {
                let AirItem::Type(ty) = item else {
                    continue;
                };
                if ty.kind != TypeKind::Struct {
                    continue;
                }
                let Some(name_pattern) = section
                    .canonical_name_patterns
                    .iter()
                    .find(|pat| name_contains(pat, &ty.name))
                else {
                    continue;
                };
                let test_field_names: BTreeSet<&str> =
                    ty.fields.iter().map(|f| f.name.as_str()).collect();
                if test_field_names.is_empty() {
                    continue;
                }
                let Some((canonical_set, overlap)) =
                    best_jaccard_match(&test_field_names, &section.canonical_field_sets)
                else {
                    continue;
                };
                if overlap < 0.5 {
                    continue;
                }
                out.push(Diagnostic {
                    rule_id: "TA003".to_string(),
                    severity: mode.elevate(Severity::Warning),
                    span: ty.span.clone(),
                    concept: None,
                    message: format!(
                        "test struct `{}` in `{module_path}` shadows a canonical \
                         concept (name overlap with pattern `{name_pattern}`, field \
                         Jaccard {:.2} against canonical field set)",
                        ty.name, overlap,
                    ),
                    why: vec![
                        format!("module `{module_path}` matches test pattern `{test_pattern}`"),
                        format!(
                            "struct name `{}` contains canonical pattern `{name_pattern}`",
                            ty.name
                        ),
                        format!(
                            "field-set Jaccard overlap {:.2} >= 0.5 against canonical \
                             field set `{:?}`",
                            overlap, canonical_set
                        ),
                        "test structs that mirror canonical names *and* canonical \
                         field shapes are the spec's shape-shadow anti-pattern: \
                         agents recreate domain truth in test code rather than \
                         using the real type"
                            .into(),
                    ],
                    suggested_fix: Some(format!(
                        "import the canonical struct and construct it directly in this \
                         test, or, if this fixture is genuinely a different concept \
                         that just happens to share a few field names, rename it to \
                         break the name overlap (e.g. `{}_TestStub`)",
                        ty.name
                    )),
                });
            }
        }
    }
    out
}

/// TA004 — port impl in a test file that isn't an accepted test-adapter home.
///
/// For every `AirItem::Impl` with `Some(trait_path)`, fire when:
/// - The impl's enclosing file's `module_path` matches a `test_paths`
///   pattern.
/// - The trait path matches any pattern in `port_trait_patterns`.
/// - The file's `module_path` does NOT match any pattern in
///   `accepted_test_adapter_paths`.
///
/// Catches the "agent stitched in an in-memory `UserRepository` adapter
/// inside the test module" smell. Test adapters are legitimate, but they
/// belong on a declared adapter path (`tests::support::*`,
/// `*::test_adapters::*`), not in the same file as the production tests
/// they support — that's the path that drifts.
///
/// Severity: Warning by default; Fatal under `--agent-strict`.
pub fn ta004(air: &AirWorkspace, section: &TaSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.test_paths.is_empty() || section.port_trait_patterns.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            let Some(test_pattern) = section
                .test_paths
                .iter()
                .find(|pat| matches_pattern(pat, module_path))
            else {
                continue;
            };
            if section
                .accepted_test_adapter_paths
                .iter()
                .any(|pat| matches_pattern(pat, module_path))
            {
                continue;
            }
            for item in &file.items {
                let AirItem::Impl(imp) = item else {
                    continue;
                };
                let Some(trait_path) = imp.trait_path.as_deref() else {
                    continue;
                };
                let trait_short = trait_path.rsplit("::").next().unwrap_or(trait_path);
                let Some(port_pattern) = section.port_trait_patterns.iter().find(|pat| {
                    matches_pattern(pat, trait_path)
                        || matches_pattern(pat, trait_short)
                        || name_matches(pat, trait_path)
                        || name_matches(pat, trait_short)
                }) else {
                    continue;
                };
                out.push(Diagnostic {
                    rule_id: "TA004".to_string(),
                    severity: mode.elevate(Severity::Warning),
                    span: imp.span.clone(),
                    concept: None,
                    message: format!(
                        "port impl `impl {trait_path} for {}` in test module `{module_path}` \
                         lives outside any `paradigms.TA.accepted_test_adapter_paths`",
                        imp.self_ty,
                    ),
                    why: vec![
                        format!("module `{module_path}` matches test pattern `{test_pattern}`"),
                        format!("trait path `{trait_path}` matches port pattern `{port_pattern}`"),
                        format!(
                            "module `{module_path}` matches no \
                             `paradigms.TA.accepted_test_adapter_paths` pattern"
                        ),
                        "test adapters belong on a declared adapter path (e.g. \
                         `tests::support::*`); inline port impls inside test files \
                         drift from the production adapter contract"
                            .into(),
                    ],
                    suggested_fix: Some(format!(
                        "move `impl {trait_path} for {}` to a dedicated test-adapter \
                         module (and add that module to \
                         `paradigms.TA.accepted_test_adapter_paths` in `locus.lock`), \
                         or — if this trait isn't really a port — narrow \
                         `paradigms.TA.port_trait_patterns` so it no longer matches",
                        imp.self_ty,
                    )),
                });
            }
        }
    }
    out
}

/// Wildcard-aware name match. Reuses [`matches_pattern`] when the pattern
/// contains `::` separators (so users can write `pkg::module::User`); for
/// bare names, supports leading/trailing `*` glob.
fn name_matches(pattern: &str, name: &str) -> bool {
    if pattern.contains("::") {
        return matches_pattern(pattern, name);
    }
    let leading = pattern.starts_with('*');
    let trailing = pattern.ends_with('*') && pattern.len() > 1;
    let stripped = match (leading, trailing) {
        (true, true) => &pattern[1..pattern.len() - 1],
        (true, false) => &pattern[1..],
        (false, true) => &pattern[..pattern.len() - 1],
        (false, false) => pattern,
    };
    if stripped.is_empty() {
        return pattern == "*";
    }
    match (leading, trailing) {
        (true, true) => name.contains(stripped),
        (true, false) => name.ends_with(stripped),
        (false, true) => name.starts_with(stripped),
        (false, false) => pattern == name,
    }
}

/// Looser variant of [`name_matches`] used by TA003: strips any leading/
/// trailing `*` and tests for a substring containment. `User`, `*User`,
/// `User*`, `*User*` all collapse to "name contains `User`".
fn name_contains(pattern: &str, name: &str) -> bool {
    let trimmed = pattern.trim_matches('*');
    if trimmed.is_empty() {
        return false;
    }
    name.contains(trimmed)
}

/// Compute Jaccard overlap between `test_fields` and each entry in
/// `canonical_sets`; return the best-matching canonical set together with
/// its overlap. `None` when `canonical_sets` is empty or every entry is
/// empty.
fn best_jaccard_match<'a>(
    test_fields: &BTreeSet<&str>,
    canonical_sets: &'a [Vec<String>],
) -> Option<(&'a [String], f32)> {
    let mut best: Option<(&'a [String], f32)> = None;
    for canonical in canonical_sets {
        if canonical.is_empty() {
            continue;
        }
        let canonical_set: BTreeSet<&str> = canonical.iter().map(String::as_str).collect();
        let intersection = test_fields.intersection(&canonical_set).count() as f32;
        let union = test_fields.union(&canonical_set).count() as f32;
        if union == 0.0 {
            continue;
        }
        let jaccard = intersection / union;
        match best {
            Some((_, b)) if jaccard <= b => {}
            _ => best = Some((canonical.as_slice(), jaccard)),
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use locus_air::{AIR_SCHEMA_VERSION, AirField, AirFile, AirImpl, AirPackage, AirSpan, AirType};

    fn ty(name: &str, vis: Visibility) -> AirItem {
        AirItem::Type(AirType {
            kind: TypeKind::Struct,
            name: name.into(),
            symbol: format!("x::tests::{name}"),
            visibility: vis,
            fields: Vec::new(),
            variants: Vec::new(),
            derives: Vec::new(),
            attrs: Vec::new(),
            span: AirSpan::new("t.rs", 1, 1),
            doc: None,
        })
    }

    fn air_with_module(module: &str, items: Vec<AirItem>) -> AirWorkspace {
        AirWorkspace {
            schema_version: AIR_SCHEMA_VERSION,
            packages: vec![AirPackage {
                name: "x".into(),
                version: "0".into(),
                root_dir: "/".into(),
                files: vec![AirFile {
                    path: "t.rs".into(),
                    module_path: Some(module.into()),
                    items,
                    hints: Vec::new(),
                    parse_error: None,
                    line_count: 1,
                }],
            }],
            facts: Vec::new(),
        }
    }

    #[test]
    fn ta001_fires_on_public_type_in_test_module() {
        let air = air_with_module("x::tests", vec![ty("User", Visibility::Public)]);
        let section = TaSection {
            test_paths: vec!["x::tests::*".into()],
            ..TaSection::default()
        };
        let diags = ta001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "TA001");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(diags[0].message.contains("User"));
        assert!(diags[0].message.contains("x::tests"));
        assert!(diags[0].message.contains("x::tests::*"));
    }

    #[test]
    fn ta001_quiet_on_private_type_in_test_module() {
        let air = air_with_module("x::tests", vec![ty("Fixture", Visibility::Private)]);
        let section = TaSection {
            test_paths: vec!["x::tests::*".into()],
            ..TaSection::default()
        };
        assert!(ta001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ta001_quiet_on_public_type_in_non_matching_module() {
        let air = air_with_module("x::domain::user", vec![ty("User", Visibility::Public)]);
        let section = TaSection {
            test_paths: vec!["x::tests::*".into()],
            ..TaSection::default()
        };
        assert!(ta001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ta001_silent_when_test_paths_empty() {
        let air = air_with_module("x::tests", vec![ty("User", Visibility::Public)]);
        let section = TaSection::default();
        assert!(ta001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ta001_multiple_public_types_produce_multiple_diagnostics() {
        let air = air_with_module(
            "x::tests",
            vec![
                ty("User", Visibility::Public),
                ty("Order", Visibility::Public),
                ty("Internal", Visibility::Private), // not flagged
                ty("Account", Visibility::Public),
            ],
        );
        let section = TaSection {
            test_paths: vec!["x::tests::*".into()],
            ..TaSection::default()
        };
        let diags = ta001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 3);
        let names: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
        assert!(names.iter().any(|m| m.contains("User")));
        assert!(names.iter().any(|m| m.contains("Order")));
        assert!(names.iter().any(|m| m.contains("Account")));
        assert!(!names.iter().any(|m| m.contains("Internal")));
    }

    #[test]
    fn ta001_agent_strict_elevates_to_fatal() {
        let air = air_with_module("x::tests", vec![ty("User", Visibility::Public)]);
        let section = TaSection {
            test_paths: vec!["x::tests::*".into()],
            ..TaSection::default()
        };
        let diags = ta001(&air, &section, CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    fn struct_with_fields(name: &str, fields: &[&str]) -> AirItem {
        AirItem::Type(AirType {
            kind: TypeKind::Struct,
            name: name.into(),
            symbol: format!("x::tests::{name}"),
            visibility: Visibility::Private,
            fields: fields
                .iter()
                .map(|n| AirField {
                    name: (*n).into(),
                    type_text: "()".into(),
                    visibility: Visibility::Private,
                })
                .collect(),
            variants: Vec::new(),
            derives: Vec::new(),
            attrs: Vec::new(),
            span: AirSpan::new("t.rs", 1, 1),
            doc: None,
        })
    }

    fn impl_item(trait_path: Option<&str>, self_ty: &str) -> AirItem {
        AirItem::Impl(AirImpl {
            trait_path: trait_path.map(|s| s.to_string()),
            self_ty: self_ty.into(),
            method_names: Vec::new(),
            span: AirSpan::new("t.rs", 1, 1),
        })
    }

    // ─── TA002 ───────────────────────────────────────────────────────────

    #[test]
    fn ta002_fires_on_test_type_with_canonical_name() {
        let air = air_with_module(
            "x::tests::user",
            vec![
                ty("User", Visibility::Private),
                ty("Helper", Visibility::Private),
            ],
        );
        let section = TaSection {
            test_paths: vec!["x::tests::*".into()],
            canonical_name_patterns: vec!["User".into()],
            ..TaSection::default()
        };
        let diags = ta002(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "TA002");
        assert!(diags[0].message.contains("User"));
        assert!(diags[0].message.contains("x::tests::user"));
    }

    #[test]
    fn ta002_silent_when_canonical_name_patterns_empty() {
        let air = air_with_module("x::tests", vec![ty("User", Visibility::Public)]);
        let section = TaSection {
            test_paths: vec!["x::tests::*".into()],
            ..TaSection::default()
        };
        assert!(ta002(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ta002_quiet_outside_test_paths() {
        let air = air_with_module("x::domain::user", vec![ty("User", Visibility::Public)]);
        let section = TaSection {
            test_paths: vec!["x::tests::*".into()],
            canonical_name_patterns: vec!["User".into()],
            ..TaSection::default()
        };
        assert!(ta002(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ta002_wildcard_name_pattern_matches() {
        let air = air_with_module(
            "x::tests",
            vec![
                ty("OrderDto", Visibility::Private),
                ty("Misc", Visibility::Private),
            ],
        );
        let section = TaSection {
            test_paths: vec!["x::tests::*".into()],
            canonical_name_patterns: vec!["Order*".into()],
            ..TaSection::default()
        };
        let diags = ta002(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("OrderDto"));
    }

    #[test]
    fn ta002_agent_strict_elevates_to_fatal() {
        let air = air_with_module("x::tests", vec![ty("User", Visibility::Private)]);
        let section = TaSection {
            test_paths: vec!["x::tests::*".into()],
            canonical_name_patterns: vec!["User".into()],
            ..TaSection::default()
        };
        let diags = ta002(&air, &section, CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    // ─── TA003 ───────────────────────────────────────────────────────────

    #[test]
    fn ta003_fires_on_shape_shadow() {
        // TestUser carries the canonical User's field set verbatim.
        let air = air_with_module(
            "x::tests",
            vec![struct_with_fields("TestUser", &["id", "email", "name"])],
        );
        let section = TaSection {
            test_paths: vec!["x::tests::*".into()],
            canonical_name_patterns: vec!["User".into()],
            canonical_field_sets: vec![vec!["id".into(), "email".into(), "name".into()]],
            ..TaSection::default()
        };
        let diags = ta003(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "TA003");
        assert!(diags[0].message.contains("TestUser"));
    }

    #[test]
    fn ta003_quiet_when_field_overlap_below_threshold() {
        // Only 1 field shared out of a union of 5 → Jaccard 0.2 < 0.5.
        let air = air_with_module(
            "x::tests",
            vec![struct_with_fields(
                "UserFixture",
                &["id", "tag", "score", "color"],
            )],
        );
        let section = TaSection {
            test_paths: vec!["x::tests::*".into()],
            canonical_name_patterns: vec!["User".into()],
            canonical_field_sets: vec![vec!["id".into(), "email".into()]],
            ..TaSection::default()
        };
        assert!(ta003(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ta003_quiet_when_name_does_not_overlap() {
        // Field set matches canonical, but type name doesn't echo the
        // canonical concept — TA003 needs both gates.
        let air = air_with_module(
            "x::tests",
            vec![struct_with_fields("Widget", &["id", "email", "name"])],
        );
        let section = TaSection {
            test_paths: vec!["x::tests::*".into()],
            canonical_name_patterns: vec!["User".into()],
            canonical_field_sets: vec![vec!["id".into(), "email".into(), "name".into()]],
            ..TaSection::default()
        };
        assert!(ta003(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ta003_silent_when_canonical_field_sets_empty() {
        let air = air_with_module(
            "x::tests",
            vec![struct_with_fields("TestUser", &["id", "email", "name"])],
        );
        let section = TaSection {
            test_paths: vec!["x::tests::*".into()],
            canonical_name_patterns: vec!["User".into()],
            ..TaSection::default()
        };
        assert!(ta003(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ta003_silent_when_test_paths_empty() {
        let air = air_with_module(
            "x::tests",
            vec![struct_with_fields("TestUser", &["id", "email", "name"])],
        );
        let section = TaSection {
            canonical_name_patterns: vec!["User".into()],
            canonical_field_sets: vec![vec!["id".into(), "email".into()]],
            ..TaSection::default()
        };
        assert!(ta003(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ta003_agent_strict_elevates_to_fatal() {
        let air = air_with_module(
            "x::tests",
            vec![struct_with_fields("TestUser", &["id", "email", "name"])],
        );
        let section = TaSection {
            test_paths: vec!["x::tests::*".into()],
            canonical_name_patterns: vec!["User".into()],
            canonical_field_sets: vec![vec!["id".into(), "email".into(), "name".into()]],
            ..TaSection::default()
        };
        let diags = ta003(&air, &section, CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    // ─── TA004 ───────────────────────────────────────────────────────────

    #[test]
    fn ta004_fires_on_port_impl_in_test_module() {
        let air = air_with_module(
            "x::tests::auth",
            vec![impl_item(Some("x::ports::UserRepository"), "FakeRepo")],
        );
        let section = TaSection {
            test_paths: vec!["x::tests::*".into()],
            port_trait_patterns: vec!["*Repository".into()],
            ..TaSection::default()
        };
        let diags = ta004(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "TA004");
        assert!(diags[0].message.contains("UserRepository"));
        assert!(diags[0].message.contains("FakeRepo"));
    }

    #[test]
    fn ta004_quiet_in_accepted_test_adapter_path() {
        let air = air_with_module(
            "x::tests::support::repos",
            vec![impl_item(Some("x::ports::UserRepository"), "InMemoryRepo")],
        );
        let section = TaSection {
            test_paths: vec!["x::tests::*".into()],
            port_trait_patterns: vec!["*Repository".into()],
            accepted_test_adapter_paths: vec!["x::tests::support::*".into()],
            ..TaSection::default()
        };
        assert!(ta004(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ta004_quiet_for_inherent_impl() {
        // No trait_path → not a port impl, never flagged.
        let air = air_with_module("x::tests", vec![impl_item(None, "FakeRepo")]);
        let section = TaSection {
            test_paths: vec!["x::tests::*".into()],
            port_trait_patterns: vec!["*Repository".into()],
            ..TaSection::default()
        };
        assert!(ta004(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ta004_silent_when_port_trait_patterns_empty() {
        let air = air_with_module(
            "x::tests",
            vec![impl_item(Some("x::ports::UserRepository"), "FakeRepo")],
        );
        let section = TaSection {
            test_paths: vec!["x::tests::*".into()],
            ..TaSection::default()
        };
        assert!(ta004(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ta004_quiet_outside_test_paths() {
        let air = air_with_module(
            "x::infrastructure::repos",
            vec![impl_item(Some("x::ports::UserRepository"), "PgRepo")],
        );
        let section = TaSection {
            test_paths: vec!["x::tests::*".into()],
            port_trait_patterns: vec!["*Repository".into()],
            ..TaSection::default()
        };
        assert!(ta004(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ta004_agent_strict_elevates_to_fatal() {
        let air = air_with_module(
            "x::tests",
            vec![impl_item(Some("x::ports::UserGateway"), "FakeGw")],
        );
        let section = TaSection {
            test_paths: vec!["x::tests::*".into()],
            port_trait_patterns: vec!["*Gateway".into()],
            ..TaSection::default()
        };
        let diags = ta004(&air, &section, CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }
}
