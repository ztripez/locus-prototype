//! DG001 — forbidden import.
//!
//! Migrated to `RuleDefinition` in P2 (epic #71). Replaces the legacy
//! `super::dg001()` function. Walks `AirImport` items in every file,
//! compares each against `section.forbidden_edges`, and emits a
//! `RuleFinding` with `Evidence::Structured(json)` for each match.
//! Always Fatal: a forbidden edge is, by the user's own declaration, a
//! directional violation.

// locus: ot canonical

use super::super::lockfile_schema::{DgSection, ForbiddenEdge, matches_pattern};
use crate::diagnostics::Severity;
use crate::governance::evidence::Evidence;
use crate::governance::finding::{FindingSource, RuleFinding};
use crate::governance::ids::{ParadigmId, RuleId};
use crate::governance::rule::{RuleContext, RuleDefinition};

pub struct Dg001Rule;

pub static DG001_RULE: Dg001Rule = Dg001Rule;

const DG001_ID: RuleId = RuleId::new("DG001");
const DG_PARADIGM: ParadigmId = ParadigmId::new("DG");

impl RuleDefinition for Dg001Rule {
    fn id(&self) -> RuleId {
        DG001_ID
    }
    fn paradigm(&self) -> ParadigmId {
        DG_PARADIGM
    }
    fn title(&self) -> &'static str {
        "forbidden import"
    }
    fn default_severity(&self) -> Severity {
        Severity::Fatal
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        let section: DgSection = ctx.lockfile.paradigm_section("DG").unwrap_or_default();
        if section.forbidden_edges.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for pkg in &ctx.air.packages {
            for file in &pkg.files {
                let Some(module_path) = file.module_path.as_deref() else {
                    continue;
                };
                check_file(file, module_path, &section, ctx, &mut out);
            }
        }
        out
    }
}

// locus: allow OT009 — helper scoped to DG001 rule; not a canonical-type owner
fn check_file(
    file: &locus_air::AirFile,
    module_path: &str,
    section: &DgSection,
    ctx: &RuleContext<'_>,
    out: &mut Vec<RuleFinding>,
) {
    use locus_air::AirItem;
    for item in &file.items {
        let AirItem::Import(imp) = item else {
            continue;
        };
        for edge in &section.forbidden_edges {
            if !matches_pattern(&edge.from, module_path) {
                continue;
            }
            if !matches_pattern(&edge.to, &imp.path) {
                continue;
            }
            out.push(make_finding(module_path, imp, edge, ctx));
            break; // one diagnostic per (file, import) — match legacy semantics
        }
    }
}

fn make_finding(
    module_path: &str,
    imp: &locus_air::AirImport,
    edge: &ForbiddenEdge,
    ctx: &RuleContext<'_>,
) -> RuleFinding {
    let mut why = vec![
        format!("importer `{module_path}` matches `from = {}`", edge.from),
        format!("import `{}` matches `to = {}`", imp.path, edge.to),
    ];
    if let Some(reason) = &edge.reason {
        why.push(format!("reason: {reason}"));
    }
    let severity = ctx.mode.elevate(Severity::Fatal);
    let mut json = serde_json::json!({
        "from_pattern": &edge.from,
        "to_pattern": &edge.to,
        "importer_module": module_path,
        "import_path": &imp.path,
    });
    if let Some(reason) = &edge.reason {
        json["reason"] = serde_json::Value::String(reason.clone());
    }
    RuleFinding {
        id: ctx.finding_ids.next(),
        source: FindingSource::RegisteredRule(DG001_ID),
        rule_id: Some(DG001_ID),
        paradigm_id: Some(DG_PARADIGM),
        default_severity: severity,
        span: Some(imp.span.clone()),
        concept: None,
        message: format!(
            "forbidden import: `{module_path}` must not reach `{}`",
            imp.path
        ),
        evidence: vec![Evidence::Structured(json)],
        why,
        suggested_fix: Some(
            "remove the import, or route the call through an accepted \
             intermediary (port, application service, or shared crate); \
             if the edge is wrong, edit `paradigms.DG.forbidden_edges` in \
             `locus.lock`"
                .into(),
        ),
        diagnostic_code: None,
    }
}

