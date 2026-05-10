use super::super::lockfile_schema::{MoOverride, MoSection};
use super::*;
use locus_air::{
    AIR_SCHEMA_VERSION, AirCallSite, AirConversion, AirFunction, AirImplBlock, AirPackage, AirType,
    CallKind, ConversionMechanism, TypeKind,
};

fn pub_type(name: &str) -> AirItem {
    AirItem::Type(AirType {
        kind: TypeKind::Struct,
        name: name.into(),
        symbol: format!("x::{name}"),
        visibility: Visibility::Public,
        fields: Vec::new(),
        variants: Vec::new(),
        decorators: Vec::new(),
        symbol_segments: Vec::new(),
        span: AirSpan::new("t.rs", 1, 1),
        doc: None,
    })
}

fn priv_type(name: &str) -> AirItem {
    AirItem::Type(AirType {
        kind: TypeKind::Struct,
        name: name.into(),
        symbol: format!("x::{name}"),
        visibility: Visibility::Private,
        fields: Vec::new(),
        variants: Vec::new(),
        decorators: Vec::new(),
        symbol_segments: Vec::new(),
        span: AirSpan::new("t.rs", 1, 1),
        doc: None,
    })
}

fn air_with(module: Option<&str>, items: Vec<AirItem>) -> AirWorkspace {
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

fn n_pub_types(n: usize) -> Vec<AirItem> {
    (0..n).map(|i| pub_type(&format!("T{i}"))).collect()
}

fn configured(default_budget: u32) -> MoSection {
    MoSection {
        default_max_public_types: Some(default_budget),
        overrides: Vec::new(),
        entropy_threshold: None,
        handler_name_patterns: Vec::new(),
        persistence_import_patterns: Vec::new(),
    }
}

#[test]
fn mo001_fires_with_built_in_fallback_on_default_section() {
    // Default section uses DEFAULT_MAX_PUBLIC_TYPES (5) as the budget.
    // 50 public types trip it without any user configuration — rule
    // fires by default per the "noisy-by-default" convention.
    let air = air_with(Some("foo::bar"), n_pub_types(50));
    let section = MoSection::default();
    let diags = mo001(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1, "expected one diag, got {diags:?}");
    assert!(
        diags[0].why.iter().any(|w| w.contains("built-in fallback")),
        "expected built-in fallback explanation in why; got {:?}",
        diags[0].why,
    );
}

#[test]
fn mo001_quiet_when_count_within_built_in_fallback() {
    // 3 public types under the 5-type built-in fallback → no diag.
    let air = air_with(Some("foo::bar"), n_pub_types(3));
    let section = MoSection::default();
    assert!(mo001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn mo001_fires_when_count_exceeds_default_budget() {
    // 6 public types under default budget of 5 → fires.
    let air = air_with(Some("foo::bar"), n_pub_types(6));
    let section = configured(5);
    let diags = mo001(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1, "expected one diag, got {diags:?}");
    assert_eq!(diags[0].rule_id, "MO001");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(diags[0].message.contains("foo::bar"));
    assert!(diags[0].message.contains("6"));
    assert!(diags[0].message.contains("budget 5"));
}

#[test]
fn mo001_quiet_when_count_at_or_below_default_budget() {
    let section = configured(5);
    // exactly at budget
    let air = air_with(Some("foo::bar"), n_pub_types(5));
    assert!(mo001(&air, &section, CheckMode::Human).is_empty());
    // under budget
    let air = air_with(Some("foo::bar"), n_pub_types(2));
    assert!(mo001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn mo001_only_counts_public_top_level_types() {
    // 4 private + 5 public = 9 items, but only 5 are pub → at budget, quiet.
    let mut items = n_pub_types(5);
    for i in 0..4 {
        items.push(priv_type(&format!("Priv{i}")));
    }
    let air = air_with(Some("foo::bar"), items);
    let section = configured(5);
    assert!(mo001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn mo001_override_raises_budget_effectively() {
    // Default budget 5; api module has 12 public types, override gives 20.
    let air = air_with(Some("lore::api::v1"), n_pub_types(12));
    let section = MoSection {
        default_max_public_types: Some(5),
        overrides: vec![MoOverride {
            module: "lore::api::*".into(),
            max_public_types: 20,
            ..Default::default()
        }],
        ..Default::default()
    };
    assert!(
        mo001(&air, &section, CheckMode::Human).is_empty(),
        "override should raise budget above the file's count"
    );
}

#[test]
fn mo001_override_lowers_budget_effectively() {
    // Default 5; domain file has 5 public types (within default). Override
    // lowers the domain budget to 2 → fires.
    let air = air_with(Some("lore::domain::user"), n_pub_types(5));
    let section = MoSection {
        default_max_public_types: Some(5),
        overrides: vec![MoOverride {
            module: "lore::domain::*".into(),
            max_public_types: 2,
            ..Default::default()
        }],
        ..Default::default()
    };
    let diags = mo001(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1, "override should lower budget below count");
    assert_eq!(diags[0].rule_id, "MO001");
    assert!(diags[0].message.contains("budget 2"));
    assert!(
        diags[0]
            .why
            .iter()
            .any(|w| w.contains("override") && w.contains("lore::domain::*")),
        "expected override mention in `why`; got {:?}",
        diags[0].why
    );
}

#[test]
fn mo001_first_override_wins() {
    let air = air_with(Some("lore::api::v1"), n_pub_types(8));
    let section = MoSection {
        default_max_public_types: Some(5),
        overrides: vec![
            MoOverride {
                module: "lore::api::*".into(),
                max_public_types: 20,
                ..Default::default()
            },
            MoOverride {
                module: "lore::*".into(),
                max_public_types: 3,
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    // First override (20) wins, so 8 public types is fine.
    assert!(mo001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn mo001_agent_strict_elevates_to_fatal() {
    let air = air_with(Some("foo::bar"), n_pub_types(6));
    let section = configured(5);
    let diags = mo001(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(
        diags[0].severity,
        Severity::Fatal,
        "agent-strict should elevate Warning to Fatal"
    );
}

#[test]
fn mo001_skips_files_without_module_path() {
    // No module_path → can't apply overrides → skip entirely.
    let air = air_with(None, n_pub_types(50));
    let section = configured(5);
    assert!(mo001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn mo001_one_diagnostic_per_file() {
    // Two violating files → two diagnostics, regardless of how many
    // public types each contains.
    let air = AirWorkspace {
        schema_version: AIR_SCHEMA_VERSION,
        packages: vec![AirPackage {
            name: "x".into(),
            version: "0".into(),
            root_dir: "/".into(),
            files: vec![
                AirFile {
                    path: "a.rs".into(),
                    module_path: Some("x::a".into()),
                    items: n_pub_types(10),
                    hints: Vec::new(),
                    parse_error: None,
                    line_count: 1,
                },
                AirFile {
                    path: "b.rs".into(),
                    module_path: Some("x::b".into()),
                    items: n_pub_types(7),
                    hints: Vec::new(),
                    parse_error: None,
                    line_count: 1,
                },
            ],
        }],
        facts: Vec::new(),
    };
    let section = configured(5);
    let diags = mo001(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 2, "got {diags:?}");
}

#[test]
fn mo001_with_only_overrides_and_no_default_uses_fallback_for_unmatched() {
    // overrides set → section is non-default → MO001 active. Files that
    // don't match any override fall back to DEFAULT_MAX_PUBLIC_TYPES (5).
    let air = air_with(Some("other::module"), n_pub_types(6));
    let section = MoSection {
        default_max_public_types: None,
        overrides: vec![MoOverride {
            module: "lore::api::*".into(),
            max_public_types: 20,
            ..Default::default()
        }],
        ..Default::default()
    };
    let diags = mo001(&air, &section, CheckMode::Human);
    assert_eq!(
        diags.len(),
        1,
        "fallback budget should apply; got {diags:?}"
    );
    assert!(diags[0].message.contains("budget 5"));
    assert!(
        diags[0].why.iter().any(|w| w.contains("built-in fallback")),
        "expected fallback explanation in why; got {:?}",
        diags[0].why
    );
}

// ---- shared helpers for MO002 / MO003 / MO004 tests ----

fn canonical_hint() -> AirHint {
    AirHint {
        kind: HintKind::Canonical,
        raw: "// locus: ot canonical".into(),
        span: AirSpan::new("t.rs", 5, 5),
        target_span: Some(AirSpan::new("t.rs", 6, 10)),
    }
}

fn boundary_hint() -> AirHint {
    AirHint {
        kind: HintKind::Boundary {
            concept: Some("user".into()),
            boundary: Some("api".into()),
        },
        raw: "// locus: ot boundary user api".into(),
        span: AirSpan::new("t.rs", 20, 20),
        target_span: Some(AirSpan::new("t.rs", 21, 30)),
    }
}

fn converter_hint() -> AirHint {
    AirHint {
        kind: HintKind::Converter,
        raw: "// locus: ot converter".into(),
        span: AirSpan::new("t.rs", 40, 40),
        target_span: Some(AirSpan::new("t.rs", 41, 45)),
    }
}

fn func(name: &str, line: u32) -> AirItem {
    AirItem::Function(AirFunction {
        name: name.into(),
        symbol: format!("x::{name}"),
        visibility: Visibility::Public,
        params: Vec::new(),
        return_type: None,
        span: AirSpan::new("t.rs", line, line + 5),
        line_count: 6,
        decorators: Vec::new(),
        symbol_segments: Vec::new(),
        doc: None,
    })
}

fn import(path: &str) -> AirItem {
    AirItem::Import(AirImport {
        path: path.into(),
        path_segments: Vec::new(),
        visibility: Visibility::Private,
        span: AirSpan::new("t.rs", 1, 1),
    })
}

fn call_site(callee: &str) -> AirItem {
    AirItem::CallSite(AirCallSite {
        callee: callee.into(),
        kind: CallKind::Function,
        function: None,
        span: AirSpan::new("t.rs", 1, 1),
    })
}

fn air_with_full(module: Option<&str>, items: Vec<AirItem>, hints: Vec<AirHint>) -> AirWorkspace {
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
                hints,
                parse_error: None,
                line_count: 100,
            }],
        }],
        facts: Vec::new(),
    }
}

fn mo_section_with_entropy(threshold: u32) -> MoSection {
    MoSection {
        entropy_threshold: Some(threshold),
        ..Default::default()
    }
}

// ---- MO002 tests ----

#[test]
fn mo002_fires_with_built_in_fallback_on_default_section() {
    // Default section uses entropy_threshold built-in fallback (3).
    // A clear blob (canonical+boundary+converter+handler+persistence)
    // trips it without any user configuration — rule fires by default
    // per the "noisy-by-default" convention.
    let air = air_with_full(
        Some("foo::bar"),
        vec![func("user_handler", 10), import("crate::sqlx::query")],
        vec![canonical_hint(), boundary_hint(), converter_hint()],
    );
    let section = MoSection::default();
    let diags = mo002(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1, "expected one diag, got {diags:?}");
}

#[test]
fn mo002_fires_when_three_roles_meet_default_threshold() {
    // canonical + boundary + handler = 3 roles → at default threshold (3)
    let air = air_with_full(
        Some("foo::bar"),
        vec![func("user_handler", 10)],
        vec![canonical_hint(), boundary_hint()],
    );
    // section is "configured" via entropy_threshold=Some(3) so the rule
    // is active; default threshold path is exercised in another test.
    let section = mo_section_with_entropy(3);
    let diags = mo002(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1, "got {diags:?}");
    assert_eq!(diags[0].rule_id, "MO002");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(diags[0].message.contains("foo::bar"));
    assert!(diags[0].message.contains("3"));
    assert!(diags[0].message.contains("canonical"));
    assert!(diags[0].message.contains("boundary"));
    assert!(diags[0].message.contains("handler"));
}

#[test]
fn mo002_quiet_when_below_threshold() {
    // Only canonical + handler = 2 roles → under default threshold of 3.
    let air = air_with_full(
        Some("foo::bar"),
        vec![func("on_user_handler", 10)],
        vec![canonical_hint()],
    );
    let section = mo_section_with_entropy(3);
    assert!(mo002(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn mo002_counts_persistence_imports_and_io_call_sites() {
    // canonical + persistence import (sqlx) + io call site (fs::read) = 3
    let air = air_with_full(
        Some("foo::bar"),
        vec![
            import("crate::sqlx::query"),
            call_site("std::fs::read_to_string"),
        ],
        vec![canonical_hint()],
    );
    let section = mo_section_with_entropy(3);
    let diags = mo002(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    let m = &diags[0].message;
    assert!(m.contains("canonical"));
    assert!(m.contains("persistence"));
    assert!(m.contains("io"));
}

#[test]
fn mo002_user_handler_patterns_override_defaults() {
    // A function called `process` does NOT match the default
    // `*_handler`/`handle_*` patterns. With user-supplied `process*`
    // pattern it does, raising the role count.
    let air = air_with_full(
        Some("foo::bar"),
        vec![func("process", 10), import("crate::sqlx::query")],
        vec![canonical_hint(), boundary_hint()],
    );
    // Default patterns: canonical + boundary + persistence = 3 → fires.
    // User-narrowed patterns to `does_not_match_*`: canonical + boundary +
    // persistence = 3 (handler still not counted) → still fires.
    // To verify the override path, give threshold = 4 and patterns that
    // match `process` so the count is 4.
    let section = MoSection {
        entropy_threshold: Some(4),
        handler_name_patterns: vec!["process*".into()],
        ..Default::default()
    };
    let diags = mo002(&air, &section, CheckMode::Human);
    assert_eq!(
        diags.len(),
        1,
        "expected fire at threshold 4; got {diags:?}"
    );
    assert!(diags[0].message.contains("handler"));
}

#[test]
fn mo002_agent_strict_elevates_to_fatal_and_skips_no_module_path() {
    // Compound: agent-strict elevates Warning→Fatal; files without a
    // module_path are skipped entirely (no diagnostic).
    let air_with_path = air_with_full(
        Some("foo::bar"),
        vec![func("user_handler", 10)],
        vec![canonical_hint(), boundary_hint()],
    );
    let section = mo_section_with_entropy(3);
    let diags = mo002(&air_with_path, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);

    let air_no_path = air_with_full(
        None,
        vec![func("user_handler", 10)],
        vec![canonical_hint(), boundary_hint(), converter_hint()],
    );
    assert!(mo002(&air_no_path, &section, CheckMode::Human).is_empty());
}

// ---- MO003 tests ----

#[test]
fn mo003_fires_when_canonical_and_boundary_co_exist() {
    let air = air_with_full(
        Some("foo::bar"),
        vec![],
        vec![canonical_hint(), boundary_hint()],
    );
    let diags = mo003(&air, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "MO003");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(diags[0].message.contains("foo::bar"));
    assert!(diags[0].message.contains("canonical"));
    assert!(diags[0].message.contains("boundary"));
}

#[test]
fn mo003_quiet_with_only_canonical() {
    let air = air_with_full(Some("foo::bar"), vec![], vec![canonical_hint()]);
    assert!(mo003(&air, CheckMode::Human).is_empty());
}

#[test]
fn mo003_quiet_with_only_boundary() {
    let air = air_with_full(Some("foo::bar"), vec![], vec![boundary_hint()]);
    assert!(mo003(&air, CheckMode::Human).is_empty());
}

#[test]
fn mo003_quiet_with_no_hints() {
    let air = air_with_full(Some("foo::bar"), vec![func("anything", 1)], vec![]);
    assert!(mo003(&air, CheckMode::Human).is_empty());
}

#[test]
fn mo003_skips_files_without_module_path() {
    let air = air_with_full(None, vec![], vec![canonical_hint(), boundary_hint()]);
    assert!(mo003(&air, CheckMode::Human).is_empty());
}

#[test]
fn mo003_agent_strict_elevates_to_fatal() {
    let air = air_with_full(
        Some("foo::bar"),
        vec![],
        vec![canonical_hint(), boundary_hint()],
    );
    let diags = mo003(&air, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

// ---- MO004 tests ----

#[test]
fn mo004_fires_when_canonical_and_handler_co_exist() {
    let air = air_with_full(
        Some("foo::bar"),
        vec![func("user_handler", 10)],
        vec![canonical_hint()],
    );
    let section = MoSection::default();
    let diags = mo004(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "MO004");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(diags[0].message.contains("foo::bar"));
    assert!(diags[0].message.contains("user_handler"));
}

#[test]
fn mo004_quiet_when_only_canonical_no_handler() {
    let air = air_with_full(
        Some("foo::bar"),
        vec![func("compute", 10)],
        vec![canonical_hint()],
    );
    let section = MoSection::default();
    assert!(mo004(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn mo004_quiet_when_only_handler_no_canonical() {
    let air = air_with_full(Some("foo::bar"), vec![func("user_handler", 10)], vec![]);
    let section = MoSection::default();
    assert!(mo004(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn mo004_uses_user_supplied_handler_patterns_when_set() {
    // `process` doesn't match the default `*_handler`/`handle_*` patterns
    // but does match the user-supplied `process*` pattern.
    let air = air_with_full(
        Some("foo::bar"),
        vec![func("process", 10)],
        vec![canonical_hint()],
    );
    let default_section = MoSection::default();
    assert!(mo004(&air, &default_section, CheckMode::Human).is_empty());
    let section = MoSection {
        handler_name_patterns: vec!["process*".into()],
        ..Default::default()
    };
    let diags = mo004(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("process"));
}

#[test]
fn mo004_skips_files_without_module_path() {
    let air = air_with_full(None, vec![func("user_handler", 10)], vec![canonical_hint()]);
    let section = MoSection::default();
    assert!(mo004(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn mo004_agent_strict_elevates_to_fatal() {
    let air = air_with_full(
        Some("foo::bar"),
        vec![func("handle_request", 10)],
        vec![canonical_hint()],
    );
    let section = MoSection::default();
    let diags = mo004(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

// ---- MO005 test helpers ----

fn type_item(name: &str, kind: TypeKind) -> AirItem {
    AirItem::Type(AirType {
        kind,
        name: name.into(),
        symbol: format!("x::{name}"),
        visibility: Visibility::Public,
        fields: Vec::new(),
        variants: Vec::new(),
        decorators: Vec::new(),
        symbol_segments: Vec::new(),
        span: AirSpan::new("main.rs", 5, 5),
        doc: None,
    })
}

fn impl_item(target: &str) -> AirItem {
    AirItem::Impl(AirImplBlock {
        interface: None,
        target_type: target.into(),
        method_names: Vec::new(),
        dispatch: locus_air::ImplDispatch::Static,
        span: AirSpan::new("main.rs", 10, 15),
    })
}

fn func_with_lines(name: &str, line_count: u32) -> AirItem {
    AirItem::Function(AirFunction {
        name: name.into(),
        symbol: format!("x::{name}"),
        visibility: Visibility::Public,
        params: Vec::new(),
        return_type: None,
        span: AirSpan::new("main.rs", 1, line_count),
        line_count,
        decorators: Vec::new(),
        symbol_segments: Vec::new(),
        doc: None,
    })
}

fn conversion_item(from: &str, to: &str) -> AirItem {
    AirItem::Conversion(AirConversion {
        from: from.into(),
        to: to.into(),
        mechanism: ConversionMechanism::InfallibleAdapter,
        symbol: format!("{from}::from_{to}"),
        span: AirSpan::new("main.rs", 20, 25),
    })
}

fn air_with_module_and_items(module: &str, items: Vec<AirItem>) -> AirWorkspace {
    air_with_module_path_and_file(module, "main.rs", items)
}

fn air_with_module_path_and_file(
    module: &str,
    file_path: &str,
    items: Vec<AirItem>,
) -> AirWorkspace {
    AirWorkspace {
        schema_version: AIR_SCHEMA_VERSION,
        packages: vec![AirPackage {
            name: "x".into(),
            version: "0".into(),
            root_dir: "/".into(),
            files: vec![AirFile {
                path: file_path.into(),
                module_path: Some(module.to_string()),
                items,
                hints: Vec::new(),
                parse_error: None,
                line_count: 50,
            }],
        }],
        facts: Vec::new(),
    }
}

// ---- MO005 tests ----

#[test]
fn mo005_flags_struct_in_main() {
    let air =
        air_with_module_and_items("locus_cli::main", vec![type_item("Cli", TypeKind::Struct)]);
    let diags = mo005(&air, CheckMode::Human);
    assert_eq!(diags.len(), 1, "expected one diag, got {diags:?}");
    assert_eq!(diags[0].rule_id, "MO005");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(
        diags[0].message.contains("struct"),
        "expected 'struct' in message; got: {}",
        diags[0].message
    );
    assert!(
        diags[0].message.contains("Cli"),
        "expected struct name in message; got: {}",
        diags[0].message
    );
}

#[test]
fn mo005_flags_enum_in_main() {
    let air = air_with_module_and_items(
        "locus_cli::main",
        vec![type_item("Command", TypeKind::Enum)],
    );
    let diags = mo005(&air, CheckMode::Human);
    assert_eq!(diags.len(), 1, "expected one diag, got {diags:?}");
    assert_eq!(diags[0].rule_id, "MO005");
    assert!(
        diags[0].message.contains("enum"),
        "expected 'enum' in message"
    );
    assert!(
        diags[0].message.contains("Command"),
        "expected enum name in message"
    );
}

#[test]
fn mo005_flags_trait_in_main() {
    let air = air_with_module_and_items("pkg::main", vec![type_item("Runner", TypeKind::Trait)]);
    let diags = mo005(&air, CheckMode::Human);
    assert_eq!(diags.len(), 1, "expected one diag, got {diags:?}");
    assert!(diags[0].message.contains("trait"));
}

#[test]
fn mo005_flags_impl_block_in_main() {
    let air = air_with_module_and_items("pkg::main", vec![impl_item("Cli")]);
    let diags = mo005(&air, CheckMode::Human);
    assert_eq!(diags.len(), 1, "expected one diag, got {diags:?}");
    assert!(diags[0].message.contains("impl block"));
    assert!(diags[0].message.contains("Cli"));
}

#[test]
fn mo005_flags_long_helper_function_in_main() {
    // Function named `helper_xyz` — not a permitted name, any line count fires.
    let air = air_with_module_and_items("pkg::main", vec![func_with_lines("helper_xyz", 10)]);
    let diags = mo005(&air, CheckMode::Human);
    assert_eq!(diags.len(), 1, "expected one diag, got {diags:?}");
    assert!(
        diags[0].message.contains("helper_xyz"),
        "expected fn name in message"
    );
}

#[test]
fn mo005_flags_named_fn_exceeding_budget_in_main() {
    // `main` is a permitted name but exceeds the line budget.
    let budget = MO005_THIN_FN_MAX_LINES;
    let air = air_with_module_and_items("pkg::main", vec![func_with_lines("main", budget + 1)]);
    let diags = mo005(&air, CheckMode::Human);
    assert_eq!(diags.len(), 1, "expected one diag, got {diags:?}");
    assert!(
        diags[0].message.contains("main"),
        "expected fn name in message"
    );
    assert!(
        diags[0].message.contains(&budget.to_string()),
        "expected budget in message"
    );
}

#[test]
fn mo005_silent_for_thin_main_fn() {
    // A small `fn main` (≤ budget lines) is allowed — classic composition glue.
    let budget = MO005_THIN_FN_MAX_LINES;
    let air = air_with_module_and_items("pkg::main", vec![func_with_lines("main", budget)]);
    let diags = mo005(&air, CheckMode::Human);
    assert!(
        diags.is_empty(),
        "expected no diags for thin main fn, got {diags:?}"
    );
}

#[test]
fn mo005_silent_for_thin_run_fn() {
    // `fn run(cli: Cli) -> Result<()> { commands::run(cli) }` — thin dispatch
    // glue; allowed.
    let air = air_with_module_and_items("pkg::main", vec![func_with_lines("run", 5)]);
    let diags = mo005(&air, CheckMode::Human);
    assert!(
        diags.is_empty(),
        "expected no diags for thin run fn, got {diags:?}"
    );
}

#[test]
fn mo005_silent_for_thin_init_fn() {
    let air = air_with_module_and_items("pkg::main", vec![func_with_lines("init", 3)]);
    let diags = mo005(&air, CheckMode::Human);
    assert!(
        diags.is_empty(),
        "expected no diags for thin init fn, got {diags:?}"
    );
}

#[test]
fn mo005_silent_for_imports_in_main() {
    // Imports are passive observations — not declarations.
    let air = air_with_module_and_items(
        "pkg::main",
        vec![
            import("anyhow::Result"),
            import("clap::Parser"),
            import("crate::cli::Cli"),
        ],
    );
    let diags = mo005(&air, CheckMode::Human);
    assert!(
        diags.is_empty(),
        "expected no diags for imports, got {diags:?}"
    );
}

#[test]
fn mo005_applies_to_lib_module() {
    // The rule also fires for `::lib` module paths.
    let air = AirWorkspace {
        schema_version: AIR_SCHEMA_VERSION,
        packages: vec![AirPackage {
            name: "x".into(),
            version: "0".into(),
            root_dir: "/".into(),
            files: vec![AirFile {
                path: "src/lib.rs".into(),
                module_path: Some("my_crate::lib".to_string()),
                items: vec![type_item("InternalHelper", TypeKind::Struct)],
                hints: Vec::new(),
                parse_error: None,
                line_count: 30,
            }],
        }],
        facts: Vec::new(),
    };
    let diags = mo005(&air, CheckMode::Human);
    assert_eq!(
        diags.len(),
        1,
        "expected one diag for struct in lib, got {diags:?}"
    );
    assert_eq!(diags[0].rule_id, "MO005");
}

#[test]
fn mo005_applies_to_mod_module() {
    // The rule also fires for `::mod` module paths.
    let air = AirWorkspace {
        schema_version: AIR_SCHEMA_VERSION,
        packages: vec![AirPackage {
            name: "x".into(),
            version: "0".into(),
            root_dir: "/".into(),
            files: vec![AirFile {
                path: "src/commands/mod.rs".into(),
                module_path: Some("my_crate::commands::mod".to_string()),
                items: vec![type_item("InternalState", TypeKind::Struct)],
                hints: Vec::new(),
                parse_error: None,
                line_count: 30,
            }],
        }],
        facts: Vec::new(),
    };
    let diags = mo005(&air, CheckMode::Human);
    assert_eq!(
        diags.len(),
        1,
        "expected one diag for struct in mod.rs, got {diags:?}"
    );
}

#[test]
fn mo005_does_not_apply_to_non_entrypoint_files() {
    // `pkg::other` does not end in `main`/`lib`/`mod`, and the file is `other.rs`
    // — rule must not fire.
    let air = air_with_module_path_and_file(
        "pkg::other",
        "other.rs",
        vec![
            type_item("Foo", TypeKind::Struct),
            type_item("Bar", TypeKind::Enum),
        ],
    );
    let diags = mo005(&air, CheckMode::Human);
    assert!(
        diags.is_empty(),
        "MO005 must not fire on non-entrypoint modules; got {diags:?}"
    );
}

#[test]
fn mo005_does_not_fire_on_module_path_containing_main_as_non_suffix() {
    // `pkg::main_loop` in `main_loop.rs` — neither the module segment nor
    // the file stem is `main` / `lib` / `mod` — must not match.
    let air = air_with_module_path_and_file(
        "pkg::main_loop",
        "main_loop.rs",
        vec![type_item("State", TypeKind::Struct)],
    );
    let diags = mo005(&air, CheckMode::Human);
    assert!(
        diags.is_empty(),
        "MO005 must not fire on `main_loop` module; got {diags:?}"
    );
}

#[test]
fn mo005_multiple_violations_emit_one_diagnostic_per_item() {
    // Three forbidden items in the same entrypoint → three diagnostics.
    let air = air_with_module_and_items(
        "locus_cli::main",
        vec![
            type_item("Cli", TypeKind::Struct),
            type_item("Command", TypeKind::Enum),
            impl_item("Cli"),
        ],
    );
    let diags = mo005(&air, CheckMode::Human);
    assert_eq!(
        diags.len(),
        3,
        "expected one diag per forbidden item; got {diags:?}"
    );
}

#[test]
fn mo005_agent_strict_elevates_to_fatal() {
    let air = air_with_module_and_items("pkg::main", vec![type_item("Foo", TypeKind::Struct)]);
    let diags = mo005(&air, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

#[test]
fn mo005_skips_files_without_module_path() {
    // Files without a module_path cannot be classified as entrypoints.
    let air = air_with_module_and_items("pkg::main", vec![type_item("Foo", TypeKind::Struct)]);
    // Manually create workspace with no module_path to test the skip.
    let air_no_path = AirWorkspace {
        schema_version: AIR_SCHEMA_VERSION,
        packages: vec![AirPackage {
            name: "x".into(),
            version: "0".into(),
            root_dir: "/".into(),
            files: vec![AirFile {
                path: "main.rs".into(),
                module_path: None,
                items: vec![type_item("Foo", TypeKind::Struct)],
                hints: Vec::new(),
                parse_error: None,
                line_count: 10,
            }],
        }],
        facts: Vec::new(),
    };
    assert!(
        mo005(&air_no_path, CheckMode::Human).is_empty(),
        "must skip files with no module_path"
    );
    // But with a module_path it fires normally.
    let diags = mo005(&air, CheckMode::Human);
    assert_eq!(diags.len(), 1);
}

#[test]
fn mo005_flags_conversion_in_entrypoint() {
    // A converter in an entrypoint module is misplaced.
    let air = air_with_module_and_items("pkg::main", vec![conversion_item("FooDto", "Foo")]);
    let diags = mo005(&air, CheckMode::Human);
    assert_eq!(diags.len(), 1, "expected conversion to fire; got {diags:?}");
    assert_eq!(diags[0].rule_id, "MO005");
}

#[test]
fn mo005_detects_binary_crate_root_by_file_path() {
    // Rust binary crate roots have module_path = crate name (no `::main`
    // suffix). MO005 must still fire because the file is `main.rs`.
    // This is the critical real-world case — `locus_cli` itself.
    let air = air_with_module_path_and_file(
        "locus_cli", // flat crate-name module path, as Rust adapter emits
        "crates/locus-cli/src/main.rs",
        vec![
            type_item("Cli", TypeKind::Struct),
            type_item("Command", TypeKind::Enum),
        ],
    );
    let diags = mo005(&air, CheckMode::Human);
    assert_eq!(
        diags.len(),
        2,
        "must detect binary-crate root via file path; got {diags:?}"
    );
    assert_eq!(diags[0].rule_id, "MO005");
}
