//! CX001 — function exceeds its line budget.
//!
//! Migrated to `RuleDefinition` in P2 (epic #71). Replaces the legacy
//! `super::cx001()` function. Walks `AirItem::Function` items, compares
//! each function's `line_count` against the effective budget (override or
//! workspace default or built-in fallback), and emits a `RuleFinding`
//! with `Evidence::ComplexityBudget` for each function that overshoots.

// locus: ot canonical

use crate::diagnostics::Severity;
use crate::governance::evidence::Evidence;
use crate::governance::finding::{FindingSource, RuleFinding};
use crate::governance::ids::{ParadigmId, RuleId};
use crate::governance::rule::{RuleContext, RuleDefinition};

pub struct Cx001Rule;

pub static CX001_RULE: Cx001Rule = Cx001Rule;

const CX001_ID: RuleId = RuleId::new("CX001");
const CX_PARADIGM: ParadigmId = ParadigmId::new("CX");

impl RuleDefinition for Cx001Rule {
    fn id(&self) -> RuleId {
        CX001_ID
    }
    fn paradigm(&self) -> ParadigmId {
        CX_PARADIGM
    }
    fn title(&self) -> &'static str {
        "function exceeds its line budget"
    }
    fn default_severity(&self) -> Severity {
        Severity::Warning
    }
    fn observe(&self, _ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        // Implemented in Task 4.
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::CheckMode;
    use crate::governance::ids::FindingIdMinter;
    use crate::governance::registry::{ParadigmRegistry, RuleRegistry};
    use crate::lockfile::Lockfile;
    use locus_air::{
        AIR_SCHEMA_VERSION, AirFile, AirFunction, AirItem, AirPackage, AirSpan, AirWorkspace,
        Visibility,
    };

    /// Build a workspace with one function whose line_count overshoots the
    /// 50-line built-in fallback budget. The migrated rule should emit
    /// exactly one finding.
    #[test]
    fn fires_on_function_over_built_in_fallback_budget() {
        let air = workspace_with_function("crate_a::module_b::overlong_fn", 73);
        let lf = Lockfile::default();
        let rules = RuleRegistry::standard();
        let paradigms = ParadigmRegistry::empty();
        let minter = FindingIdMinter::new();
        let ctx = RuleContext {
            air: &air,
            lockfile: &lf,
            mode: CheckMode::Human,
            rule_registry: &rules,
            paradigm_registry: &paradigms,
            finding_ids: &minter,
        };

        let findings = Cx001Rule.observe(&ctx);
        assert_eq!(findings.len(), 1, "expected one finding, got {findings:?}");
        let f = &findings[0];
        assert_eq!(f.source, FindingSource::RegisteredRule(RuleId::new("CX001")));
        assert_eq!(f.rule_id, Some(RuleId::new("CX001")));
        assert_eq!(f.paradigm_id, Some(ParadigmId::new("CX")));
        assert_eq!(f.default_severity, Severity::Warning);
        assert!(f.message.contains("overlong_fn"));
        assert!(f.message.contains("73 lines"));
        assert!(f.message.contains("budget 50"));

        // Evidence is typed.
        assert_eq!(f.evidence.len(), 1);
        match &f.evidence[0] {
            Evidence::ComplexityBudget {
                lines,
                budget,
                override_match,
            } => {
                assert_eq!(*lines, 73);
                assert_eq!(*budget, 50);
                assert_eq!(*override_match, None);
            }
            other => panic!("expected ComplexityBudget evidence, got {other:?}"),
        }
    }

    /// Test helper: build a one-file workspace containing a single function
    /// with the given symbol and line_count.
    fn workspace_with_function(symbol: &str, line_count: u32) -> AirWorkspace {
        AirWorkspace {
            schema_version: AIR_SCHEMA_VERSION,
            packages: vec![AirPackage {
                name: "crate_a".into(),
                version: "0".into(),
                root_dir: "/".into(),
                files: vec![AirFile {
                    path: "src/module_b.rs".into(),
                    module_path: Some("crate_a::module_b".into()),
                    items: vec![AirItem::Function(AirFunction {
                        name: symbol.rsplit("::").next().unwrap_or(symbol).to_string(),
                        symbol: symbol.to_string(),
                        symbol_segments: Vec::new(),
                        visibility: Visibility::Public,
                        params: Vec::new(),
                        return_type: None,
                        span: AirSpan::new("src/module_b.rs", 1, line_count),
                        line_count,
                        doc: None,
                        decorators: Vec::new(),
                    })],
                    hints: Vec::new(),
                    parse_error: None,
                    line_count: line_count + 5,
                }],
            }],
            facts: Vec::new(),
        }
    }
}
