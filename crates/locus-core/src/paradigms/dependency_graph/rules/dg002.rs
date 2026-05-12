//! DG002 — dependency cycle across crates.

use std::collections::{BTreeMap, BTreeSet};

use crate::diagnostics::Severity;
use crate::governance::finding::{FindingSource, RuleFinding};
use crate::governance::ids::{ParadigmId, RuleId};
use crate::governance::rule::{RuleContext, RuleDefinition};

pub struct Dg002Rule;
pub static DG002_RULE: Dg002Rule = Dg002Rule;

const DG002_ID: RuleId = RuleId::new("DG002");
const DG_PARADIGM: ParadigmId = ParadigmId::new("DG");

impl RuleDefinition for Dg002Rule {
    fn id(&self) -> RuleId {
        DG002_ID
    }
    fn paradigm(&self) -> ParadigmId {
        DG_PARADIGM
    }
    fn title(&self) -> &'static str {
        "dependency cycle"
    }
    fn default_severity(&self) -> Severity {
        Severity::Fatal
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        let edges = super::collect_crate_edges(ctx.air);
        if edges.is_empty() {
            return Vec::new();
        }

        let mut nodes: Vec<String> = edges
            .keys()
            .flat_map(|(a, b)| [a.clone(), b.clone()])
            .collect();
        nodes.sort();
        nodes.dedup();
        let node_idx: BTreeMap<&str, usize> = nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.as_str(), i))
            .collect();

        let mut adj: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
        for (a, b) in edges.keys() {
            adj[node_idx[a.as_str()]].push(node_idx[b.as_str()]);
        }

        let sccs = super::tarjan_sccs(&adj);

        let mut out = Vec::new();
        for scc in sccs {
            if scc.len() < 2 {
                continue;
            }
            let scc_set: BTreeSet<usize> = scc.iter().copied().collect();
            let mut members: Vec<&str> = scc.iter().map(|&i| nodes[i].as_str()).collect();
            members.sort();

            for ((a, b), evidence) in &edges {
                let ai = node_idx[a.as_str()];
                let bi = node_idx[b.as_str()];
                if !scc_set.contains(&ai) || !scc_set.contains(&bi) {
                    continue;
                }
                let members_label = if members.len() == 2 {
                    format!("`{}` ↔ `{}`", members[0], members[1])
                } else {
                    let joined = members
                        .iter()
                        .map(|m| format!("`{m}`"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("[{joined}]")
                };
                out.push(RuleFinding {
                    id: ctx.finding_ids.next(),
                    source: FindingSource::RegisteredRule(DG002_ID),
                    rule_id: Some(DG002_ID),
                    paradigm_id: Some(DG_PARADIGM),
                    default_severity: ctx.mode.elevate(Severity::Fatal),
                    span: Some(evidence.span.clone()),
                    concept: None,
                    message: format!(
                        "dependency cycle: `{a}` -> `{}` participates in cycle {members_label}",
                        evidence.import_path
                    ),
                    evidence: vec![],
                    why: vec![
                        format!("`{a}` -> `{b}` (via `{}`)", evidence.import_path),
                        format!("cycle participants: {members_label}"),
                        format!("evidence import in `{}`", evidence.file_path),
                    ],
                    suggested_fix: Some(
                        "break the cycle by extracting a shared trait/port crate, or restructure \
                         ownership so one direction is implementation-side only and goes through a port"
                            .into(),
                    ),
                    diagnostic_code: None,
                });
            }
        }
        out
    }
}
