use super::super::lockfile_schema::{CxOverride, CxSection};
use super::*;
use locus_air::{
    AIR_SCHEMA_VERSION, AirCallSite, AirFile, AirFunction, AirPackage, AirSpan, AirType, CallKind,
    TypeKind, Visibility,
};

// CX001 migrated to RuleDefinition (#71 P2). Tests call this helper
// instead of the legacy `cx001()` function; helper constructs the
// RuleContext + lockfile shape that `Cx001Rule::observe` expects.
use crate::governance::evidence::Evidence;
use crate::governance::finding::RuleFinding;
use crate::governance::ids::{FindingIdMinter, RuleId};
use crate::governance::registry::{ParadigmRegistry, RuleRegistry};
use crate::governance::rule::{RuleContext, RuleDefinition};
use crate::lockfile::Lockfile;
use crate::paradigms::complexity_budget::rules::cx001::Cx001Rule;

fn observe_cx001(
    air: &locus_air::AirWorkspace,
    section: &CxSection,
    mode: CheckMode,
) -> Vec<RuleFinding> {
    let mut lf = Lockfile::default();
    lf.paradigms.insert(
        "CX".to_string(),
        serde_json::to_value(section).expect("CxSection must serialize"),
    );
    let rules = RuleRegistry::standard();
    let paradigms = ParadigmRegistry::empty();
    let minter = FindingIdMinter::new();
    let ctx = RuleContext {
        air,
        lockfile: &lf,
        mode,
        rule_registry: &rules,
        paradigm_registry: &paradigms,
        finding_ids: &minter,
    };
    Cx001Rule.observe(&ctx)
}

