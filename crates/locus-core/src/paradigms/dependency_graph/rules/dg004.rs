//! DG004 — shared module reaching feature-specific code.

use crate::diagnostics::Severity;
use crate::governance::finding::{FindingSource, RuleFinding};
use crate::governance::ids::{ParadigmId, RuleId};
use crate::governance::rule::{RuleContext, RuleDefinition};
use locus_air::AirItem;

pub struct Dg004Rule;
pub static DG004_RULE: Dg004Rule = Dg004Rule;

const DG004_ID: RuleId = RuleId::new("DG004");
const DG_PARADIGM: ParadigmId = ParadigmId::new("DG");

impl RuleDefinition for Dg004Rule {
    fn id(&self) -> RuleId {
        DG004_ID
    }
    fn paradigm(&self) -> ParadigmId {
        DG_PARADIGM
    }
    fn title(&self) -> &'static str {
        "shared module reaching feature-specific code"
    }
    fn default_severity(&self) -> Severity {
        Severity::Fatal
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        use super::super::lockfile_schema::{DgSection, matches_pattern};
        let section: DgSection = ctx.lockfile.paradigm_section("DG").unwrap_or_default();
        if section.shared_paths.is_empty() || section.features.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for pkg in &ctx.air.packages {
            for file in &pkg.files {
                let Some(module_path) = file.module_path.as_deref() else {
                    continue;
                };
                let Some(shared_pattern) = section
                    .shared_paths
                    .iter()
                    .find(|pat| matches_pattern(pat, module_path))
                else {
                    continue;
                };
                for item in &file.items {
                    let AirItem::Import(imp) = item else {
                        continue;
                    };
                    let Some(target_feature) =
                        super::helpers::owning_feature(&section.features, &imp.path)
                    else {
                        continue;
                    };
                    out.push(RuleFinding {
                        id: ctx.finding_ids.next(),
                        source: FindingSource::RegisteredRule(DG004_ID),
                        rule_id: Some(DG004_ID),
                        paradigm_id: Some(DG_PARADIGM),
                        default_severity: ctx.mode.elevate(Severity::Fatal),
                        span: Some(imp.span.clone()),
                        concept: None,
                        message: format!(
                            "shared module `{module_path}` imports feature `{}` via `{}`",
                            target_feature.name, imp.path
                        ),
                        evidence: vec![],
                        why: vec![
                            format!(
                                "`{module_path}` matches shared_paths pattern `{shared_pattern}`"
                            ),
                            format!(
                                "`{}` belongs to feature `{}` (pattern `{}`)",
                                imp.path, target_feature.name, target_feature.module
                            ),
                            "shared infrastructure must not depend on any feature".into(),
                        ],
                        suggested_fix: Some(
                            "invert the dependency: the feature should depend on the shared module \
                             (move the call into the feature, or extract the shared module's \
                             responsibility into a port the feature provides)"
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
