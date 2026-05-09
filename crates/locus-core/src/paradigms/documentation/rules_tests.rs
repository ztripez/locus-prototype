//! Tests for [`super`] rule implementations.
//!
//! Extracted from `rules.rs` to keep the production module within the
//! CX002 line budget. Re-attached via `#[path = "rules_tests.rs"] mod
//! tests;` at the bottom of `rules.rs`.

use super::*;
use locus_air::{
    AIR_SCHEMA_VERSION, AirFile, AirFunction, AirPackage, AirSpan, AirType, TypeKind, Visibility,
};

fn ty_item(name: &str, vis: Visibility, doc: Option<&str>) -> AirItem {
    AirItem::Type(AirType {
        kind: TypeKind::Struct,
        name: name.into(),
        symbol: format!("x::api::{name}"),
        visibility: vis,
        fields: Vec::new(),
        variants: Vec::new(),
        decorators: Vec::new(),
        symbol_segments: Vec::new(),
        span: AirSpan::new("t.rs", 1, 1),
        doc: doc.map(|s| s.to_string()),
    })
}

fn fn_item(name: &str, vis: Visibility, doc: Option<&str>) -> AirItem {
    AirItem::Function(AirFunction {
        name: name.into(),
        symbol: format!("x::api::{name}"),
        visibility: vis,
        params: Vec::new(),
        return_type: None,
        span: AirSpan::new("t.rs", 1, 1),
        line_count: 1,
        decorators: Vec::new(),
        symbol_segments: Vec::new(),
        doc: doc.map(|s| s.to_string()),
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
fn dc001_silent_when_require_public_docs_is_default_false() {
    let air = air_with_module(
        "x::api",
        vec![
            ty_item("Widget", Visibility::Public, None),
            fn_item("make_widget", Visibility::Public, None),
        ],
    );
    let section = DcSection::default();
    assert!(dc001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn dc001_fires_on_public_type_without_doc() {
    let air = air_with_module("x::api", vec![ty_item("Widget", Visibility::Public, None)]);
    let section = DcSection {
        require_public_docs: true,
        exempt_paths: Vec::new(),
        ..DcSection::default()
    };
    let diags = dc001(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "DC001");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(diags[0].message.contains("Widget"));
    assert!(diags[0].message.contains("x::api"));
    assert!(diags[0].message.contains("no doc comment"));
}

#[test]
fn dc001_fires_on_public_function_without_doc() {
    let air = air_with_module(
        "x::api",
        vec![fn_item("make_widget", Visibility::Public, None)],
    );
    let section = DcSection {
        require_public_docs: true,
        exempt_paths: Vec::new(),
        ..DcSection::default()
    };
    let diags = dc001(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "DC001");
    assert!(diags[0].message.contains("make_widget"));
    assert!(diags[0].message.contains("public function"));
}

#[test]
fn dc001_quiet_on_private_items() {
    let air = air_with_module(
        "x::api",
        vec![
            ty_item("Widget", Visibility::Private, None),
            ty_item("Inner", Visibility::Module, None),
            ty_item("Restricted", Visibility::Restricted, None),
            fn_item("helper", Visibility::Private, None),
            fn_item("crate_helper", Visibility::Module, None),
        ],
    );
    let section = DcSection {
        require_public_docs: true,
        exempt_paths: Vec::new(),
        ..DcSection::default()
    };
    assert!(dc001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn dc001_quiet_on_items_with_doc() {
    let air = air_with_module(
        "x::api",
        vec![
            ty_item("Widget", Visibility::Public, Some("a thing")),
            fn_item("make_widget", Visibility::Public, Some("makes one")),
        ],
    );
    let section = DcSection {
        require_public_docs: true,
        exempt_paths: Vec::new(),
        ..DcSection::default()
    };
    assert!(dc001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn dc001_skips_files_matching_exempt_paths() {
    let air = air_with_module(
        "x::api::tests",
        vec![
            ty_item("Widget", Visibility::Public, None),
            fn_item("make_widget", Visibility::Public, None),
        ],
    );
    let section = DcSection {
        require_public_docs: true,
        exempt_paths: vec!["x::api::tests::*".into(), "x::api::tests".into()],
        ..DcSection::default()
    };
    assert!(dc001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn dc001_agent_strict_elevates_to_fatal() {
    let air = air_with_module("x::api", vec![ty_item("Widget", Visibility::Public, None)]);
    let section = DcSection {
        require_public_docs: true,
        exempt_paths: Vec::new(),
        ..DcSection::default()
    };
    let diags = dc001(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

#[test]
fn dc001_fires_per_undocumented_item_in_mixed_file() {
    let air = air_with_module(
        "x::api",
        vec![
            ty_item("Documented", Visibility::Public, Some("good")),
            ty_item("UndocType", Visibility::Public, None),
            fn_item("undoc_fn", Visibility::Public, None),
            ty_item("PrivateType", Visibility::Private, None),
        ],
    );
    let section = DcSection {
        require_public_docs: true,
        exempt_paths: Vec::new(),
        ..DcSection::default()
    };
    let diags = dc001(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 2);
    let messages: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
    assert!(messages.iter().any(|m| m.contains("UndocType")));
    assert!(messages.iter().any(|m| m.contains("undoc_fn")));
    assert!(!messages.iter().any(|m| m.contains("Documented")));
    assert!(!messages.iter().any(|m| m.contains("PrivateType")));
}

use super::super::lockfile_schema::ForbiddenPhrase;

#[test]
fn dc002_fires_when_public_type_doc_contains_as_discussed() {
    let air = air_with_module(
        "x::api",
        vec![ty_item(
            "Widget",
            Visibility::Public,
            Some("As discussed, this represents a widget."),
        )],
    );
    let section = DcSection::default();
    let diags = dc002(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "DC002");
    assert!(diags[0].concept.is_none());
    assert!(diags[0].message.contains("Widget"));
    assert!(diags[0].message.contains("as discussed"));
    // 0.90 confidence => Fatal regardless of mode.
    assert_eq!(diags[0].severity, Severity::Fatal);
}

#[test]
fn dc002_fires_when_public_function_doc_contains_todo() {
    let air = air_with_module(
        "x::api",
        vec![fn_item(
            "make_widget",
            Visibility::Public,
            Some("Returns a widget. TODO: handle errors."),
        )],
    );
    let section = DcSection::default();
    let diags = dc002(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "DC002");
    assert!(diags[0].message.contains("make_widget"));
    assert!(diags[0].message.contains("TODO"));
    // 0.70 confidence under Human => Warning.
    assert_eq!(diags[0].severity, Severity::Warning);
}

#[test]
fn dc002_matching_is_case_insensitive() {
    // Lowercase `todo` in source should still match the seeded `TODO` phrase.
    let air = air_with_module(
        "x::api",
        vec![ty_item(
            "Widget",
            Visibility::Public,
            Some("a widget. todo: revisit."),
        )],
    );
    let section = DcSection::default();
    let diags = dc002(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("TODO"));
}

#[test]
fn dc002_silent_when_forbidden_doc_phrases_is_empty() {
    // Even with a doc that would otherwise match every default phrase,
    // an empty list is the documented opt-out.
    let air = air_with_module(
        "x::api",
        vec![ty_item(
            "Widget",
            Visibility::Public,
            Some("As discussed, TODO, FIXME, HACK, the prompt — for now."),
        )],
    );
    let section = DcSection {
        forbidden_doc_phrases: Vec::new(),
        ..DcSection::default()
    };
    assert!(dc002(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn dc002_fires_once_per_matched_phrase_on_one_item() {
    let air = air_with_module(
        "x::api",
        vec![ty_item(
            "Widget",
            Visibility::Public,
            Some("As discussed, TODO clean this up."),
        )],
    );
    let section = DcSection::default();
    let diags = dc002(&air, &section, CheckMode::Human);
    // Default list contains "as discussed", "TODO", and "clean this up"
    // — three matches on one item.
    assert_eq!(diags.len(), 3);
    let messages: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
    assert!(messages.iter().any(|m| m.contains("as discussed")));
    assert!(messages.iter().any(|m| m.contains("TODO")));
    assert!(messages.iter().any(|m| m.contains("clean this up")));
}

#[test]
fn dc002_agent_strict_elevates_warning_band_to_fatal() {
    // 0.75 confidence => Warning under Human, Fatal under AgentStrict.
    let air = air_with_module(
        "x::api",
        vec![ty_item(
            "Widget",
            Visibility::Public,
            Some("A widget. For now this leaks."),
        )],
    );
    let section = DcSection {
        forbidden_doc_phrases: vec![ForbiddenPhrase {
            phrase: "for now".into(),
            confidence: 0.75,
            aliases: Vec::new(),
        }],
        ..DcSection::default()
    };
    let human = dc002(&air, &section, CheckMode::Human);
    assert_eq!(human.len(), 1);
    assert_eq!(human[0].severity, Severity::Warning);

    let strict = dc002(&air, &section, CheckMode::AgentStrict);
    assert_eq!(strict.len(), 1);
    assert_eq!(strict[0].severity, Severity::Fatal);
}

#[test]
fn dc002_skips_items_without_doc() {
    let air = air_with_module(
        "x::api",
        vec![
            ty_item("NoDoc", Visibility::Public, None),
            fn_item("no_doc_fn", Visibility::Public, None),
        ],
    );
    let section = DcSection::default();
    assert!(dc002(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn dc002_skips_non_public_items_with_residue() {
    // Residue in a private type is a separate problem; DC002 scopes to
    // public surface only, mirroring DC001.
    let air = air_with_module(
        "x::api",
        vec![ty_item(
            "Internal",
            Visibility::Private,
            Some("TODO: refactor this"),
        )],
    );
    let section = DcSection::default();
    assert!(dc002(&air, &section, CheckMode::Human).is_empty());
}

// ---- alias-matching tests ----

#[test]
fn dc002_matches_alias_and_surfaces_it_in_diagnostic() {
    let air = air_with_module(
        "x::api",
        vec![ty_item(
            "Widget",
            Visibility::Public,
            Some("This struct is what you wanted; we'll iterate next pass."),
        )],
    );
    let section = DcSection {
        forbidden_doc_phrases: vec![ForbiddenPhrase {
            phrase: "the user wanted".into(),
            confidence: 0.85,
            aliases: vec!["you wanted".into(), "you requested".into()],
        }],
        ..DcSection::default()
    };
    let diags = dc002(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert!(
        diags[0].message.contains("you wanted"),
        "diagnostic should surface the matched alias; got {}",
        diags[0].message,
    );
    assert!(
        diags[0].message.contains("(alias of `the user wanted`)"),
        "diagnostic should note alias-of-primary; got {}",
        diags[0].message,
    );
}

#[test]
fn dc002_primary_phrase_match_does_not_show_alias_note() {
    let air = air_with_module(
        "x::api",
        vec![ty_item(
            "Widget",
            Visibility::Public,
            Some("As discussed, this needs more work."),
        )],
    );
    let section = DcSection {
        forbidden_doc_phrases: vec![ForbiddenPhrase {
            phrase: "as discussed".into(),
            confidence: 0.90,
            aliases: vec!["as we discussed".into(), "we discussed".into()],
        }],
        ..DcSection::default()
    };
    let diags = dc002(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert!(
        !diags[0].message.contains("(alias of"),
        "primary-match diagnostic shouldn't carry the alias-note; got {}",
        diags[0].message,
    );
}

#[test]
fn dc002_alias_match_is_case_insensitive() {
    let air = air_with_module(
        "x::api",
        vec![ty_item(
            "Widget",
            Visibility::Public,
            Some("AS WE DISCUSSED, this is fine."),
        )],
    );
    let section = DcSection {
        forbidden_doc_phrases: vec![ForbiddenPhrase {
            phrase: "as discussed".into(),
            confidence: 0.90,
            aliases: vec!["as we discussed".into()],
        }],
        ..DcSection::default()
    };
    assert_eq!(dc002(&air, &section, CheckMode::Human).len(), 1);
}

#[test]
fn dc002_seed_aliases_cover_paraphrased_residue() {
    // The motivating use case: the seed list ships with curated
    // aliases so users don't need to enumerate them per-codebase.
    // "we agreed" should match through the `as discussed` alias set.
    let air = air_with_module(
        "x::api",
        vec![ty_item(
            "Widget",
            Visibility::Public,
            Some("We agreed this was good enough."),
        )],
    );
    let section = DcSection::default();
    let diags = dc002(&air, &section, CheckMode::Human);
    assert_eq!(
        diags.len(),
        1,
        "expected default aliases to catch `we agreed`; got {diags:?}"
    );
}

// ---- DC004: owner-less follow-up marker ----

fn dc004_section_only(markers: Vec<&str>) -> DcSection {
    // Suppress DC002 so test asserts cleanly target dc004's output —
    // the test asserts on dc004 directly, but using `DcSection {
    // forbidden_doc_phrases: vec![] }` keeps the section honest.
    DcSection {
        forbidden_doc_phrases: Vec::new(),
        unowned_marker_patterns: markers.into_iter().map(String::from).collect(),
        ..DcSection::default()
    }
}

#[test]
fn dc004_fires_on_owner_less_todo_in_public_type() {
    let air = air_with_module(
        "x::api",
        vec![ty_item(
            "Widget",
            Visibility::Public,
            Some("A widget. TODO: revisit later."),
        )],
    );
    let section = dc004_section_only(vec!["TODO", "FIXME", "HACK", "XXX"]);
    let diags = dc004(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1, "got {diags:?}");
    assert_eq!(diags[0].rule_id, "DC004");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(diags[0].message.contains("Widget"));
    assert!(diags[0].message.contains("TODO"));
    assert!(diags[0].message.contains("without an owner reference"));
}

#[test]
fn dc004_quiet_when_marker_has_owner_parenthesis() {
    // `TODO(alice):` and `FIXME(#123):` are both owned and should
    // pass silently — the marker is well-formed.
    let air = air_with_module(
        "x::api",
        vec![
            ty_item(
                "A",
                Visibility::Public,
                Some("TODO(alice): rewrite this when the API stabilizes."),
            ),
            fn_item(
                "do_thing",
                Visibility::Public,
                Some("FIXME(#123): handle the timeout case."),
            ),
        ],
    );
    let section = dc004_section_only(vec!["TODO", "FIXME", "HACK", "XXX"]);
    assert!(dc004(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn dc004_case_insensitive_marker_match() {
    // Lowercase / mixed-case markers should still fire.
    let air = air_with_module(
        "x::api",
        vec![
            ty_item("A", Visibility::Public, Some("todo: refactor.")),
            ty_item("B", Visibility::Public, Some("Fixme: handle err.")),
        ],
    );
    let section = dc004_section_only(vec!["TODO", "FIXME"]);
    let diags = dc004(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 2, "got {diags:?}");
}

#[test]
fn dc004_silent_when_unowned_marker_patterns_empty() {
    let air = air_with_module(
        "x::api",
        vec![ty_item(
            "Widget",
            Visibility::Public,
            Some("TODO: nothing should fire because the list is empty."),
        )],
    );
    let section = DcSection {
        forbidden_doc_phrases: Vec::new(),
        unowned_marker_patterns: Vec::new(),
        ..DcSection::default()
    };
    assert!(dc004(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn dc004_skips_non_public_items() {
    // DC004 mirrors DC001/DC002: public surface only.
    let air = air_with_module(
        "x::api",
        vec![ty_item(
            "Internal",
            Visibility::Private,
            Some("TODO: refactor"),
        )],
    );
    let section = dc004_section_only(vec!["TODO"]);
    assert!(dc004(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn dc004_agent_strict_elevates_to_fatal() {
    let air = air_with_module(
        "x::api",
        vec![ty_item(
            "Widget",
            Visibility::Public,
            Some("Widget. TODO: rewrite."),
        )],
    );
    let section = dc004_section_only(vec!["TODO"]);
    let diags = dc004(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

#[test]
fn dc004_fires_per_unowned_occurrence_and_skips_owned() {
    // `TODO(alice):` is owned (silent); the second `TODO:` is owner-less.
    let air = air_with_module(
        "x::api",
        vec![ty_item(
            "Widget",
            Visibility::Public,
            Some("TODO(alice): step 1. Then TODO: handle step 2."),
        )],
    );
    let section = dc004_section_only(vec!["TODO"]);
    let diags = dc004(&air, &section, CheckMode::Human);
    assert_eq!(
        diags.len(),
        1,
        "expected only the unowned `TODO:` to fire; got {diags:?}"
    );
}
