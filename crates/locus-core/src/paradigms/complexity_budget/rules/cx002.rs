//! CX002 — module line budget.
//!
//! Migrated to `RuleDefinition` in P4 (epic #71). Replaces the legacy
//! `super::cx002()` function. Walks `AirFile` entries, compares each
//! file's `line_count` against the effective module budget (override or
//! workspace default or built-in fallback), and emits a `RuleFinding`
//! for each file that overshoots.

use crate::diagnostics::Severity;
use crate::governance::finding::{FindingSource, RuleFinding};
use crate::governance::ids::{ParadigmId, RuleId};
use crate::governance::rule::{RuleContext, RuleDefinition};

pub struct Cx002Rule;

pub static CX002_RULE: Cx002Rule = Cx002Rule;

const CX002_ID: RuleId = RuleId::new("CX002");
const CX_PARADIGM: ParadigmId = ParadigmId::new("CX");

impl RuleDefinition for Cx002Rule {
    fn id(&self) -> RuleId {
        CX002_ID
    }
    fn paradigm(&self) -> ParadigmId {
        CX_PARADIGM
    }
    fn title(&self) -> &'static str {
        "module line budget"
    }
    fn default_severity(&self) -> Severity {
        Severity::Warning
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        use super::super::lockfile_schema::CxSection;
        let section: CxSection = ctx.lockfile.paradigm_section("CX").unwrap_or_default();
        let default_budget = section.effective_default_module();
        let mut out = Vec::new();
        for pkg in &ctx.air.packages {
            for file in &pkg.files {
                let Some(module_path) = file.module_path.as_deref() else {
                    continue;
                };
                if let Some(f) = check_file(file, module_path, &section, default_budget, ctx) {
                    out.push(f);
                }
            }
        }
        out
    }
}

// locus: allow OT009 — `check_file` here is CX002's per-file walker, not a validator for the AirFile canonical (false positive: name matches the `check_` heuristic but the function is a rule internal)
fn check_file(
    file: &locus_air::AirFile,
    module_path: &str,
    section: &super::super::lockfile_schema::CxSection,
    default_budget: u32,
    ctx: &RuleContext<'_>,
) -> Option<RuleFinding> {
    let matched_override = section.matching_module_override(module_path);
    let budget = matched_override
        .map(|o| o.max_module_lines)
        .unwrap_or(default_budget);
    if file.line_count <= budget {
        return None;
    }
    let narrowed = matched_override.is_some() || section.default_max_module_lines.is_some();
    let why = cx002_why(
        &file.path,
        file.line_count,
        budget,
        default_budget,
        matched_override,
        section,
    );
    let source_label = match matched_override {
        Some(o) => format!("override `{}`", o.module),
        None => "workspace default".to_string(),
    };
    Some(RuleFinding {
        id: ctx.finding_ids.next(),
        source: FindingSource::RegisteredRule(CX002_ID),
        rule_id: Some(CX002_ID),
        paradigm_id: Some(CX_PARADIGM),
        default_severity: ctx
            .mode
            .elevate_when_actionable(Severity::Warning, narrowed),
        span: Some(locus_air::AirSpan::new(file.path.clone(), 1, 1)),
        concept: None,
        message: format!(
            "module `{module_path}` is {} lines, budget {} ({source_label})",
            file.line_count, budget
        ),
        evidence: vec![],
        why,
        suggested_fix: Some(
            "split the module into smaller, more focused files each owning one \
             responsibility, or — if this density is intended (e.g. a rule table, \
             a lockfile schema, a state machine) — raise the budget by adding an \
             override to `paradigms.CX.module_overrides` in `locus.lock`"
                .into(),
        ),
        diagnostic_code: None,
    })
}

fn cx002_why(
    file_path: &str,
    file_line_count: u32,
    budget: u32,
    default_budget: u32,
    matched_override: Option<&super::super::lockfile_schema::CxModuleOverride>,
    section: &super::super::lockfile_schema::CxSection,
) -> Vec<String> {
    let mut why = vec![
        format!("file `{file_path}` spans {file_line_count} line(s)"),
        if let Some(o) = matched_override {
            format!("budget {budget} from override `module = {}`", o.module)
        } else {
            format!("budget {budget} (workspace default)")
        },
    ];
    if matched_override.is_none() && section.default_max_module_lines.is_none() {
        why.push(format!(
            "no `default_max_module_lines` configured; using built-in fallback {}",
            default_budget
        ));
    }
    why
}