fn func(name: &str, line_count: u32) -> AirItem {
    AirItem::Function(AirFunction {
        name: name.into(),
        symbol: format!("x::{name}"),
        visibility: Visibility::Public,
        params: Vec::new(),
        return_type: None,
        span: AirSpan::new("t.rs", 1, line_count.max(1)),
        line_count,
        decorators: Vec::new(),
        symbol_segments: Vec::new(),
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

fn configured(default_budget: u32) -> CxSection {
    CxSection {
        default_max_function_lines: Some(default_budget),
        overrides: Vec::new(),
        ..CxSection::default()
    }
}

#[test]
fn cx001_fires_with_built_in_fallback_on_default_section() {
    // Default section uses DEFAULT_MAX_FUNCTION_LINES (50) as the
    // budget. A 500-line function trips it without any user
    // configuration — the rule fires by default per the
    // "noisy-by-default, configuration narrows" convention.
    let air = air_with(Some("foo::bar"), vec![func("big", 500)]);
    let section = CxSection::default();
    let findings = observe_cx001(&air, &section, CheckMode::Human);
    assert_eq!(findings.len(), 1, "expected one finding, got {findings:?}");
    assert!(
        findings[0].why.iter().any(|w| w.contains("built-in fallback")),
        "expected built-in fallback explanation in why; got {:?}",
        findings[0].why,
    );
    match &findings[0].evidence[0] {
        Evidence::ComplexityBudget { lines, budget, override_match } => {
            assert_eq!(*lines, 500);
            assert_eq!(*budget, 50);
            assert_eq!(*override_match, None);
        }
        other => panic!("expected ComplexityBudget evidence, got {other:?}"),
    }
}

#[test]
fn cx001_quiet_when_function_within_built_in_fallback() {
    // 30-line function under the 50-line built-in fallback → no finding.
    let air = air_with(Some("foo::bar"), vec![func("small", 30)]);
    let section = CxSection::default();
    assert!(observe_cx001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn cx001_fires_when_line_count_exceeds_default_budget() {
    // 60 lines under default budget of 50 → fires.
    let air = air_with(Some("foo::bar"), vec![func("big", 60)]);
    let section = configured(50);
    let findings = observe_cx001(&air, &section, CheckMode::Human);
    assert_eq!(findings.len(), 1, "expected one finding, got {findings:?}");
    assert_eq!(findings[0].rule_id, Some(RuleId::new("CX001")));
    assert_eq!(findings[0].default_severity, Severity::Warning);
    assert!(findings[0].message.contains("x::big"));
    assert!(findings[0].message.contains("60"));
    assert!(findings[0].message.contains("budget 50"));
    match &findings[0].evidence[0] {
        Evidence::ComplexityBudget { lines, budget, override_match } => {
            assert_eq!(*lines, 60);
            assert_eq!(*budget, 50);
            assert_eq!(*override_match, None);
        }
        other => panic!("expected ComplexityBudget evidence, got {other:?}"),
    }
}

#[test]
fn cx001_quiet_when_line_count_at_or_below_budget() {
    let section = configured(50);
    // exactly at budget
    let air = air_with(Some("foo::bar"), vec![func("ok", 50)]);
    assert!(observe_cx001(&air, &section, CheckMode::Human).is_empty());
    // under budget
    let air = air_with(Some("foo::bar"), vec![func("tiny", 10)]);
    assert!(observe_cx001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn cx001_override_raises_budget_effectively() {
    // Default 50; parser function is 120 lines, override gives 200.
    let air = air_with(Some("lore::parser::expr"), vec![func("parse_expr", 120)]);
    let section = CxSection {
        default_max_function_lines: Some(50),
        overrides: vec![CxOverride {
            module: "lore::parser::*".into(),
            max_function_lines: 200,
            ..Default::default()
        }],
        ..CxSection::default()
    };
    assert!(
        observe_cx001(&air, &section, CheckMode::Human).is_empty(),
        "override should raise budget above the function's line count"
    );
}

#[test]
fn cx001_override_lowers_budget_effectively() {
    // Default 50; converter function is 40 lines (within default). Override
    // lowers the converter budget to 20 → fires.
    let air = air_with(Some("lore::convert::user"), vec![func("to_dto", 40)]);
    let section = CxSection {
        default_max_function_lines: Some(50),
        overrides: vec![CxOverride {
            module: "lore::convert::*".into(),
            max_function_lines: 20,
            ..Default::default()
        }],
        ..CxSection::default()
    };
    let findings = observe_cx001(&air, &section, CheckMode::Human);
    assert_eq!(findings.len(), 1, "override should lower budget below count");
    assert_eq!(findings[0].rule_id, Some(RuleId::new("CX001")));
    assert!(findings[0].message.contains("budget 20"));
    assert!(
        findings[0]
            .why
            .iter()
            .any(|w| w.contains("override") && w.contains("lore::convert::*")),
        "expected override mention in `why`; got {:?}",
        findings[0].why
    );
    match &findings[0].evidence[0] {
        Evidence::ComplexityBudget { lines, budget, override_match } => {
            assert_eq!(*lines, 40);
            assert_eq!(*budget, 20);
            assert_eq!(*override_match, Some("lore::convert::*".to_string()));
        }
        other => panic!("expected ComplexityBudget evidence, got {other:?}"),
    }
}

#[test]
fn cx001_agent_strict_elevates_to_fatal() {
    let air = air_with(Some("foo::bar"), vec![func("big", 60)]);
    let section = configured(50);
    let findings = observe_cx001(&air, &section, CheckMode::AgentStrict);
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].default_severity,
        Severity::Fatal,
        "agent-strict should elevate Warning to Fatal"
    );
    match &findings[0].evidence[0] {
        Evidence::ComplexityBudget { lines, budget, override_match } => {
            assert_eq!(*lines, 60);
            assert_eq!(*budget, 50);
            assert_eq!(*override_match, None);
        }
        other => panic!("expected ComplexityBudget evidence, got {other:?}"),
    }
}

/// Advisory-tier elevation: under `--agent-strict` the rule stays
/// Warning when the user hasn't narrowed it (default section, no
/// workspace budget, no per-module override). Built-in fallback alone
/// is a smoke alarm, not a CI blocker. See `CheckMode::elevate_when_actionable`
/// and issue #6.
#[test]
fn cx001_agent_strict_stays_warning_when_using_built_in_fallback() {
    let air = air_with(Some("foo::bar"), vec![func("big", 500)]);
    let section = CxSection::default();
    let findings = observe_cx001(&air, &section, CheckMode::AgentStrict);
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].default_severity,
        Severity::Warning,
        "un-narrowed advisory rule stays Warning under agent-strict; \
         user must declare a budget before this becomes a CI blocker",
    );
    match &findings[0].evidence[0] {
        Evidence::ComplexityBudget { lines, budget, override_match } => {
            assert_eq!(*lines, 500);
            assert_eq!(*budget, 50);
            assert_eq!(*override_match, None);
        }
        other => panic!("expected ComplexityBudget evidence, got {other:?}"),
    }
}

/// Once the user has set a workspace default, the rule is "narrowed" —
/// they've explicitly opted into the budget. Agent-strict should
/// elevate to Fatal at that point.
#[test]
fn cx001_agent_strict_elevates_when_workspace_default_set() {
    let air = air_with(Some("foo::bar"), vec![func("big", 60)]);
    let section = CxSection {
        default_max_function_lines: Some(50),
        ..CxSection::default()
    };
    let findings = observe_cx001(&air, &section, CheckMode::AgentStrict);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].default_severity, Severity::Fatal);
    match &findings[0].evidence[0] {
        Evidence::ComplexityBudget { lines, budget, override_match } => {
            assert_eq!(*lines, 60);
            assert_eq!(*budget, 50);
            assert_eq!(*override_match, None);
        }
        other => panic!("expected ComplexityBudget evidence, got {other:?}"),
    }
}

/// Per-module override is also a "narrowed" signal — the user has
/// made an explicit budget decision for this module path, so
/// agent-strict should elevate.
#[test]
fn cx001_agent_strict_elevates_when_module_override_matches() {
    use super::super::lockfile_schema::CxOverride;
    let air = air_with(Some("foo::bar"), vec![func("big", 200)]);
    let section = CxSection {
        // No workspace default; only a per-module override.
        default_max_function_lines: None,
        overrides: vec![CxOverride {
            module: "foo::*".into(),
            max_function_lines: 100,
            ..Default::default()
        }],
        ..CxSection::default()
    };
    let findings = observe_cx001(&air, &section, CheckMode::AgentStrict);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].default_severity, Severity::Fatal);
    match &findings[0].evidence[0] {
        Evidence::ComplexityBudget { lines, budget, override_match } => {
            assert_eq!(*lines, 200);
            assert_eq!(*budget, 100);
            assert_eq!(*override_match, Some("foo::*".to_string()));
        }
        other => panic!("expected ComplexityBudget evidence, got {other:?}"),
    }
}

