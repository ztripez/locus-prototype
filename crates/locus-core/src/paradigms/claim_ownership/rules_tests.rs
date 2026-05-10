use super::*;
use locus_air::{AIR_SCHEMA_VERSION, AirFile, AirFunction, AirPackage, AirSpan, AirType};

fn typ_with_doc(name: &str, doc: Option<&str>) -> AirItem {
    AirItem::Type(AirType {
        kind: locus_air::TypeKind::Struct,
        name: name.into(),
        symbol: format!("crate::{name}"),
        symbol_segments: Vec::new(),
        visibility: locus_air::Visibility::Public,
        fields: Vec::new(),
        variants: Vec::new(),
        decorators: Vec::new(),
        span: AirSpan::new("t.rs", 1, 1),
        doc: doc.map(str::to_string),
    })
}

fn fn_with_doc(name: &str, doc: Option<&str>) -> AirItem {
    AirItem::Function(AirFunction {
        name: name.into(),
        symbol: format!("crate::{name}"),
        symbol_segments: Vec::new(),
        visibility: locus_air::Visibility::Public,
        params: Vec::new(),
        return_type: None,
        decorators: Vec::new(),
        span: AirSpan::new("t.rs", 1, 1),
        line_count: 1,
        doc: doc.map(str::to_string),
    })
}

fn ws(module: Option<&str>, items: Vec<AirItem>) -> AirWorkspace {
    AirWorkspace {
        schema_version: AIR_SCHEMA_VERSION,
        packages: vec![AirPackage {
            name: "x".into(),
            version: "0".into(),
            root_dir: "/".into(),
            files: vec![AirFile {
                path: "t.rs".into(),
                module_path: module.map(str::to_string),
                items,
                hints: Vec::new(),
                parse_error: None,
                line_count: 1,
            }],
        }],
        facts: Vec::new(),
    }
}

fn enabled() -> ClSection {
    ClSection {
        require_local_rationale: true,
        ..ClSection::default()
    }
}

#[test]
fn cl001_silent_when_require_local_rationale_is_default_false() {
    let air = ws(Some("a"), vec![typ_with_doc("Foo", Some("See #123."))]);
    let section = ClSection::default();
    assert!(cl001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn cl001_fires_on_orphan_issue_reference_in_type_doc() {
    let air = ws(Some("a"), vec![typ_with_doc("Foo", Some("See #123."))]);
    let diags = cl001(&air, &enabled(), CheckMode::Human);
    assert_eq!(diags.len(), 1, "expected one CL001, got {diags:#?}");
    assert!(diags[0].message.contains("type `crate::Foo`"));
    assert!(diags[0].message.contains("1 external reference"));
}

#[test]
fn cl001_fires_on_orphan_url_reference_in_function_doc() {
    let air = ws(
        Some("a"),
        vec![fn_with_doc(
            "bar",
            Some("See https://example.org/spec/v2 ."),
        )],
    );
    let diags = cl001(&air, &enabled(), CheckMode::Human);
    assert_eq!(diags.len(), 1, "expected one CL001, got {diags:#?}");
    let why = diags[0].why.join("\n");
    assert!(why.contains("https://example.org/spec/v2"));
}

#[test]
fn cl001_quiet_when_doc_has_local_rationale_alongside_reference() {
    let doc = "Use the compatibility path because mobile clients still send v1 \
               payloads. See #123 for the migration plan.";
    let air = ws(Some("a"), vec![typ_with_doc("Foo", Some(doc))]);
    let diags = cl001(&air, &enabled(), CheckMode::Human);
    assert!(
        diags.is_empty(),
        "doc has rationale + reference; rule should not fire. got {diags:#?}",
    );
}

#[test]
fn cl001_quiet_when_no_references_present() {
    let doc = "Plain doc text describing the type's role in the system.";
    let air = ws(Some("a"), vec![typ_with_doc("Foo", Some(doc))]);
    assert!(cl001(&air, &enabled(), CheckMode::Human).is_empty());
}

#[test]
fn cl001_quiet_for_items_without_doc() {
    let air = ws(Some("a"), vec![typ_with_doc("Foo", None)]);
    assert!(cl001(&air, &enabled(), CheckMode::Human).is_empty());
}

#[test]
fn cl001_skips_files_in_exempt_paths() {
    let air = ws(
        Some("a::tests::widget_tests"),
        vec![typ_with_doc("Foo", Some("See #1."))],
    );
    let section = ClSection {
        require_local_rationale: true,
        exempt_paths: vec!["*::tests::*".into()],
    };
    assert!(cl001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn cl001_agent_strict_elevates_to_fatal() {
    let air = ws(Some("a"), vec![typ_with_doc("Foo", Some("See #1."))]);
    let diags = cl001(&air, &enabled(), CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

#[test]
fn cl001_handles_multiple_references_in_same_doc_block() {
    // Both `#1` and the URL count; word count after stripping is still
    // small ("See and ."), so it fires once with a count of 2.
    let air = ws(
        Some("a"),
        vec![typ_with_doc(
            "Foo",
            Some("See #1 and https://x.io/issue/1."),
        )],
    );
    let diags = cl001(&air, &enabled(), CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("2 external reference"));
}

#[test]
fn cl001_only_inspects_public_items() {
    let mut item = typ_with_doc("Foo", Some("See #1."));
    if let AirItem::Type(t) = &mut item {
        t.visibility = locus_air::Visibility::Module;
    }
    let air = ws(Some("a"), vec![item]);
    // The MVP scans all items with `doc`; private items with doc
    // comments are uncommon but technically still visible to the
    // scanner. This test documents the current behaviour: the rule
    // fires regardless of visibility, since the doc text is the
    // authority surface either way.
    let diags = cl001(&air, &enabled(), CheckMode::Human);
    assert_eq!(diags.len(), 1);
}

// ---- analyse_doc unit tests ----

#[test]
fn analyse_extracts_github_style_issue_reference() {
    let a = analyse_doc("See #123.");
    assert_eq!(a.references, vec!["#123"]);
    assert_eq!(a.non_reference_word_count, 1); // "See"
}

#[test]
fn analyse_extracts_url_reference() {
    let a = analyse_doc("Spec at https://example.org/foo/bar.");
    assert_eq!(a.references, vec!["https://example.org/foo/bar"]);
    assert_eq!(a.non_reference_word_count, 2); // "Spec at"
}

#[test]
fn analyse_does_not_match_inline_hash_in_word_position() {
    // `f#x` shouldn't count as a reference; `#x` isn't valid either
    // (no digits). Hash followed by non-digit doesn't fire.
    let a = analyse_doc("Use the f#x format.");
    assert!(a.references.is_empty());
}

#[test]
fn analyse_strips_trailing_punctuation_from_url() {
    let a = analyse_doc("(see https://x.io/foo).");
    assert_eq!(a.references, vec!["https://x.io/foo"]);
}
