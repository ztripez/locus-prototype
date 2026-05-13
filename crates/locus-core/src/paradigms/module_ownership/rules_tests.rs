use super::super::lockfile_schema::{MoOverride, MoSection};
use super::*;
use locus_air::{
    AIR_SCHEMA_VERSION, AirCallSite, AirConversion, AirFunction, AirImplBlock, AirPackage, AirType,
    CallKind, ConversionMechanism, ImplDispatch, TypeKind,
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
        ..Default::default()
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
    let diags = mo005(&air, &MoSection::default(), CheckMode::Human);
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
    let diags = mo005(&air, &MoSection::default(), CheckMode::Human);
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
    let diags = mo005(&air, &MoSection::default(), CheckMode::Human);
    assert_eq!(diags.len(), 1, "expected one diag, got {diags:?}");
    assert!(diags[0].message.contains("trait"));
}

#[test]
fn mo005_flags_impl_block_in_main() {
    let air = air_with_module_and_items("pkg::main", vec![impl_item("Cli")]);
    let diags = mo005(&air, &MoSection::default(), CheckMode::Human);
    assert_eq!(diags.len(), 1, "expected one diag, got {diags:?}");
    assert!(diags[0].message.contains("impl block"));
    assert!(diags[0].message.contains("Cli"));
}

#[test]
fn mo005_flags_long_helper_function_in_main() {
    // Function named `helper_xyz` — not a permitted name, any line count fires.
    let air = air_with_module_and_items("pkg::main", vec![func_with_lines("helper_xyz", 10)]);
    let diags = mo005(&air, &MoSection::default(), CheckMode::Human);
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
    let diags = mo005(&air, &MoSection::default(), CheckMode::Human);
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
    let diags = mo005(&air, &MoSection::default(), CheckMode::Human);
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
    let diags = mo005(&air, &MoSection::default(), CheckMode::Human);
    assert!(
        diags.is_empty(),
        "expected no diags for thin run fn, got {diags:?}"
    );
}

#[test]
fn mo005_silent_for_thin_init_fn() {
    let air = air_with_module_and_items("pkg::main", vec![func_with_lines("init", 3)]);
    let diags = mo005(&air, &MoSection::default(), CheckMode::Human);
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
    let diags = mo005(&air, &MoSection::default(), CheckMode::Human);
    assert!(
        diags.is_empty(),
        "expected no diags for imports, got {diags:?}"
    );
}

#[test]
fn mo005_canonical_data_lib_rs_silent_under_heuristic() {
    // Issue #69 design pass: lib.rs is classified by lockfile entry or
    // heuristic. A single `pub struct` with zero `pub use` imports is the
    // canonical-data shape — the heuristic resolves it to `CanonicalData`
    // and MO005 stays silent. (Cargo emits a flat `module_path` for the
    // lib root, e.g. `test_pkg` with no `::lib` suffix; the basename
    // `lib.rs` is what triggers the entrypoint classification.)
    let air = AirWorkspace {
        schema_version: AIR_SCHEMA_VERSION,
        packages: vec![AirPackage {
            name: "test_pkg".into(),
            version: "0".into(),
            root_dir: "/".into(),
            files: vec![AirFile {
                path: "src/lib.rs".into(),
                module_path: Some("test_pkg".to_string()),
                items: vec![type_item("Foo", TypeKind::Struct)],
                hints: Vec::new(),
                parse_error: None,
                line_count: 30,
            }],
        }],
        facts: Vec::new(),
    };
    let diagnostics = mo005(&air, &MoSection::default(), CheckMode::Human);
    assert!(
        diagnostics.is_empty(),
        "MO005 must NOT fire on canonical-data lib.rs (heuristic: zero `pub use` \
         + ≥1 declaration). Got: {diagnostics:?}"
    );
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
    let diags = mo005(&air, &MoSection::default(), CheckMode::Human);
    assert_eq!(
        diags.len(),
        1,
        "expected one diag for struct in mod.rs, got {diags:?}"
    );
}

