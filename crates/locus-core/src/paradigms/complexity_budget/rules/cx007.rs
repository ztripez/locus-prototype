//! CX007 — excessive public surface.
//!
//! Migrated to `RuleDefinition` in P4 (epic #71). Replaces the legacy
//! `super::cx007()` function. Walks `AirFile` entries, counts public
//! `AirItem::Type` and `AirItem::Function` items, and emits a `RuleFinding`
//! for each file that exceeds `max_public_items` and is not exempt.

use locus_air::{AirItem, Visibility};

use crate::diagnostics::Severity;
use crate::governance::finding::{FindingSource, RuleFinding};
use crate::governance::ids::{ParadigmId, RuleId};
use crate::governance::rule::{RuleContext, RuleDefinition};

pub struct Cx007Rule;

pub static CX007_RULE: Cx007Rule = Cx007Rule;

const CX007_ID: RuleId = RuleId::new("CX007");
const CX_PARADIGM: ParadigmId = ParadigmId::new("CX");

impl RuleDefinition for Cx007Rule {
    fn id(&self) -> RuleId {
        CX007_ID
    }
    fn paradigm(&self) -> ParadigmId {
        CX_PARADIGM
    }
    fn title(&self) -> &'static str {
        "excessive public surface"
    }
    fn default_severity(&self) -> Severity {
        Severity::Warning
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        use super::super::lockfile_schema::{CxSection, matches_pattern};
        let section: CxSection = ctx.lockfile.paradigm_section("CX").unwrap_or_default();
        let mut out = Vec::new();
        for pkg in &ctx.air.packages {
            for file in &pkg.files {
                let Some(module_path) = file.module_path.as_deref() else {
                    continue;
                };
                if section
                    .exempt_paths
                    .iter()
                    .any(|pat| matches_pattern(pat.pattern(), module_path))
                {
                    continue;
                }
                let public_count = count_public_items(file);
                if public_count <= section.max_public_items {
                    continue;
                }
                let span = anchor_span(file);
                out.push(RuleFinding {
                    id: ctx.finding_ids.next(),
                    source: FindingSource::RegisteredRule(CX007_ID),
                    rule_id: Some(CX007_ID),
                    paradigm_id: Some(CX_PARADIGM),
                    default_severity: ctx.mode.elevate(Severity::Warning),
                    span: Some(span),
                    concept: None,
                    message: format!(
                        "module `{module_path}` exposes {public_count} public items, budget {} \
                         — likely a kitchen-sink facade",
                        section.max_public_items
                    ),
                    evidence: vec![],
                    why: vec![
                        format!("file `{}`", file.path),
                        format!("module path `{module_path}`"),
                        format!(
                            "public item count {public_count} > max_public_items {}",
                            section.max_public_items
                        ),
                    ],
                    suggested_fix: Some(
                        "split the module into smaller, more focused units; or — if this \
                         facade is intentional (e.g. a public prelude) — exempt the \
                         module by adding its path pattern to `paradigms.CX.exempt_paths` \
                         in `locus.lock`, or raise `paradigms.CX.max_public_items`"
                            .into(),
                    ),
                    diagnostic_code: None,
                });
            }
        }
        out
    }
}

fn count_public_items(file: &locus_air::AirFile) -> u32 {
    file.items
        .iter()
        .filter(|it| match it {
            AirItem::Type(t) => t.visibility == Visibility::Public,
            AirItem::Function(f) => f.visibility == Visibility::Public,
            _ => false,
        })
        .count() as u32
}

fn anchor_span(file: &locus_air::AirFile) -> locus_air::AirSpan {
    file.items
        .iter()
        .find_map(|it| match it {
            AirItem::Type(t) => Some(t.span.clone()),
            AirItem::Function(f) => Some(f.span.clone()),
            _ => None,
        })
        .unwrap_or_else(|| locus_air::AirSpan::new(file.path.clone(), 1, 1))
}
