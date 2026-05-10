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
        confidence: 0.0,
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
        confidence: 0.0,
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
        confidence: 0.0,
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
        confidence: 0.0,
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
        confidence: 0.0,
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
        confidence: 0.0,
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
        confidence: 0.0,
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
        confidence: 0.0,
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
                    mechanism: ConversionMechanism::FallibleAdapter,
                    symbol: symbol.into(),
                    span: AirSpan::new("t.rs", 1, 1),
                })],
                hints: Vec::new(),
                parse_error: None,
                line_count: 1,
            }],
        }],
        facts: Vec::new(),
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
    OtSection {
        concepts,
        ..Default::default()
    }
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

// ---- type_text_references helper ----

#[test]
fn type_text_references_matches_whole_identifier() {
    assert!(type_text_references("UserDto", "UserDto"));
    assert!(type_text_references("Result<UserDto, Error>", "UserDto"));
    assert!(type_text_references("&UserDto", "UserDto"));
    assert!(type_text_references("Vec<&'a UserDto>", "UserDto"));
}

#[test]
fn type_text_references_rejects_substrings() {
    assert!(!type_text_references("UserDtoVec", "UserDto"));
    assert!(!type_text_references("MyUserDto", "UserDto"));
    assert!(!type_text_references("user_dto", "UserDto")); // case-sensitive
}

// ---- OT003 ----

use locus_air::{AirField, AirFunction, AirType, TypeKind, Visibility};

fn ty_in_file(symbol: &str, name: &str, file_path: &str) -> AirItem {
    AirItem::Type(AirType {
        kind: TypeKind::Struct,
        name: name.into(),
        symbol: symbol.into(),
        visibility: Visibility::Public,
        fields: vec![AirField {
            name: "x".into(),
            type_text: "String".into(),
            visibility: Visibility::Public,
        }],
        variants: Vec::new(),
        decorators: Vec::new(),
        symbol_segments: Vec::new(),
        span: AirSpan::new(file_path, 1, 1),
        doc: None,
    })
}

fn fn_in_file(
    symbol: &str,
    params: Vec<(&str, &str)>,
    ret: Option<&str>,
    file_path: &str,
) -> AirItem {
    AirItem::Function(AirFunction {
        name: symbol.rsplit("::").next().unwrap_or(symbol).into(),
        symbol: symbol.into(),
        visibility: Visibility::Public,
        params: params
            .into_iter()
            .map(|(n, t)| (n.to_string(), t.to_string()))
            .collect(),
        return_type: ret.map(|s| s.to_string()),
        span: AirSpan::new(file_path, 1, 1),
        line_count: 1,
        decorators: Vec::new(),
        symbol_segments: Vec::new(),
        doc: None,
    })
}

fn air_with_files(files: Vec<(&str, Vec<AirItem>)>) -> AirWorkspace {
    AirWorkspace {
        schema_version: AIR_SCHEMA_VERSION,
        packages: vec![AirPackage {
            name: "x".into(),
            version: "0".into(),
            root_dir: "/".into(),
            files: files
                .into_iter()
                .map(|(path, items)| AirFile {
                    path: path.into(),
                    module_path: Some("crate".into()),
                    items,
                    hints: Vec::new(),
                    parse_error: None,
                    line_count: 1,
                })
                .collect(),
        }],
        facts: Vec::new(),
    }
}

fn section_with_canonical_and_boundary(
    canonical_symbol: &str,
    boundary_symbol: &str,
    accepted_converters: &[&str],
) -> OtSection {
    use super::super::lockfile_schema::{
        AcceptedBoundary, AcceptedCanonical, AcceptedConverter, ConceptEntry, Source,
    };
    let mut concepts = BTreeMap::new();
    concepts.insert(
        "user".to_string(),
        ConceptEntry {
            canonical: AcceptedCanonical {
                symbol: canonical_symbol.into(),
                source: Source::Hint,
            },
            boundaries: vec![AcceptedBoundary {
                symbol: boundary_symbol.into(),
                boundary: Some("api.v1".into()),
                source: Source::Hint,
            }],
            converters: accepted_converters
                .iter()
                .map(|s| AcceptedConverter {
                    from: "UserDto".into(),
                    to: "User".into(),
                    symbol: (*s).to_string(),
                    source: Source::Init,
                })
                .collect(),
        },
    );
    OtSection {
        concepts,
        ..Default::default()
    }
}

#[test]
fn ot003_fires_on_boundary_param_in_non_boundary_file() {
    let air = air_with_files(vec![
        (
            "src/dto.rs",
            vec![ty_in_file("crate::dto::UserDto", "UserDto", "src/dto.rs")],
        ),
        (
            "src/handler.rs",
            vec![fn_in_file(
                "crate::handler::create_user",
                vec![("req", "UserDto")],
                Some("User"),
                "src/handler.rs",
            )],
        ),
    ]);
    let section =
        section_with_canonical_and_boundary("crate::identity::User", "crate::dto::UserDto", &[]);
    let diags = ot003(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "OT003");
    assert_eq!(diags[0].severity, Severity::Fatal);
    assert!(diags[0].message.contains("UserDto"));
    assert!(diags[0].message.contains("create_user"));
}