#[test]
fn mo005_does_not_apply_to_non_entrypoint_files() {
    // `pkg::other` does not end in `main`/`mod`, and the file is `other.rs`
    // — rule must not fire.
    let air = air_with_module_path_and_file(
        "pkg::other",
        "other.rs",
        vec![
            type_item("Foo", TypeKind::Struct),
            type_item("Bar", TypeKind::Enum),
        ],
    );
    let diags = mo005(&air, &MoSection::default(), CheckMode::Human);
    assert!(
        diags.is_empty(),
        "MO005 must not fire on non-entrypoint modules; got {diags:?}"
    );
}

#[test]
fn mo005_does_not_fire_on_module_path_containing_main_as_non_suffix() {
    // `pkg::main_loop` in `main_loop.rs` — neither the module segment nor
    // the file stem is `main` / `mod` — must not match.
    let air = air_with_module_path_and_file(
        "pkg::main_loop",
        "main_loop.rs",
        vec![type_item("State", TypeKind::Struct)],
    );
    let diags = mo005(&air, &MoSection::default(), CheckMode::Human);
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
    let diags = mo005(&air, &MoSection::default(), CheckMode::Human);
    assert_eq!(
        diags.len(),
        3,
        "expected one diag per forbidden item; got {diags:?}"
    );
}

#[test]
fn mo005_agent_strict_elevates_to_fatal() {
    let air = air_with_module_and_items("pkg::main", vec![type_item("Foo", TypeKind::Struct)]);
    let diags = mo005(&air, &MoSection::default(), CheckMode::AgentStrict);
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
        mo005(&air_no_path, &MoSection::default(), CheckMode::Human).is_empty(),
        "must skip files with no module_path"
    );
    // But with a module_path it fires normally.
    let diags = mo005(&air, &MoSection::default(), CheckMode::Human);
    assert_eq!(diags.len(), 1);
}

#[test]
fn mo005_flags_conversion_in_entrypoint() {
    // A converter in an entrypoint module is misplaced.
    let air = air_with_module_and_items("pkg::main", vec![conversion_item("FooDto", "Foo")]);
    let diags = mo005(&air, &MoSection::default(), CheckMode::Human);
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
    let diags = mo005(&air, &MoSection::default(), CheckMode::Human);
    assert_eq!(
        diags.len(),
        2,
        "must detect binary-crate root via file path; got {diags:?}"
    );
    assert_eq!(diags[0].rule_id, "MO005");
}

// ---- MO005 composition-host helpers ----

/// Build a unit struct `AirItem` (no fields) for use in composition-host tests.
fn unit_struct_item(name: &str) -> AirItem {
    AirItem::Type(AirType {
        kind: TypeKind::Struct,
        name: name.into(),
        symbol: format!("x::{name}"),
        visibility: Visibility::Public,
        fields: Vec::new(),
        variants: Vec::new(),
        decorators: Vec::new(),
        symbol_segments: Vec::new(),
        span: AirSpan::new("mod.rs", 1, 1),
        doc: None,
    })
}

/// Build a struct WITH fields (non-unit) for negative tests.
fn struct_with_fields(name: &str) -> AirItem {
    AirItem::Type(AirType {
        kind: TypeKind::Struct,
        name: name.into(),
        symbol: format!("x::{name}"),
        visibility: Visibility::Public,
        fields: vec![locus_air::AirField {
            name: "state".into(),
            type_text: "u32".into(),
            visibility: Visibility::Private,
        }],
        variants: Vec::new(),
        decorators: Vec::new(),
        symbol_segments: Vec::new(),
        span: AirSpan::new("mod.rs", 1, 1),
        doc: None,
    })
}

/// Build a thin `impl Paradigm for <target>` block (within the line budget).
fn paradigm_impl_thin(target: &str) -> AirItem {
    AirItem::Impl(AirImplBlock {
        interface: Some("Paradigm".into()),
        target_type: target.into(),
        method_names: vec![
            "name".into(),
            "rule_prefix".into(),
            "init".into(),
            "check".into(),
        ],
        dispatch: ImplDispatch::Static,
        // 40 lines — well within the 120-line budget.
        span: AirSpan::new("mod.rs", 10, 50),
    })
}

