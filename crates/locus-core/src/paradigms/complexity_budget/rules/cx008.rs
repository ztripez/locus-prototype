//! CX008 — high fan-out outside orchestration owners.
//!
//! Migrated to `RuleDefinition` in P4 (epic #71). Replaces the legacy
//! `super::cx008()` function. Builds a call-site count index, then walks
//! `AirItem::Function` items, skipping those whose module matches an
//! `orchestration_paths` pattern, emitting a `RuleFinding` for each
//! function whose call-site count exceeds `max_fan_out`.

use std::collections::HashMap;

use locus_air::AirItem;

use crate::diagnostics::Severity;
use crate::governance::finding::{FindingSource, RuleFinding};
use crate::governance::ids::{ParadigmId, RuleId};
use crate::governance::rule::{RuleContext, RuleDefinition};

pub struct Cx008Rule;

pub static CX008_RULE: Cx008Rule = Cx008Rule;

const CX008_ID: RuleId = RuleId::new("CX008");
const CX_PARADIGM: ParadigmId = ParadigmId::new("CX");

impl RuleDefinition for Cx008Rule {
    fn id(&self) -> RuleId {
        CX008_ID
    }
    fn paradigm(&self) -> ParadigmId {
        CX_PARADIGM
    }
    fn title(&self) -> &'static str {
        "high fan-out outside orchestration owners"
    }
    fn default_severity(&self) -> Severity {
        Severity::Warning
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        use super::super::lockfile_schema::{CxSection, matches_pattern};
        let section: CxSection = ctx.lockfile.paradigm_section("CX").unwrap_or_default();
        if section.orchestration_paths.is_empty() {
            return Vec::new();
        }
        let fan_out = build_fan_out_index(ctx.air);
        let mut out = Vec::new();
        for pkg in &ctx.air.packages {
            for file in &pkg.files {
                let module_path = file.module_path.as_deref();
                let is_orchestrator = module_path
                    .map(|mp| {
                        section
                            .orchestration_paths
                            .iter()
                            .any(|pat| matches_pattern(pat, mp))
                    })
                    .unwrap_or(false);
                if is_orchestrator {
                    continue;
                }
                for item in &file.items {
                    let AirItem::Function(func) = item else {
                        continue;
                    };
                    let Some(&count) = fan_out.get(func.symbol.as_str()) else {
                        continue;
                    };
                    if count <= section.max_fan_out {
                        continue;
                    }
                    out.push(RuleFinding {
                        id: ctx.finding_ids.next(),
                        source: FindingSource::RegisteredRule(CX008_ID),
                        rule_id: Some(CX008_ID),
                        paradigm_id: Some(CX_PARADIGM),
                        default_severity: ctx.mode.elevate(Severity::Warning),
                        span: Some(func.span.clone()),
                        concept: None,
                        message: format!(
                            "function `{}` issues {count} call sites, budget {} \
                             — high fan-out outside an accepted orchestration module",
                            func.symbol, section.max_fan_out
                        ),
                        evidence: vec![],
                        why: vec![
                            format!("function symbol `{}`", func.symbol),
                            match module_path {
                                Some(mp) => format!("module path `{mp}`"),
                                None => "module path unknown".to_string(),
                            },
                            format!(
                                "call-site count {count} > max_fan_out {}",
                                section.max_fan_out
                            ),
                        ],
                        suggested_fix: Some(
                            "extract sub-steps into helper functions, or — if this \
                             function is a legitimate orchestrator — add its module \
                             path to `paradigms.CX.orchestration_paths` in \
                             `locus.lock`"
                                .into(),
                        ),
                        diagnostic_code: None,
                    });
                }
            }
        }
        out
    }
}

fn build_fan_out_index(air: &locus_air::AirWorkspace) -> HashMap<String, u32> {
    let mut fan_out: HashMap<String, u32> = HashMap::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                if let AirItem::CallSite(cs) = item
                    && let Some(sym) = cs.function.as_deref()
                {
                    *fan_out.entry(sym.to_string()).or_insert(0) += 1;
                }
            }
        }
    }
    fan_out
}
