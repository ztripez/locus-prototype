//! User-marker loader. Translates `// locus: fact <fact_kind>` source
//! annotations into `AirFact` entries the consuming paradigms read the same
//! way they read std-rt's facts.
//!
//! Why this loader exists: the spec's normalised fact vocabulary
//! (`docs/PARADIGMS.md` §"Framework Knowledge and Sub-Paradigm
//! Loaders") includes architectural concepts like `hot_path`,
//! `request_context`, `boundary_entry`, `runtime_state_owner`,
//! `background_worker` that the loader tier can't auto-recognise
//! without framework knowledge. Until per-framework adapters land,
//! users mark functions explicitly via `// locus: fact <fact_kind>` and
//! this loader promotes the marker to an `AirFact`. The same
//! mechanism lets users annotate their own helpers as carrying
//! kinds std-rt only recognises in stdlib (`external_io`,
//! `persistence_write`, `blocking_call`).
//!
//! The loader binds each `AirHint::MarksFact` to the function whose
//! span overlaps the hint's `target_span`. Unknown `fact_kind`
//! strings (typos, future spec additions) are silently skipped — the
//! hint scanner already canonicalises casing/punctuation, so anything
//! that doesn't map to a known [`FactKind`] is genuinely unknown.

use locus_air::{
    AirFact, AirFunction, AirHint, AirItem, AirWorkspace, FactKind, FactTarget, HintKind,
};
use locus_core::Loader;

// locus: ot canonical
pub struct MarkersLoader;

impl Loader for MarkersLoader {
    fn name(&self) -> &'static str {
        "markers"
    }

    fn enrich(&self, air: &AirWorkspace) -> Vec<AirFact> {
        let mut out = Vec::new();
        for pkg in &air.packages {
            for file in &pkg.files {
                for hint in &file.hints {
                    let HintKind::MarksFact { fact_kind } = &hint.kind else {
                        continue;
                    };
                    let Some(kind) = parse_fact_kind(fact_kind) else {
                        continue;
                    };
                    let Some(func) = function_for_hint(&file.items, hint) else {
                        continue;
                    };
                    out.push(AirFact {
                        kind,
                        target: FactTarget::Function {
                            symbol: func.symbol.clone(),
                        },
                        source: "markers".to_string(),
                        confidence: 1.0,
                        reasons: vec![format!(
                            "user marker: `// locus: fact {fact_kind}` above `{}`",
                            func.name
                        )],
                        evidence: Some(format!("// locus: fact {fact_kind}")),
                    });
                }
            }
        }
        out
    }
}

/// Map the canonicalised snake_case spec name to a [`FactKind`].
/// Returns `None` for unknown markers — the hint scanner already
/// normalises casing, so an unknown value here is a genuine
/// "this kind isn't in `FactKind`" rather than a typo.
fn parse_fact_kind(s: &str) -> Option<FactKind> {
    Some(match s {
        "spawned_work" => FactKind::SpawnedWork,
        "config_read" => FactKind::ConfigRead,
        "logging" => FactKind::Logging,
        "external_io" => FactKind::ExternalIo,
        "persistence_write" => FactKind::PersistenceWrite,
        "blocking_call" => FactKind::BlockingCall,
        "hot_path" => FactKind::HotPath,
        "request_context" => FactKind::RequestContext,
        "boundary_entry" => FactKind::BoundaryEntry,
        "runtime_state_owner" => FactKind::RuntimeStateOwner,
        "background_worker" => FactKind::BackgroundWorker,
        _ => return None,
    })
}

/// Find the `AirFunction` the hint binds to. The hint scanner already
/// resolved the hint's `target_span` to the next non-comment-non-attr
/// line; we look for the function whose span starts on that line (or
/// contains it — syn's function span includes attributes, so the
/// resolved-line / function-line relationship can vary).
fn function_for_hint<'a>(items: &'a [AirItem], hint: &AirHint) -> Option<&'a AirFunction> {
    let target = hint.target_span.as_ref()?;
    items.iter().find_map(|item| match item {
        AirItem::Function(f) => {
            if f.span.line_start <= target.line_start && target.line_start <= f.span.line_end {
                Some(f)
            } else {
                None
            }
        }
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use locus_air::{
        AIR_SCHEMA_VERSION, AirFile, AirHint, AirPackage, AirSpan, AirWorkspace, Visibility,
    };

    fn func(symbol: &str, line: u32) -> AirItem {
        AirItem::Function(AirFunction {
            name: symbol.rsplit("::").next().unwrap_or(symbol).into(),
            symbol: symbol.into(),
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

    fn marks_hint(fact_kind: &str, target_line: u32) -> AirHint {
        AirHint {
            kind: HintKind::MarksFact {
                fact_kind: fact_kind.into(),
            },
            raw: format!("// locus: fact {fact_kind}"),
            span: AirSpan::new("t.rs", target_line - 1, target_line - 1),
            target_span: Some(AirSpan::new("t.rs", target_line, target_line)),
        }
    }

    fn workspace(items: Vec<AirItem>, hints: Vec<AirHint>) -> AirWorkspace {
        AirWorkspace {
            schema_version: AIR_SCHEMA_VERSION,
            packages: vec![AirPackage {
                name: "x".into(),
                version: "0".into(),
                root_dir: "/".into(),
                files: vec![AirFile {
                    path: "t.rs".into(),
                    module_path: Some("x::handler".into()),
                    items,
                    hints,
                    parse_error: None,
                    line_count: 50,
                }],
            }],
            facts: Vec::new(),
        }
    }

    #[test]
    fn marker_promotes_to_hot_path_fact() {
        let air = workspace(
            vec![func("x::handler::tick", 10)],
            vec![marks_hint("hot_path", 10)],
        );
        let facts = MarkersLoader.enrich(&air);
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].kind, FactKind::HotPath);
        match &facts[0].target {
            FactTarget::Function { symbol } => assert_eq!(symbol, "x::handler::tick"),
            other => panic!("expected Function target; got {other:?}"),
        }
        assert_eq!(facts[0].source, "markers");
    }

    #[test]
    fn every_known_fact_kind_round_trips() {
        let known = [
            "spawned_work",
            "config_read",
            "logging",
            "external_io",
            "persistence_write",
            "blocking_call",
            "hot_path",
            "request_context",
            "boundary_entry",
            "runtime_state_owner",
            "background_worker",
        ];
        for name in known {
            let air = workspace(vec![func("x::handler::f", 10)], vec![marks_hint(name, 10)]);
            let facts = MarkersLoader.enrich(&air);
            assert_eq!(facts.len(), 1, "marker `{name}` produced no facts");
        }
    }

    #[test]
    fn unknown_marker_is_silently_skipped() {
        let air = workspace(
            vec![func("x::handler::f", 10)],
            vec![marks_hint("policy_decision", 10)],
        );
        let facts = MarkersLoader.enrich(&air);
        assert!(facts.is_empty());
    }

    #[test]
    fn marker_without_target_span_is_skipped() {
        let mut hint = marks_hint("hot_path", 10);
        hint.target_span = None;
        let air = workspace(vec![func("x::handler::f", 10)], vec![hint]);
        assert!(MarkersLoader.enrich(&air).is_empty());
    }

    #[test]
    fn marker_pointing_at_no_function_is_skipped() {
        // target_span line 99, no function there
        let air = workspace(
            vec![func("x::handler::f", 10)],
            vec![marks_hint("hot_path", 99)],
        );
        assert!(MarkersLoader.enrich(&air).is_empty());
    }

    #[test]
    fn loader_name_is_markers() {
        assert_eq!(MarkersLoader.name(), "markers");
    }
}