/// Build a `impl Paradigm for <target>` block that exceeds the line budget.
fn paradigm_impl_over_budget(target: &str) -> AirItem {
    AirItem::Impl(AirImplBlock {
        interface: Some("Paradigm".into()),
        target_type: target.into(),
        method_names: vec!["check".into()],
        dispatch: ImplDispatch::Static,
        // 150 lines — exceeds MO005_COMPOSITION_HOST_IMPL_MAX_LINES (120).
        span: AirSpan::new("mod.rs", 10, 160),
    })
}

/// Build an impl block that does NOT implement Paradigm.
fn non_paradigm_impl(trait_name: &str, target: &str) -> AirItem {
    AirItem::Impl(AirImplBlock {
        interface: Some(trait_name.into()),
        target_type: target.into(),
        method_names: vec!["do_thing".into()],
        dispatch: ImplDispatch::Static,
        span: AirSpan::new("mod.rs", 10, 30),
    })
}

/// Build an inherent (no-trait) impl block for a target.
fn inherent_impl(target: &str) -> AirItem {
    AirItem::Impl(AirImplBlock {
        interface: None,
        target_type: target.into(),
        method_names: vec!["new".into()],
        dispatch: ImplDispatch::Static,
        span: AirSpan::new("mod.rs", 10, 20),
    })
}

/// Build a workspace with a `main.rs` file at the given module path containing
/// the supplied items.
fn main_rs_air(module_path: &str, items: Vec<AirItem>) -> AirWorkspace {
    AirWorkspace {
        schema_version: AIR_SCHEMA_VERSION,
        packages: vec![AirPackage {
            name: "x".into(),
            version: "0".into(),
            root_dir: "/".into(),
            files: vec![AirFile {
                path: "src/main.rs".into(),
                module_path: Some(module_path.to_string()),
                items,
                hints: Vec::new(),
                parse_error: None,
                line_count: 200,
            }],
        }],
        facts: Vec::new(),
    }
}

/// Build a workspace with a `mod.rs` file at the given module path containing
/// the supplied items.
fn mod_rs_air(module_path: &str, items: Vec<AirItem>) -> AirWorkspace {
    AirWorkspace {
        schema_version: AIR_SCHEMA_VERSION,
        packages: vec![AirPackage {
            name: "x".into(),
            version: "0".into(),
            root_dir: "/".into(),
            files: vec![AirFile {
                path: "src/something/mod.rs".into(),
                module_path: Some(module_path.to_string()),
                items,
                hints: Vec::new(),
                parse_error: None,
                line_count: 200,
            }],
        }],
        facts: Vec::new(),
    }
}

// ---- MO005 composition-host tests ----

#[test]
fn mo005_silent_for_composition_host_pattern() {
    // pub struct Foo; + impl Paradigm for Foo with thin methods in mod.rs.
    // Both the unit struct and the impl must be allowed — 0 MO005 hits.
    let items = vec![
        unit_struct_item("MyParadigm"),
        paradigm_impl_thin("MyParadigm"),
    ];
    let air = mod_rs_air("my_crate::paradigms::my_paradigm::mod", items);
    let diags = mo005(&air, &MoSection::default(), CheckMode::Human);
    assert!(
        diags.is_empty(),
        "composition-host pattern (unit struct + thin impl Paradigm) must be silent; \
         got {diags:?}"
    );
}