#[test]
fn ot003_quiet_when_function_lives_in_boundary_file() {
    let air = air_with_files(vec![(
        "src/dto.rs",
        vec![
            ty_in_file("crate::dto::UserDto", "UserDto", "src/dto.rs"),
            fn_in_file(
                "crate::dto::handle",
                vec![("req", "UserDto")],
                Some("User"),
                "src/dto.rs",
            ),
        ],
    )]);
    let section =
        section_with_canonical_and_boundary("crate::identity::User", "crate::dto::UserDto", &[]);
    assert!(ot003(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ot003_quiet_for_accepted_converter_even_in_domain_file() {
    let converter_sym = "crate::handler::map_user";
    let air = air_with_files(vec![
        (
            "src/dto.rs",
            vec![ty_in_file("crate::dto::UserDto", "UserDto", "src/dto.rs")],
        ),
        (
            "src/handler.rs",
            vec![fn_in_file(
                converter_sym,
                vec![("req", "UserDto")],
                Some("User"),
                "src/handler.rs",
            )],
        ),
    ]);
    let section = section_with_canonical_and_boundary(
        "crate::identity::User",
        "crate::dto::UserDto",
        &[converter_sym],
    );
    assert!(ot003(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ot003_silent_when_no_boundaries_accepted() {
    let air = air_with_files(vec![(
        "src/handler.rs",
        vec![fn_in_file(
            "crate::handler::create_user",
            vec![("req", "UserDto")],
            Some("User"),
            "src/handler.rs",
        )],
    )]);
    // Section has a canonical but no boundaries → nothing to leak.
    use super::super::lockfile_schema::{AcceptedCanonical, ConceptEntry, Source};
    let mut concepts = BTreeMap::new();
    concepts.insert(
        "user".to_string(),
        ConceptEntry {
            canonical: AcceptedCanonical {
                symbol: "crate::identity::User".into(),
                source: Source::Hint,
            },
            boundaries: Vec::new(),
            converters: Vec::new(),
        },
    );
    let section = OtSection {
        concepts,
        ..Default::default()
    };
    assert!(ot003(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ot003_emits_one_diag_per_boundary_per_function() {
    // Function references the same boundary twice (param + return);
    // should still produce a single OT003.
    let air = air_with_files(vec![
        (
            "src/dto.rs",
            vec![ty_in_file("crate::dto::UserDto", "UserDto", "src/dto.rs")],
        ),
        (
            "src/handler.rs",
            vec![fn_in_file(
                "crate::handler::echo",
                vec![("req", "UserDto")],
                Some("UserDto"),
                "src/handler.rs",
            )],
        ),
    ]);
    let section =
        section_with_canonical_and_boundary("crate::identity::User", "crate::dto::UserDto", &[]);
    let diags = ot003(&air, &section, CheckMode::Human);
    assert_eq!(
        diags.len(),
        1,
        "expected exactly one OT003 per (fn, boundary)"
    );
}

// ---- OT004 ----

use locus_air::AirTruthAction;

fn construct_action(target: &str, function: &str, file_path: &str) -> AirItem {
    AirItem::TruthAction(AirTruthAction {
        action: ActionKind::Construct,
        target: target.into(),
        function: Some(function.into()),
        span: AirSpan::new(file_path, 10, 10),
        confidence: 0.95,
        reasons: vec!["struct literal in function body".into()],
    })
}

#[test]
fn ot004_fires_on_canonical_construction_outside_owner_and_converter() {
    let air = air_with_files(vec![
        (
            "src/identity.rs",
            vec![ty_in_file(
                "crate::identity::User",
                "User",
                "src/identity.rs",
            )],
        ),
        (
            "src/handler.rs",
            vec![construct_action(
                "User",
                "crate::handler::create_user",
                "src/handler.rs",
            )],
        ),
    ]);
    let section =
        section_with_canonical_and_boundary("crate::identity::User", "crate::dto::UserDto", &[]);
    let diags = ot004(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "OT004");
    assert_eq!(diags[0].severity, Severity::Fatal);
    assert!(diags[0].message.contains("crate::identity::User"));
}

#[test]
fn ot004_quiet_in_owner_file() {
    let air = air_with_files(vec![(
        "src/identity.rs",
        vec![
            ty_in_file("crate::identity::User", "User", "src/identity.rs"),
            construct_action("User", "crate::identity::User::create", "src/identity.rs"),
        ],
    )]);
    let section =
        section_with_canonical_and_boundary("crate::identity::User", "crate::dto::UserDto", &[]);
    assert!(ot004(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ot004_quiet_inside_accepted_converter() {
    let converter_sym = "crate::dto::map_user";
    let air = air_with_files(vec![
        (
            "src/identity.rs",
            vec![ty_in_file(
                "crate::identity::User",
                "User",
                "src/identity.rs",
            )],
        ),
        (
            "src/dto.rs",
            vec![construct_action("User", converter_sym, "src/dto.rs")],
        ),
    ]);
    let section = section_with_canonical_and_boundary(
        "crate::identity::User",
        "crate::dto::UserDto",
        &[converter_sym],
    );
    assert!(ot004(&air, &section, CheckMode::Human).is_empty());
}

/// Crate-level adapter authority: a `converter_paths` pattern that
/// covers an entire adapter crate (`adapter_crate::*`) silences OT004
/// for every constructor symbol inside that crate, with no per-function
/// annotation. This is the documented mechanism for adapter authority
/// per `docs/superpowers/specs/2026-05-09-ot-adapter-authority.md`
/// (issue #31). Locus uses the pattern `locus_rust::*` to authorise
/// the entire AIR adapter crate.
#[test]
fn ot004_quiet_for_crate_level_converter_path() {
    // Canonical lives in `canonical_crate`; constructions happen across
    // multiple modules of `adapter_crate`. A single crate-level pattern
    // should cover all of them.
    let air = air_with_files(vec![
        (
            "canonical_crate/src/identity.rs",
            vec![ty_in_file(
                "canonical_crate::identity::User",
                "User",
                "canonical_crate/src/identity.rs",
            )],
        ),
        (
            "adapter_crate/src/visitor.rs",
            vec![construct_action(
                "User",
                "adapter_crate::visitor::collect_user",
                "adapter_crate/src/visitor.rs",
            )],
        ),
        (
            "adapter_crate/src/loaders.rs",
            vec![construct_action(
                "User",
                "adapter_crate::loaders::std_rt::build_user",
                "adapter_crate/src/loaders.rs",
            )],
        ),
    ]);
    let mut section = section_with_canonical_and_boundary(
        "canonical_crate::identity::User",
        "canonical_crate::dto::UserDto",
        &[],
    );
    // One pattern covers the whole adapter crate.
    section.converter_paths.push("adapter_crate::*".into());

    let diags = ot004(&air, &section, CheckMode::Human);
    assert!(
        diags.is_empty(),
        "crate-level converter_paths pattern `adapter_crate::*` must silence \
         every OT004 inside the adapter crate; got {diags:#?}",
    );
}

/// Companion to `ot004_quiet_for_crate_level_converter_path`: a
/// crate-level pattern only authorises constructions *inside that
/// crate*. A construction outside the declared adapter crate still
/// fires OT004 — the pattern is a scoped grant, not a global silence.
#[test]
fn ot004_fires_outside_crate_level_converter_path() {
    let air = air_with_files(vec![
        (
            "canonical_crate/src/identity.rs",
            vec![ty_in_file(
                "canonical_crate::identity::User",
                "User",
                "canonical_crate/src/identity.rs",
            )],
        ),
        (
            "other_crate/src/sneaky.rs",
            vec![construct_action(
                "User",
                "other_crate::sneaky::build_user",
                "other_crate/src/sneaky.rs",
            )],
        ),
    ]);
    let mut section = section_with_canonical_and_boundary(
        "canonical_crate::identity::User",
        "canonical_crate::dto::UserDto",
        &[],
    );
    section.converter_paths.push("adapter_crate::*".into());

    let diags = ot004(&air, &section, CheckMode::Human);
    assert_eq!(
        diags.len(),
        1,
        "construction in `other_crate` is outside the `adapter_crate::*` grant \
         and must still fire OT004; got {diags:#?}",
    );
    assert!(diags[0].message.contains("User"));
}

#[test]
fn ot004_quiet_for_converter_path_authority() {
    let air = air_with_files(vec![
        (
            "src/identity.rs",
            vec![ty_in_file(
                "crate::identity::User",
                "User",
                "src/identity.rs",
            )],
        ),
        (
            "src/adapter.rs",
            vec![construct_action(
                "User",
                "crate::adapter::build_user",
                "src/adapter.rs",
            )],
        ),
    ]);
    let mut section =
        section_with_canonical_and_boundary("crate::identity::User", "crate::dto::UserDto", &[]);
    section.converter_paths.push("crate::adapter::*".into());
    assert!(ot004(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ot004_silent_when_no_canonicals_accepted() {
    let air = air_with_files(vec![(
        "src/handler.rs",
        vec![construct_action(
            "User",
            "crate::handler::create_user",
            "src/handler.rs",
        )],
    )]);
    let section = OtSection::default();
    assert!(ot004(&air, &section, CheckMode::Human).is_empty());
}

// ---- OT005 ----

#[test]
fn ot005_fires_when_boundary_has_no_converter() {
    let air = air_with_files(vec![
        (
            "src/identity.rs",
            vec![ty_in_file(
                "crate::identity::User",
                "User",
                "src/identity.rs",
            )],
        ),
        (
            "src/dto.rs",
            vec![ty_in_file("crate::dto::UserDto", "UserDto", "src/dto.rs")],
        ),
    ]);
    let section = section_with_canonical_and_boundary(
        "crate::identity::User",
        "crate::dto::UserDto",
        &[], // no converters accepted
    );
    let diags = ot005(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "OT005");
    assert_eq!(diags[0].severity, Severity::Fatal);
    assert!(diags[0].message.contains("UserDto"));
    assert!(
        diags[0].span.file.ends_with("src/dto.rs"),
        "should pin to the boundary's defining file; got {}",
        diags[0].span.file
    );
}

#[test]
fn ot005_quiet_when_a_converter_mentions_the_boundary() {
    let air = air_with_files(vec![
        (
            "src/identity.rs",
            vec![ty_in_file(
                "crate::identity::User",
                "User",
                "src/identity.rs",
            )],
        ),
        (
            "src/dto.rs",
            vec![ty_in_file("crate::dto::UserDto", "UserDto", "src/dto.rs")],
        ),
    ]);
    let section = section_with_canonical_and_boundary(
        "crate::identity::User",
        "crate::dto::UserDto",
        &["crate::dto::impl TryFrom<UserDto> for User"],
    );
    assert!(ot005(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ot005_silent_when_no_boundaries_accepted() {
    let air = air_with_files(vec![(
        "src/identity.rs",
        vec![ty_in_file(
            "crate::identity::User",
            "User",
            "src/identity.rs",
        )],
    )]);
    use super::super::lockfile_schema::{AcceptedCanonical, ConceptEntry, Source};
    let mut concepts = BTreeMap::new();
    concepts.insert(
        "user".into(),
        ConceptEntry {
            canonical: AcceptedCanonical {
                symbol: "crate::identity::User".into(),
                source: Source::Hint,
            },
            boundaries: Vec::new(),
            converters: Vec::new(),
        },
    );
    let section = OtSection {
        concepts,
        ..Default::default()
    };
    assert!(ot005(&air, &section, CheckMode::Human).is_empty());
}

// ---- OT007 ----

fn section_with_two_boundaries(canonical: &str, b1: (&str, &str), b2: (&str, &str)) -> OtSection {
    use super::super::lockfile_schema::{
        AcceptedBoundary, AcceptedCanonical, ConceptEntry, Source,
    };
    let mut concepts = BTreeMap::new();
    concepts.insert(
        "user".into(),
        ConceptEntry {
            canonical: AcceptedCanonical {
                symbol: canonical.into(),
                source: Source::Hint,
            },
            boundaries: vec![
                AcceptedBoundary {
                    symbol: b1.0.into(),
                    boundary: Some(b1.1.into()),
                    source: Source::Hint,
                },
                AcceptedBoundary {
                    symbol: b2.0.into(),
                    boundary: Some(b2.1.into()),
                    source: Source::Hint,
                },
            ],
            converters: Vec::new(),
        },
    );
    OtSection {
        concepts,
        ..Default::default()
    }
}

fn conversion_in_file(symbol: &str, from: &str, to: &str, file_path: &str, line: u32) -> AirItem {
    AirItem::Conversion(AirConversion {
        from: from.into(),
        to: to.into(),
        mechanism: ConversionMechanism::InfallibleAdapter,
        symbol: symbol.into(),
        span: AirSpan::new(file_path, line, line),
    })
}

#[test]
fn ot007_fires_on_adapter_to_adapter() {
    let air = air_with_files(vec![(
        "src/api/v1.rs",
        vec![conversion_in_file(
            "crate::api::v1::impl From<UserDtoV1> for UserDtoV2",
            "UserDtoV1",
            "UserDtoV2",
            "src/api/v1.rs",
            10,
        )],
    )]);
    let section = section_with_two_boundaries(
        "crate::identity::User",
        ("crate::api::v1::UserDtoV1", "api.v1"),
        ("crate::api::v2::UserDtoV2", "api.v2"),
    );
    let diags = ot007(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "OT007");
    assert_eq!(diags[0].severity, Severity::Fatal);
    assert!(
        diags[0].message.contains("UserDtoV1") && diags[0].message.contains("UserDtoV2"),
        "message: {}",
        diags[0].message
    );
}

#[test]
fn ot007_quiet_with_protocol_translation_hint() {
    use locus_air::AirHint;
    let air = AirWorkspace {
        schema_version: AIR_SCHEMA_VERSION,
        packages: vec![AirPackage {
            name: "x".into(),
            version: "0".into(),
            root_dir: "/".into(),
            files: vec![AirFile {
                path: "src/api/v1.rs".into(),
                module_path: Some("crate".into()),
                items: vec![conversion_in_file(
                    "crate::api::v1::translate",
                    "UserDtoV1",
                    "UserDtoV2",
                    "src/api/v1.rs",
                    10,
                )],
                hints: vec![AirHint {
                    kind: HintKind::ProtocolTranslation {
                        reason: Some("compatibility endpoint".into()),
                    },
                    raw: "// locus: ot protocol-translation reason=\"compatibility endpoint\""
                        .into(),
                    span: AirSpan::new("src/api/v1.rs", 9, 9),
                    target_span: Some(AirSpan::new("src/api/v1.rs", 10, 10)),
                }],
                parse_error: None,
                line_count: 1,
            }],
        }],
        facts: Vec::new(),
    };
    let section = section_with_two_boundaries(
        "crate::identity::User",
        ("crate::api::v1::UserDtoV1", "api.v1"),
        ("crate::api::v2::UserDtoV2", "api.v2"),
    );
    assert!(
        ot007(&air, &section, CheckMode::Human).is_empty(),
        "protocol-translation hint should suppress OT007"
    );
}

#[test]
fn ot007_silent_when_no_boundaries_accepted() {
    let air = air_with_files(vec![(
        "src/api/v1.rs",
        vec![conversion_in_file(
            "crate::api::v1::translate",
            "Foo",
            "Bar",
            "src/api/v1.rs",
            10,
        )],
    )]);
    let section = OtSection::default();
    assert!(ot007(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ot007_quiet_when_only_one_endpoint_is_a_boundary() {
    // boundary → canonical isn't OT007 — that's the expected path.
    let air = air_with_files(vec![(
        "src/dto.rs",
        vec![conversion_in_file(
            "crate::dto::impl TryFrom<UserDto> for User",
            "UserDto",
            "User",
            "src/dto.rs",
            10,
        )],
    )]);
    let section =
        section_with_canonical_and_boundary("crate::identity::User", "crate::dto::UserDto", &[]);
    assert!(ot007(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ot004_matches_path_qualified_target() {
    // Constructions like `crate::identity::User { ... }` appear in AIR
    // with the full path as `target`. Should still match.
    let air = air_with_files(vec![
        (
            "src/identity.rs",
            vec![ty_in_file(
                "crate::identity::User",
                "User",
                "src/identity.rs",
            )],
        ),
        (
            "src/handler.rs",
            vec![construct_action(
                "crate::identity::User",
                "crate::handler::create_user",
                "src/handler.rs",
            )],
        ),
    ]);
    let section =
        section_with_canonical_and_boundary("crate::identity::User", "crate::dto::UserDto", &[]);
    let diags = ot004(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
}

// ---- OT008 / OT009 / OT010 / OT011 / OT012 helpers ----

fn impl_in_file(
    target_type: &str,
    interface: Option<&str>,
    method_names: &[&str],
    file_path: &str,
) -> AirItem {
    AirItem::Impl(locus_air::AirImplBlock {
        interface: interface.map(|s| s.to_string()),
        target_type: target_type.into(),
        method_names: method_names.iter().map(|s| s.to_string()).collect(),
        dispatch: locus_air::ImplDispatch::Static,
        span: AirSpan::new(file_path, 1, 1),
    })
}

fn enum_in_file(symbol: &str, name: &str, variants: &[&str], file_path: &str) -> AirItem {
    use locus_air::AirVariant;
    AirItem::Type(AirType {
        kind: TypeKind::Enum,
        name: name.into(),
        symbol: symbol.into(),
        visibility: Visibility::Public,
        fields: Vec::new(),
        variants: variants
            .iter()
            .map(|v| AirVariant {
                name: (*v).to_string(),
                fields: Vec::new(),
            })
            .collect(),
        decorators: Vec::new(),
        symbol_segments: Vec::new(),
        span: AirSpan::new(file_path, 1, 1),
        doc: None,
    })
}

fn struct_with_fields(
    symbol: &str,
    name: &str,
    fields: &[(&str, &str)],
    file_path: &str,
) -> AirItem {
    AirItem::Type(AirType {
        kind: TypeKind::Struct,
        name: name.into(),
        symbol: symbol.into(),
        visibility: Visibility::Public,
        fields: fields
            .iter()
            .map(|(n, t)| AirField {
                name: (*n).to_string(),
                type_text: (*t).to_string(),
                visibility: Visibility::Public,
            })
            .collect(),
        variants: Vec::new(),
        decorators: Vec::new(),
        symbol_segments: Vec::new(),
        span: AirSpan::new(file_path, 1, 1),
        doc: None,
    })
}

// ---- OT008 ----

#[test]
fn ot008_fires_on_domain_method_on_boundary_inherent_impl() {
    let air = air_with_files(vec![(
        "src/dto.rs",
        vec![impl_in_file(
            "UserDto",
            None,
            &["from", "is_active"], // `is_active` is the smoking gun
            "src/dto.rs",
        )],
    )]);
    let section =
        section_with_canonical_and_boundary("crate::identity::User", "crate::dto::UserDto", &[]);
    let diags = ot008(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1, "expected one diagnostic; got {diags:?}");
    assert_eq!(diags[0].rule_id, "OT008");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(diags[0].message.contains("is_active"));
}

#[test]
fn ot008_quiet_on_pure_translation_impls() {
    let air = air_with_files(vec![(
        "src/dto.rs",
        vec![impl_in_file(
            "UserDto",
            None,
            &["from", "try_from", "as_str", "to_string"],
            "src/dto.rs",
        )],
    )]);
    let section =
        section_with_canonical_and_boundary("crate::identity::User", "crate::dto::UserDto", &[]);
    assert!(ot008(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ot008_quiet_on_trait_impls() {
    let air = air_with_files(vec![(
        "src/dto.rs",
        vec![impl_in_file(
            "UserDto",
            Some("std::fmt::Display"), // trait impl
            &["fmt", "weird_method"],
            "src/dto.rs",
        )],
    )]);
    let section =
        section_with_canonical_and_boundary("crate::identity::User", "crate::dto::UserDto", &[]);
    assert!(ot008(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ot008_silent_when_no_boundaries_accepted() {
    let air = air_with_files(vec![(
        "src/dto.rs",
        vec![impl_in_file("UserDto", None, &["is_active"], "src/dto.rs")],
    )]);
    let section = OtSection::default();
    assert!(ot008(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ot008_agent_strict_elevates_to_fatal() {
    let air = air_with_files(vec![(
        "src/dto.rs",
        vec![impl_in_file("UserDto", None, &["is_active"], "src/dto.rs")],
    )]);
    let section =
        section_with_canonical_and_boundary("crate::identity::User", "crate::dto::UserDto", &[]);
    let diags = ot008(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

// ---- OT009 ----

#[test]
fn ot009_fires_on_validate_function_outside_owner_module() {
    let air = air_with_files(vec![
        (
            "src/identity.rs",
            vec![ty_in_file(
                "crate::identity::User",
                "User",
                "src/identity.rs",
            )],
        ),
        (
            "src/handler.rs",
            vec![fn_in_file(
                "crate::handler::validate_user",
                vec![("u", "User")],
                Some("Result<User, ()>"),
                "src/handler.rs",
            )],
        ),
    ]);
    let section =
        section_with_canonical_and_boundary("crate::identity::User", "crate::dto::UserDto", &[]);
    let diags = ot009(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1, "expected OT009 to fire; got {diags:?}");
    assert_eq!(diags[0].rule_id, "OT009");
    assert_eq!(diags[0].severity, Severity::Warning);
}

#[test]
fn ot009_quiet_when_validator_lives_in_owner_module() {
    let air = air_with_files(vec![(
        "src/identity.rs",
        vec![
            ty_in_file("crate::identity::User", "User", "src/identity.rs"),
            fn_in_file(
                "crate::identity::validate_user",
                vec![("u", "User")],
                Some("Result<User, ()>"),
                "src/identity.rs",
            ),
        ],
    )]);
    let section =
        section_with_canonical_and_boundary("crate::identity::User", "crate::dto::UserDto", &[]);
    assert!(ot009(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ot009_quiet_when_validator_is_accepted_converter() {
    let air = air_with_files(vec![
        (
            "src/identity.rs",
            vec![ty_in_file(
                "crate::identity::User",
                "User",
                "src/identity.rs",
            )],
        ),
        (
            "src/api.rs",
            vec![fn_in_file(
                "crate::api::validate_user",
                vec![("u", "User")],
                Some("Result<User, ()>"),
                "src/api.rs",
            )],
        ),
    ]);
    let section = section_with_canonical_and_boundary(
        "crate::identity::User",
        "crate::dto::UserDto",
        &["crate::api::validate_user"],
    );
    assert!(ot009(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ot009_quiet_for_validators_not_referencing_a_canonical() {
    // A `validate_input(s: &str) -> bool` doesn't touch any canonical;
    // OT009 should not flag it.
    let air = air_with_files(vec![
        (
            "src/identity.rs",
            vec![ty_in_file(
                "crate::identity::User",
                "User",
                "src/identity.rs",
            )],
        ),
        (
            "src/handler.rs",
            vec![fn_in_file(
                "crate::handler::validate_input",
                vec![("s", "&str")],
                Some("bool"),
                "src/handler.rs",
            )],
        ),
    ]);
    let section =
        section_with_canonical_and_boundary("crate::identity::User", "crate::dto::UserDto", &[]);
    assert!(ot009(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ot009_agent_strict_elevates_to_fatal() {
    let air = air_with_files(vec![
        (
            "src/identity.rs",
            vec![ty_in_file(
                "crate::identity::User",
                "User",
                "src/identity.rs",
            )],
        ),
        (
            "src/handler.rs",
            vec![fn_in_file(
                "crate::handler::validate_user",
                vec![("u", "User")],
                Some("Result<User, ()>"),
                "src/handler.rs",
            )],
        ),
    ]);
    let section =
        section_with_canonical_and_boundary("crate::identity::User", "crate::dto::UserDto", &[]);
    let diags = ot009(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

// ---- OT010 ----

#[test]
fn ot010_fires_on_overlapping_unaccepted_enum() {
    let air = air_with_files(vec![
        (
            "src/identity.rs",
            vec![enum_in_file(
                "crate::identity::Status",
                "Status",
                &["Active", "Disabled", "Pending"],
                "src/identity.rs",
            )],
        ),
        (
            "src/elsewhere.rs",
            vec![enum_in_file(
                "crate::elsewhere::UserState",
                "UserState",
                &["Active", "Disabled", "Banned"],
                "src/elsewhere.rs",
            )],
        ),
    ]);
    let section = {
        use super::super::lockfile_schema::{AcceptedCanonical, ConceptEntry, OtSection, Source};
        let mut concepts = BTreeMap::new();
        concepts.insert(
            "status".to_string(),
            ConceptEntry {
                canonical: AcceptedCanonical {
                    symbol: "crate::identity::Status".into(),
                    source: Source::Hint,
                },
                boundaries: Vec::new(),
                converters: Vec::new(),
            },
        );
        OtSection {
            concepts,
            ..Default::default()
        }
    };
    let diags = ot010(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "OT010");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(diags[0].message.contains("UserState"));
}

#[test]
fn ot010_quiet_when_overlap_below_threshold() {
    let air = air_with_files(vec![
        (
            "src/identity.rs",
            vec![enum_in_file(
                "crate::identity::Status",
                "Status",
                &["Active", "Disabled", "Pending"],
                "src/identity.rs",
            )],
        ),
        (
            "src/elsewhere.rs",
            vec![enum_in_file(
                "crate::elsewhere::Color",
                "Color",
                &["Red", "Green", "Blue"],
                "src/elsewhere.rs",
            )],
        ),
    ]);
    let section = {
        use super::super::lockfile_schema::{AcceptedCanonical, ConceptEntry, OtSection, Source};
        let mut concepts = BTreeMap::new();
        concepts.insert(
            "status".to_string(),
            ConceptEntry {
                canonical: AcceptedCanonical {
                    symbol: "crate::identity::Status".into(),
                    source: Source::Hint,
                },
                boundaries: Vec::new(),
                converters: Vec::new(),
            },
        );
        OtSection {
            concepts,
            ..Default::default()
        }
    };
    assert!(ot010(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ot010_silent_when_canonical_is_not_an_enum() {
    let air = air_with_files(vec![(
        "src/elsewhere.rs",
        vec![enum_in_file(
            "crate::elsewhere::Status",
            "Status",
            &["A", "B"],
            "src/elsewhere.rs",
        )],
    )]);
    let section =
        section_with_canonical_and_boundary("crate::identity::User", "crate::dto::UserDto", &[]);
    assert!(ot010(&air, &section, CheckMode::Human).is_empty());
}

// ---- OT011 ----

#[test]
fn ot011_fires_on_shadow_newtype_with_same_name() {
    let air = air_with_files(vec![
        (
            "src/identity.rs",
            vec![struct_with_fields(
                "crate::identity::UserId",
                "UserId",
                &[("0", "String")],
                "src/identity.rs",
            )],
        ),
        (
            "src/dto.rs",
            vec![struct_with_fields(
                "crate::dto::UserId",
                "UserId",
                &[("0", "String")],
                "src/dto.rs",
            )],
        ),
    ]);
    let section = {
        use super::super::lockfile_schema::{AcceptedCanonical, ConceptEntry, OtSection, Source};
        let mut concepts = BTreeMap::new();
        concepts.insert(
            "user-id".to_string(),
            ConceptEntry {
                canonical: AcceptedCanonical {
                    symbol: "crate::identity::UserId".into(),
                    source: Source::Hint,
                },
                boundaries: Vec::new(),
                converters: Vec::new(),
            },
        );
        OtSection {
            concepts,
            ..Default::default()
        }
    };
    let diags = ot011(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "OT011");
    assert!(diags[0].message.contains("crate::dto::UserId"));
}

#[test]
fn ot011_quiet_for_multi_field_structs() {
    let air = air_with_files(vec![(
        "src/dto.rs",
        vec![struct_with_fields(
            "crate::dto::UserId",
            "UserId",
            &[("a", "String"), ("b", "String")],
            "src/dto.rs",
        )],
    )]);
    let section = {
        use super::super::lockfile_schema::{AcceptedCanonical, ConceptEntry, OtSection, Source};
        let mut concepts = BTreeMap::new();
        concepts.insert(
            "user-id".to_string(),
            ConceptEntry {
                canonical: AcceptedCanonical {
                    symbol: "crate::identity::UserId".into(),
                    source: Source::Hint,
                },
                boundaries: Vec::new(),
                converters: Vec::new(),
            },
        );
        OtSection {
            concepts,
            ..Default::default()
        }
    };
    assert!(ot011(&air, &section, CheckMode::Human).is_empty());
}

// ---- OT012 ----

#[test]
fn ot012_fires_on_primitive_field_named_after_canonical() {
    let air = air_with_files(vec![
        (
            "src/identity.rs",
            vec![struct_with_fields(
                "crate::identity::UserId",
                "UserId",
                &[("0", "String")],
                "src/identity.rs",
            )],
        ),
        (
            "src/cmd.rs",
            vec![struct_with_fields(
                "crate::cmd::UserCommand",
                "UserCommand",
                &[("user_id", "String")],
                "src/cmd.rs",
            )],
        ),
    ]);
    let section = {
        use super::super::lockfile_schema::{AcceptedCanonical, ConceptEntry, OtSection, Source};
        let mut concepts = BTreeMap::new();
        concepts.insert(
            "user-id".to_string(),
            ConceptEntry {
                canonical: AcceptedCanonical {
                    symbol: "crate::identity::UserId".into(),
                    source: Source::Hint,
                },
                boundaries: Vec::new(),
                converters: Vec::new(),
            },
        );
        OtSection {
            concepts,
            ..Default::default()
        }
    };
    let diags = ot012(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "OT012");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(diags[0].message.contains("user_id"));
    assert!(diags[0].message.contains("String"));
}

#[test]
fn ot012_quiet_when_field_typed_as_canonical() {
    let air = air_with_files(vec![
        (
            "src/identity.rs",
            vec![struct_with_fields(
                "crate::identity::UserId",
                "UserId",
                &[("0", "String")],
                "src/identity.rs",
            )],
        ),
        (
            "src/cmd.rs",
            vec![struct_with_fields(
                "crate::cmd::UserCommand",
                "UserCommand",
                &[("user_id", "UserId")],
                "src/cmd.rs",
            )],
        ),
    ]);
    let section = {
        use super::super::lockfile_schema::{AcceptedCanonical, ConceptEntry, OtSection, Source};
        let mut concepts = BTreeMap::new();
        concepts.insert(
            "user-id".to_string(),
            ConceptEntry {
                canonical: AcceptedCanonical {
                    symbol: "crate::identity::UserId".into(),
                    source: Source::Hint,
                },
                boundaries: Vec::new(),
                converters: Vec::new(),
            },
        );
        OtSection {
            concepts,
            ..Default::default()
        }
    };
    assert!(ot012(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn ot012_quiet_when_struct_is_accepted_boundary() {
    // Boundaries mirror the wire shape; primitives there are fine.
    let air = air_with_files(vec![(
        "src/dto.rs",
        vec![struct_with_fields(
            "crate::dto::UserDto",
            "UserDto",
            &[("user_id", "String")],
            "src/dto.rs",
        )],
    )]);
    let section = {
        use super::super::lockfile_schema::{
            AcceptedBoundary, AcceptedCanonical, ConceptEntry, OtSection, Source,
        };
        let mut concepts = BTreeMap::new();
        concepts.insert(
            "user-id".to_string(),
            ConceptEntry {
                canonical: AcceptedCanonical {
                    symbol: "crate::identity::UserId".into(),
                    source: Source::Hint,
                },
                boundaries: vec![AcceptedBoundary {
                    symbol: "crate::dto::UserDto".into(),
                    boundary: None,
                    source: Source::Hint,
                }],
                converters: Vec::new(),
            },
        );
        OtSection {
            concepts,
            ..Default::default()
        }
    };
    assert!(ot012(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn snake_to_pascal_round_trips_known_shapes() {
    assert_eq!(snake_to_pascal("user_id").as_deref(), Some("UserId"));
    assert_eq!(snake_to_pascal("email").as_deref(), Some("Email"));
    assert_eq!(
        snake_to_pascal("email_address").as_deref(),
        Some("EmailAddress")
    );
    assert!(snake_to_pascal("").is_none());
    assert!(snake_to_pascal("a__b").is_none());
}

#[test]
fn is_primitive_type_text_handles_refs_and_options() {
    assert!(is_primitive_type_text("String"));
    assert!(is_primitive_type_text("&str"));
    assert!(is_primitive_type_text("i64"));
    assert!(is_primitive_type_text("Option<String>"));
    assert!(!is_primitive_type_text("UserId"));
    assert!(!is_primitive_type_text("Vec<String>"));
}