fn air_with_lines(module: Option<&str>, line_count: u32) -> AirWorkspace {
    AirWorkspace {
        schema_version: AIR_SCHEMA_VERSION,
        packages: vec![AirPackage {
            name: "x".into(),
            version: "0".into(),
            root_dir: "/".into(),
            files: vec![AirFile {
                path: "t.rs".into(),
                module_path: module.map(str::to_string),
                items: Vec::new(),
                hints: Vec::new(),
                parse_error: None,
                line_count,
            }],
        }],
        facts: Vec::new(),
    }
}

#[test]
fn cx002_fires_with_built_in_fallback_on_default_section() {
    let air = air_with_lines(Some("foo::bar"), 5_000);
    let section = CxSection::default();
    let diags = cx002(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Warning);
}

/// Advisory-tier elevation: CX002 stays Warning under agent-strict
/// when no workspace default and no per-module override are set.
#[test]
fn cx002_agent_strict_stays_warning_when_using_built_in_fallback() {
    let air = air_with_lines(Some("foo::bar"), 5_000);
    let section = CxSection::default();
    let diags = cx002(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(
        diags[0].severity,
        Severity::Warning,
        "un-narrowed advisory rule stays Warning under agent-strict",
    );
}

#[test]
fn cx002_agent_strict_elevates_when_workspace_default_set() {
    let air = air_with_lines(Some("foo::bar"), 1_000);
    let section = CxSection {
        default_max_module_lines: Some(500),
        ..CxSection::default()
    };
    let diags = cx002(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

#[test]
fn cx002_agent_strict_elevates_when_module_override_matches() {
    use super::super::lockfile_schema::CxModuleOverride;
    let air = air_with_lines(Some("foo::bar"), 1_500);
    let section = CxSection {
        default_max_module_lines: None,
        module_overrides: vec![CxModuleOverride {
            module: "foo::*".into(),
            max_module_lines: 1_000,
            ..Default::default()
        }],
        ..CxSection::default()
    };
    let diags = cx002(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

#[test]
fn cx001_skips_files_without_module_path() {
    // No module_path → can't apply overrides → skip entirely.
    let air = air_with(None, vec![func("big", 500)]);
    let section = configured(50);
    assert!(observe_cx001(&air, &section, CheckMode::Human).is_empty());
}

// --- CX007 fixtures + tests ----------------------------------------

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

fn priv_fn(name: &str) -> AirItem {
    AirItem::Function(AirFunction {
        name: name.into(),
        symbol: format!("x::{name}"),
        visibility: Visibility::Private,
        params: Vec::new(),
        return_type: None,
        span: AirSpan::new("t.rs", 1, 1),
        line_count: 1,
        decorators: Vec::new(),
        symbol_segments: Vec::new(),
        doc: None,
    })
}

fn cx007_section(max: u32, exempt: Vec<&str>) -> CxSection {
    use super::super::lockfile_schema::CxExemptPathEntry;
    CxSection {
        max_public_items: max,
        exempt_paths: exempt
            .into_iter()
            .map(|s| CxExemptPathEntry::Legacy(s.to_string()))
            .collect(),
        ..CxSection::default()
    }
}

#[test]
fn cx007_quiet_when_public_count_at_or_below_budget() {
    // 3 public items, budget 5 → silent. Both at-budget and under-budget.
    let air = air_with(
        Some("x::core"),
        vec![pub_type("A"), pub_type("B"), pub_type("C")],
    );
    let section = cx007_section(5, vec![]);
    assert!(cx007(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn cx007_fires_when_public_count_exceeds_budget() {
    // 4 public items vs budget 3 → one diag.
    let items = vec![
        pub_type("A"),
        pub_type("B"),
        pub_type("C"),
        func("d", 5), // public by default in our `func` helper
    ];
    let air = air_with(Some("x::core"), items);
    let section = cx007_section(3, vec![]);
    let diags = cx007(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1, "got {diags:?}");
    assert_eq!(diags[0].rule_id, "CX007");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(diags[0].message.contains("x::core"));
    assert!(diags[0].message.contains("4"));
    assert!(diags[0].message.contains("budget 3"));
}

#[test]
fn cx007_only_counts_public_items() {
    // 2 public + 5 private = total 7, but only public counts → silent at budget 3.
    let items = vec![
        pub_type("A"),
        pub_type("B"),
        priv_type("p1"),
        priv_type("p2"),
        priv_type("p3"),
        priv_fn("hidden1"),
        priv_fn("hidden2"),
    ];
    let air = air_with(Some("x::core"), items);
    let section = cx007_section(3, vec![]);
    assert!(cx007(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn cx007_exempt_paths_silence_diagnostic() {
    // 5 public items, budget 3, but module matches `*::tests::*` exempt → silent.
    let items = vec![
        pub_type("A"),
        pub_type("B"),
        pub_type("C"),
        pub_type("D"),
        pub_type("E"),
    ];
    let air = air_with(Some("x::tests::helpers"), items);
    let section = cx007_section(3, vec!["*::tests::*"]);
    assert!(cx007(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn cx007_default_exempt_paths_cover_test_modules() {
    // Default section ships with `*::tests::*` and `*::test::*` exempts.
    let items = (0..40)
        .map(|i| pub_type(&format!("T{i}")))
        .collect::<Vec<_>>();
    let air = air_with(Some("x::tests::big"), items);
    let section = CxSection::default();
    assert!(cx007(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn cx007_agent_strict_elevates_to_fatal() {
    let items = vec![pub_type("A"), pub_type("B"), pub_type("C"), pub_type("D")];
    let air = air_with(Some("x::core"), items);
    let section = cx007_section(3, vec![]);
    let diags = cx007(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

// --- CX008 fixtures + tests ----------------------------------------

fn callsite(callee: &str, in_function: &str) -> AirItem {
    AirItem::CallSite(AirCallSite {
        callee: callee.into(),
        kind: CallKind::Function,
        function: Some(in_function.into()),
        span: AirSpan::new("t.rs", 5, 5),
    })
}

fn cx008_section(max: u32, orchestration: Vec<&str>) -> CxSection {
    CxSection {
        max_fan_out: max,
        orchestration_paths: orchestration.into_iter().map(str::to_string).collect(),
        ..CxSection::default()
    }
}

#[test]
fn cx008_silent_when_orchestration_paths_empty() {
    // Even with rampant fan-out, no orchestration declaration means silent.
    // Mirrors DG/MO lockfile-driven convention.
    let mut items = vec![func("dispatch", 5)];
    for i in 0..50 {
        items.push(callsite(&format!("callee{i}"), "x::dispatch"));
    }
    let air = air_with(Some("x::core"), items);
    let section = CxSection::default(); // empty orchestration_paths
    assert!(cx008(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn cx008_fires_when_count_exceeds_budget_outside_orchestration() {
    // 6 call sites, budget 5, in `x::core` (not under orchestration) → fires.
    let mut items = vec![func("dispatch", 5)];
    for i in 0..6 {
        items.push(callsite(&format!("callee{i}"), "x::dispatch"));
    }
    let air = air_with(Some("x::core"), items);
    let section = cx008_section(5, vec!["x::cli::*"]);
    let diags = cx008(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1, "got {diags:?}");
    assert_eq!(diags[0].rule_id, "CX008");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(diags[0].message.contains("x::dispatch"));
    assert!(diags[0].message.contains("6"));
    assert!(diags[0].message.contains("budget 5"));
}

#[test]
fn cx008_quiet_when_count_at_or_below_budget() {
    let mut items = vec![func("dispatch", 5)];
    for i in 0..5 {
        // exactly at budget
        items.push(callsite(&format!("c{i}"), "x::dispatch"));
    }
    let air = air_with(Some("x::core"), items);
    let section = cx008_section(5, vec!["x::cli::*"]);
    assert!(cx008(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn cx008_orchestration_path_silences_diagnostic() {
    // 10 call sites, budget 3, but module matches orchestration → silent.
    let mut items = vec![func("dispatch", 5)];
    for i in 0..10 {
        items.push(callsite(&format!("c{i}"), "x::dispatch"));
    }
    let air = air_with(Some("x::cli::dispatch"), items);
    let section = cx008_section(3, vec!["x::cli::*"]);
    assert!(cx008(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn cx008_agent_strict_elevates_to_fatal() {
    let mut items = vec![func("dispatch", 5)];
    for i in 0..6 {
        items.push(callsite(&format!("c{i}"), "x::dispatch"));
    }
    let air = air_with(Some("x::core"), items);
    let section = cx008_section(5, vec!["x::cli::*"]);
    let diags = cx008(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

#[test]
fn cx008_only_counts_call_sites_in_owning_function() {
    // Two functions; only one issues lots of call sites.
    let mut items = vec![func("dispatch", 5), func("tiny", 5)];
    for i in 0..6 {
        items.push(callsite(&format!("c{i}"), "x::dispatch"));
    }
    items.push(callsite("only", "x::tiny"));
    let air = air_with(Some("x::core"), items);
    let section = cx008_section(5, vec!["x::cli::*"]);
    let diags = cx008(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1, "got {diags:?}");
    assert!(diags[0].message.contains("x::dispatch"));
    assert!(!diags[0].message.contains("x::tiny"));
}