#[test]
fn mo005_fires_for_non_paradigm_impl_in_mod_rs() {
    // pub struct Foo; + impl SomeOtherTrait for Foo in mod.rs.
    // The impl does not implement Paradigm — must fire.
    let items = vec![
        unit_struct_item("Foo"),
        non_paradigm_impl("SomeOtherTrait", "Foo"),
    ];
    let air = mod_rs_air("x::foo::mod", items);
    let diags = mo005(&air, &MoSection::default(), CheckMode::Human);
    // Both the struct (now without a matching Paradigm impl) and the non-Paradigm
    // impl should fire.
    assert!(
        !diags.is_empty(),
        "non-Paradigm impl in mod.rs must fire; got {diags:?}"
    );
    assert!(
        diags.iter().any(|d| d.message.contains("impl block")),
        "expected impl-block diagnostic; got {diags:?}"
    );
}

#[test]
fn mo005_fires_for_paradigm_impl_with_long_method_in_mod_rs() {
    // pub struct Foo; + impl Paradigm for Foo with a check() method that
    // makes the impl block exceed 120 lines.
    let items = vec![
        unit_struct_item("BigParadigm"),
        paradigm_impl_over_budget("BigParadigm"),
    ];
    let air = mod_rs_air("x::paradigms::big::mod", items);
    let diags = mo005(&air, &MoSection::default(), CheckMode::Human);
    assert!(
        !diags.is_empty(),
        "impl Paradigm with total span >120 lines must fire; got {diags:?}"
    );
    assert!(
        diags.iter().any(|d| d.message.contains("impl block")),
        "expected impl-block diagnostic; got {diags:?}"
    );
}

#[test]
fn mo005_fires_for_paradigm_impl_on_non_local_target() {
    // impl Paradigm for ForeignType in mod.rs, but ForeignType is NOT
    // declared in this file (not in same_file_items as a unit struct).
    let items = vec![paradigm_impl_thin("ForeignType")];
    let air = mod_rs_air("x::paradigms::mod", items);
    let diags = mo005(&air, &MoSection::default(), CheckMode::Human);
    assert!(
        !diags.is_empty(),
        "impl Paradigm on non-local type must fire (local unit struct not found); \
         got {diags:?}"
    );
    assert!(
        diags.iter().any(|d| d.message.contains("impl block")),
        "expected impl-block diagnostic; got {diags:?}"
    );
}

#[test]
fn mo005_fires_for_paradigm_impl_on_non_unit_struct() {
    // pub struct Foo { state: u32 } + impl Paradigm for Foo (thin methods).
    // The host is not a unit struct — composition-host pattern requires unit struct.
    let items = vec![struct_with_fields("Foo"), paradigm_impl_thin("Foo")];
    let air = mod_rs_air("x::paradigms::mod", items);
    let diags = mo005(&air, &MoSection::default(), CheckMode::Human);
    assert!(
        !diags.is_empty(),
        "impl Paradigm on non-unit struct must fire; got {diags:?}"
    );
    assert!(
        diags.iter().any(|d| d.message.contains("impl block")),
        "expected impl-block diagnostic; got {diags:?}"
    );
}

#[test]
fn mo005_silent_for_unit_struct_paired_with_paradigm_impl() {
    // The host unit struct itself must not be flagged when its impl Paradigm
    // is allowed. This test verifies the struct-level silent pass.
    let items = vec![unit_struct_item("Host"), paradigm_impl_thin("Host")];
    let air = mod_rs_air("x::paradigms::my::mod", items);
    let diags = mo005(&air, &MoSection::default(), CheckMode::Human);
    assert!(
        diags.is_empty(),
        "host unit struct paired with allowed impl Paradigm must not be flagged; \
         got {diags:?}"
    );
}

#[test]
fn mo005_arbitrary_impl_in_mod_rs_still_fires() {
    // An inherent impl block (no trait) for a locally-declared struct in mod.rs
    // is still forbidden — the composition-host exception does not apply.
    let items = vec![unit_struct_item("State"), inherent_impl("State")];
    let air = mod_rs_air("x::paradigms::mod", items);
    let diags = mo005(&air, &MoSection::default(), CheckMode::Human);
    assert!(
        !diags.is_empty(),
        "inherent impl block in mod.rs must still fire; got {diags:?}"
    );
    assert!(
        diags.iter().any(|d| d.message.contains("impl block")),
        "expected impl-block diagnostic; got {diags:?}"
    );
}