#[cfg(test)]
mod dg001_rule_tests {
    use super::*;
    use crate::diagnostics::CheckMode;
    use crate::governance::ids::FindingIdMinter;
    use crate::governance::registry::{ParadigmRegistry, RuleRegistry};
    use crate::lockfile::Lockfile;
    use locus_air::{AirFile, AirImport, AirItem, AirPackage, AirSpan, AirWorkspace, Visibility};

    #[test]
    fn fires_on_import_matching_forbidden_edge() {
        let air = workspace_with_forbidden_import();
        let lf = lockfile_with_forbidden_edge();
        let findings = run_observe(&air, &lf, CheckMode::Human);

        assert_eq!(
            findings.len(),
            1,
            "expected one DG001 finding, got {findings:?}"
        );
        assert_finding_shape(&findings[0]);
        assert_structured_evidence(&findings[0]);
    }

    fn run_observe(air: &AirWorkspace, lf: &Lockfile, mode: CheckMode) -> Vec<RuleFinding> {
        let rules = RuleRegistry::standard();
        let paradigms = ParadigmRegistry::empty();
        let minter = FindingIdMinter::new();
        let ctx = RuleContext {
            air,
            lockfile: lf,
            mode,
            rule_registry: &rules,
            paradigm_registry: &paradigms,
            finding_ids: &minter,
        };
        Dg001Rule.observe(&ctx)
    }

    fn assert_finding_shape(f: &RuleFinding) {
        assert_eq!(f.source, FindingSource::RegisteredRule(DG001_ID));
        assert_eq!(f.rule_id, Some(DG001_ID));
        assert_eq!(f.paradigm_id, Some(DG_PARADIGM));
        assert_eq!(f.default_severity, Severity::Fatal);
        assert!(
            f.message.contains("forbidden import"),
            "expected legacy-compatible message, got `{}`",
            f.message
        );
    }

    fn assert_structured_evidence(f: &RuleFinding) {
        assert_eq!(f.evidence.len(), 1);
        match &f.evidence[0] {
            Evidence::Structured(json) => {
                assert_eq!(json["from_pattern"], "pkg::feature_a::*");
                assert_eq!(json["to_pattern"], "pkg::feature_b::*");
                assert_eq!(json["importer_module"], "pkg::feature_a::handler");
                assert_eq!(json["import_path"], "pkg::feature_b::internal");
            }
            other => panic!("expected Structured evidence, got {other:?}"),
        }
    }

    fn workspace_with_forbidden_import() -> AirWorkspace {
        AirWorkspace::new(vec![AirPackage {
            name: "pkg".into(),
            version: "0.0.1".into(),
            root_dir: "/tmp/pkg".into(),
            files: vec![AirFile {
                path: "src/feature_a/handler.rs".into(),
                module_path: Some("pkg::feature_a::handler".into()),
                items: vec![AirItem::Import(AirImport {
                    path: "pkg::feature_b::internal".into(),
                    path_segments: Vec::new(),
                    visibility: Visibility::Module,
                    span: AirSpan::new("src/feature_a/handler.rs", 1, 1),
                })],
                hints: Vec::new(),
                parse_error: None,
                line_count: 5,
            }],
        }])
    }

    fn lockfile_with_forbidden_edge() -> Lockfile {
        let mut lf = Lockfile::default();
        let section = serde_json::json!({
            "forbidden_edges": [
                {
                    "from": "pkg::feature_a::*",
                    "to": "pkg::feature_b::*",
                    "reason": "feature isolation: A and B don't talk directly"
                }
            ],
            "features": [],
            "shared_paths": []
        });
        lf.paradigms.insert("DG".to_string(), section);
        lf
    }
}
