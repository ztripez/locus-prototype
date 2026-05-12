//! DG003 — cross-feature internals reach.

use crate::diagnostics::Severity;
use crate::governance::finding::{FindingSource, RuleFinding};
use crate::governance::ids::{ParadigmId, RuleId};
use crate::governance::rule::{RuleContext, RuleDefinition};
use locus_air::AirItem;

pub struct Dg003Rule;
pub static DG003_RULE: Dg003Rule = Dg003Rule;

const DG003_ID: RuleId = RuleId::new("DG003");
const DG_PARADIGM: ParadigmId = ParadigmId::new("DG");

impl RuleDefinition for Dg003Rule {
    fn id(&self) -> RuleId {
        DG003_ID
    }
    fn paradigm(&self) -> ParadigmId {
        DG_PARADIGM
    }
    fn title(&self) -> &'static str {
        "cross-feature internals reach"
    }
    fn default_severity(&self) -> Severity {
        Severity::Fatal
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        use super::super::lockfile_schema::DgSection;
        let section: DgSection = ctx.lockfile.paradigm_section("DG").unwrap_or_default();
        if section.features.len() < 2 {
            return Vec::new();
        }
        let mut out = Vec::new();
        for pkg in &ctx.air.packages {
            for file in &pkg.files {
                let Some(module_path) = file.module_path.as_deref() else {
                    continue;
                };
                let Some(importer_feature) =
                    super::helpers::owning_feature(&section.features, module_path)
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
                    if std::ptr::eq(importer_feature, target_feature) {
                        continue;
                    }
                    if super::helpers::path_in_public_api(target_feature, &imp.path) {
                        continue;
                    }
                    out.push(RuleFinding {
                        id: ctx.finding_ids.next(),
                        source: FindingSource::RegisteredRule(DG003_ID),
                        rule_id: Some(DG003_ID),
                        paradigm_id: Some(DG_PARADIGM),
                        default_severity: ctx.mode.elevate(Severity::Fatal),
                        span: Some(imp.span.clone()),
                        concept: None,
                        message: format!(
                            "feature `{importer}` reaches into `{target}` internals via `{}`",
                            imp.path,
                            importer = importer_feature.name,
                            target = target_feature.name,
                        ),
                        evidence: vec![],
                        why: vec![
                            format!(
                                "importer `{module_path}` belongs to feature `{}`",
                                importer_feature.name
                            ),
                            format!(
                                "import `{}` belongs to feature `{}` but is not in its public API",
                                imp.path, target_feature.name
                            ),
                            if target_feature.public_api.is_empty() {
                                format!(
                                    "feature `{}` has no public_api defined",
                                    target_feature.name
                                )
                            } else {
                                format!(
                                    "public_api patterns: {}",
                                    target_feature
                                        .public_api
                                        .iter()
                                        .map(|p| format!("`{p}`"))
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                )
                            },
                        ],
                        suggested_fix: Some(format!(
                            "import through `{}`'s public API, or expand its public_api list \
                             to include `{}` if this access is intentional",
                            target_feature.name, imp.path
                        )),
                        diagnostic_code: None,
                    });
                }
            }
        }
        out
    }
}