#[test]
fn mo005_composition_host_pattern_in_main_rs_still_fires() {
    // pub struct Foo; + impl Paradigm for Foo (thin) in main.rs — NOT mod.rs.
    // The composition-host exception is a mod.rs-only convention: main.rs is
    // the binary entrypoint and must have zero impl blocks regardless of
    // trait, target, or method size. MO005 must fire on both the struct and
    // the impl.
    let items = vec![
        unit_struct_item("MyParadigm"),
        paradigm_impl_thin("MyParadigm"),
    ];
    let air = main_rs_air("my_crate::main", items);
    let diags = mo005(&air, &MoSection::default(), CheckMode::Human);
    assert!(
        !diags.is_empty(),
        "composition-host pattern in main.rs must fire MO005 — \
         the exception is mod.rs-only; got {diags:?}"
    );
    assert!(
        diags.iter().any(|d| d.message.contains("struct")),
        "expected struct diagnostic for unit struct in main.rs; got {diags:?}"
    );
    assert!(
        diags.iter().any(|d| d.message.contains("impl block")),
        "expected impl-block diagnostic for impl Paradigm in main.rs; got {diags:?}"
    );
}

// ---- MO005 lib.rs classification tests (issue #69) ----

/// Build a `lib.rs` workspace at the given crate-level `module_path`
/// containing the supplied items.
fn lib_rs_air(module_path: &str, items: Vec<AirItem>) -> AirWorkspace {
    AirWorkspace {
        schema_version: AIR_SCHEMA_VERSION,
        packages: vec![AirPackage {
            name: "x".into(),
            version: "0".into(),
            root_dir: "/".into(),
            files: vec![AirFile {
                path: "src/lib.rs".into(),
                module_path: Some(module_path.to_string()),
                items,
                hints: Vec::new(),
                parse_error: None,
                line_count: 200,
            }],
        }],
        facts: Vec::new(),
    }
}

fn pub_use_import(path: &str) -> AirItem {
    AirItem::Import(locus_air::AirImport {
        path: path.into(),
        path_segments: Vec::new(),
        visibility: Visibility::Public,
        span: AirSpan::new("src/lib.rs", 1, 1),
    })
}

#[test]
fn mo005_lib_rs_thin_reexport_shape_silent() {
    // Only `pub use` re-exports, no declarations → thin-reexport shape.
    // The heuristic recognises D == 0 and stays silent.
    let air = lib_rs_air(
        "thin_pkg",
        vec![
            pub_use_import("foo::Foo"),
            pub_use_import("bar::Bar"),
            pub_use_import("baz::Baz"),
        ],
    );
    let diags = mo005(&air, &MoSection::default(), CheckMode::Human);
    assert!(
        diags.is_empty(),
        "thin re-export lib.rs (only `pub use`) must be silent; got {diags:?}"
    );
}

#[test]
fn mo005_lib_rs_canonical_data_shape_silent() {
    // Many `pub` declarations, zero `pub use` → canonical-data shape.
    // Even at 30 public types (like locus-air), MO005 stays silent.
    let mut items = Vec::new();
    for i in 0..30 {
        items.push(type_item(&format!("T{i}"), TypeKind::Struct));
    }
    let air = lib_rs_air("locus_air", items);
    let diags = mo005(&air, &MoSection::default(), CheckMode::Human);
    assert!(
        diags.is_empty(),
        "canonical-data lib.rs (R=0, D>0) must be silent; got {} diags",
        diags.len()
    );
}

#[test]
fn mo005_lib_rs_small_composition_root_silent() {
    // `pub use` + a few `pub` declarations (D_pub ≤ budget=5) → small
    // composition root. Mirrors locus-rust's shape (1 pub enum + 3 pub fns
    // + 5 pub uses).
    let mut items = vec![
        pub_use_import("hints::scan_hints"),
        pub_use_import("loaders::MarkersLoader"),
        type_item("ScanError", TypeKind::Enum),
        func_with_lines("scan", 5),
        func_with_lines("scan_raw", 32),
        func_with_lines("default_loaders", 7),
    ];
    // Add a private function for realism — should not be counted.
    items.push(AirItem::Function(AirFunction {
        name: "apply_default_loaders".into(),
        symbol: "x::apply_default_loaders".into(),
        visibility: Visibility::Private,
        params: Vec::new(),
        return_type: None,
        span: AirSpan::new("src/lib.rs", 100, 110),
        line_count: 10,
        decorators: Vec::new(),
        symbol_segments: Vec::new(),
        doc: None,
    }));
    let air = lib_rs_air("locus_rust", items);
    let diags = mo005(&air, &MoSection::default(), CheckMode::Human);
    assert!(
        diags.is_empty(),
        "small composition-root lib.rs (R>0, D_pub ≤ {}) must be silent; got {diags:?}",
        super::LIB_RS_COMPOSITION_ROOT_DECL_BUDGET
    );
}

#[test]
fn mo005_lib_rs_god_module_shape_fires() {
    // `pub use` + many public declarations (D_pub > budget) → accidental
    // god module. Every declaration is flagged.
    let mut items = vec![pub_use_import("foo::Foo"), pub_use_import("bar::Bar")];
    // 6 public types exceeds budget of 5.
    for i in 0..6 {
        items.push(type_item(&format!("T{i}"), TypeKind::Struct));
    }
    let air = lib_rs_air("god_pkg", items);
    let diags = mo005(&air, &MoSection::default(), CheckMode::Human);
    assert_eq!(
        diags.len(),
        6,
        "god-module lib.rs (R>0, D_pub > budget) must fire on every \
         declaration; got {diags:?}"
    );
    assert!(
        diags.iter().all(|d| d
            .suggested_fix
            .as_deref()
            .unwrap_or("")
            .contains("paradigms.MO.lib_rs_kinds")),
        "lib.rs diagnostics must mention the lockfile escape hatch in \
         their suggested_fix; got {:?}",
        diags
            .iter()
            .map(|d| d.suggested_fix.clone())
            .collect::<Vec<_>>()
    );
}

#[test]
fn mo005_lib_rs_lockfile_canonical_data_skips_rule_entirely() {
    // Explicit lockfile entry → MO005 skips the file regardless of the
    // heuristic's shape. Even a god-module shape (R>0, D>>budget) is
    // silenced because the user has accepted the canonical-data kind.
    use super::super::lockfile_schema::{LibRsKind, LibRsKindEntry};
    let mut items = vec![pub_use_import("foo::Foo")];
    for i in 0..20 {
        items.push(type_item(&format!("T{i}"), TypeKind::Struct));
    }
    let air = lib_rs_air("flat_pkg", items);
    let section = MoSection {
        lib_rs_kinds: vec![LibRsKindEntry {
            module: "flat_pkg".into(),
            kind: LibRsKind::CanonicalData,
            reason: Some("intentional flat data contract".into()),
            ..Default::default()
        }],
        ..Default::default()
    };
    let diags = mo005(&air, &section, CheckMode::Human);
    assert!(
        diags.is_empty(),
        "lockfile lib_rs_kinds=canonical-data must silence MO005 entirely; \
         got {diags:?}"
    );
}

#[test]
fn mo005_lib_rs_lockfile_composition_root_skips_rule_entirely() {
    // `composition-root` kind also skips MO005 — the user has accepted
    // declarations + glue at the crate root. MO001/MO002 still apply.
    use super::super::lockfile_schema::{LibRsKind, LibRsKindEntry};
    let mut items = vec![pub_use_import("foo::Foo")];
    for i in 0..10 {
        items.push(type_item(&format!("T{i}"), TypeKind::Struct));
    }
    let air = lib_rs_air("comp_pkg", items);
    let section = MoSection {
        lib_rs_kinds: vec![LibRsKindEntry {
            module: "comp_pkg".into(),
            kind: LibRsKind::CompositionRoot,
            ..Default::default()
        }],
        ..Default::default()
    };
    let diags = mo005(&air, &section, CheckMode::Human);
    assert!(
        diags.is_empty(),
        "lockfile lib_rs_kinds=composition-root must silence MO005; got {diags:?}"
    );
}

#[test]
fn mo005_lib_rs_lockfile_thin_reexport_forces_strict_scoping() {
    // Explicit `thin-reexport` overrides the canonical-data heuristic: a
    // lib.rs with declarations and zero `pub use` is normally treated as
    // canonical-data, but the user has asked for main.rs-style scoping.
    use super::super::lockfile_schema::{LibRsKind, LibRsKindEntry};
    let air = lib_rs_air("strict_pkg", vec![type_item("Foo", TypeKind::Struct)]);
    let section = MoSection {
        lib_rs_kinds: vec![LibRsKindEntry {
            module: "strict_pkg".into(),
            kind: LibRsKind::ThinReexport,
            reason: Some("crate root must remain a thin surface".into()),
            ..Default::default()
        }],
        ..Default::default()
    };
    let diags = mo005(&air, &section, CheckMode::Human);
    assert_eq!(
        diags.len(),
        1,
        "lockfile lib_rs_kinds=thin-reexport must enforce main.rs scoping \
         (canonical-data heuristic does NOT apply); got {diags:?}"
    );
    assert!(diags[0].message.contains("Foo"));
}

#[test]
fn mo005_lib_rs_empty_file_silent() {
    // Empty `lib.rs` (e.g. a placeholder stub crate) — D == 0, R == 0 →
    // thin re-export. No diagnostics.
    let air = lib_rs_air("locus_report", Vec::new());
    let diags = mo005(&air, &MoSection::default(), CheckMode::Human);
    assert!(
        diags.is_empty(),
        "empty lib.rs must be silent; got {diags:?}"
    );
}

#[test]
fn mo005_lib_rs_detection_via_basename_not_module_path() {
    // Cargo emits a flat `module_path` for the lib root (e.g.
    // `my_pkg`, no `::lib` suffix). Detection must come from the file
    // basename, not the module path.
    let air = AirWorkspace {
        schema_version: AIR_SCHEMA_VERSION,
        packages: vec![AirPackage {
            name: "my_pkg".into(),
            version: "0".into(),
            root_dir: "/".into(),
            files: vec![AirFile {
                path: "crates/my-pkg/src/lib.rs".into(),
                module_path: Some("my_pkg".to_string()),
                items: vec![
                    pub_use_import("foo::Foo"),
                    // 6 public types — over budget, would fire if classified
                    // as a lib.rs entrypoint.
                    type_item("T0", TypeKind::Struct),
                    type_item("T1", TypeKind::Struct),
                    type_item("T2", TypeKind::Struct),
                    type_item("T3", TypeKind::Struct),
                    type_item("T4", TypeKind::Struct),
                    type_item("T5", TypeKind::Struct),
                ],
                hints: Vec::new(),
                parse_error: None,
                line_count: 100,
            }],
        }],
        facts: Vec::new(),
    };
    let diags = mo005(&air, &MoSection::default(), CheckMode::Human);
    assert_eq!(
        diags.len(),
        6,
        "lib.rs detection via basename (not module-path suffix) must \
         classify this file and fire on each over-budget declaration; \
         got {diags:?}"
    );
}

#[test]
fn mo005_lib_rs_agent_strict_elevates_to_fatal() {
    // The strict elevation works the same as for main.rs.
    let mut items = vec![pub_use_import("foo::Foo")];
    for i in 0..6 {
        items.push(type_item(&format!("T{i}"), TypeKind::Struct));
    }
    let air = lib_rs_air("god_pkg", items);
    let diags = mo005(&air, &MoSection::default(), CheckMode::AgentStrict);
    assert_eq!(diags.len(), 6);
    assert!(diags.iter().all(|d| d.severity == Severity::Fatal));
}
