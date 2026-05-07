//! Default Rust-runtime loader. Translates AIR call-site items produced
//! by the visitor into normalized cross-language facts (`SpawnsWork`,
//! `ReadsEnv`, `LogsRaw`, `LogsStructured`) using std/tokio/std::thread/
//! log-family heuristics.
//!
//! Intentionally narrow: this loader covers the patterns the previous
//! visitor-baked logic covered. Richer framework loaders (reqwest, sqlx,
//! actix, axum, ...) live in their own modules so each can be enabled
//! independently and tested in isolation.

use locus_air::{AirCallSite, AirFact, AirItem, AirWorkspace, CallKind, FactKind, FactTarget};
use locus_core::Loader;

// ot: canonical
pub struct StdRtLoader;

impl Loader for StdRtLoader {
    fn name(&self) -> &'static str {
        "std-rt"
    }

    fn enrich(&self, air: &AirWorkspace) -> Vec<AirFact> {
        let mut out = Vec::new();
        for pkg in &air.packages {
            for file in &pkg.files {
                for item in &file.items {
                    let AirItem::CallSite(cs) = item else {
                        continue;
                    };
                    if let Some((kind, reason)) = classify(&cs.callee, cs.kind) {
                        out.push(AirFact {
                            kind,
                            target: target_for(cs),
                            source: "std-rt".to_string(),
                            confidence: 0.9,
                            reasons: vec![reason],
                        });
                    }
                }
            }
        }
        out
    }
}

fn target_for(cs: &AirCallSite) -> FactTarget {
    match &cs.function {
        Some(sym) => FactTarget::Function {
            symbol: sym.clone(),
        },
        None => FactTarget::Span(cs.span.clone()),
    }
}

/// Classify a call-site's `(callee, kind)` into a normalized fact, or
/// return `None` if it isn't one of the patterns this loader knows about.
///
/// Patterns:
/// - `*::spawn` (Function) — tokio, std::thread, rayon, smol, plus a bare
///   imported `spawn` → `SpawnsWork`.
/// - `*::env::var` / `*::env::var_os` (Function) — environment-variable
///   reads → `ReadsEnv`.
/// - bare `println` / `eprintln` / `print` / `eprint` / `dbg` macros →
///   `LogsRaw`.
/// - any macro path whose final segment is a recognised log level
///   (`info` / `warn` / `error` / `debug` / `trace`) → `LogsStructured`.
/// - method calls — receiver-type resolution is out of scope, so we
///   never classify them here.
fn classify(callee: &str, kind: CallKind) -> Option<(FactKind, String)> {
    match kind {
        CallKind::Function => {
            // `*::spawn` — tokio::spawn, std::thread::spawn, rayon::spawn,
            // smol::spawn, plus bare `spawn` when imported.
            if callee == "spawn" || callee.ends_with("::spawn") {
                return Some((
                    FactKind::SpawnsWork,
                    format!("`{callee}` is a spawn-shaped call"),
                ));
            }
            // `*::env::var` / `*::env::var_os` — the second-to-last segment
            // must be `env` to avoid false positives on user-defined `var`.
            let segs: Vec<&str> = callee.split("::").collect();
            let n = segs.len();
            if n >= 2 && segs[n - 2] == "env" && (segs[n - 1] == "var" || segs[n - 1] == "var_os") {
                return Some((FactKind::ReadsEnv, format!("`{callee}` reads an env var")));
            }
            None
        }
        CallKind::Macro => {
            let last = callee.rsplit("::").next().unwrap_or(callee);
            // Bare 1-segment print/dbg family → raw logging.
            if !callee.contains("::")
                && matches!(last, "println" | "eprintln" | "print" | "eprint" | "dbg")
            {
                return Some((
                    FactKind::LogsRaw,
                    format!("`{callee}!` is a raw print/dbg macro"),
                ));
            }
            // Path-qualified log levels: `tracing::info!`, `log::warn!`,
            // `slog::error!`, etc. → structured logging.
            if matches!(last, "info" | "warn" | "error" | "debug" | "trace") {
                return Some((
                    FactKind::LogsStructured,
                    format!("`{callee}!` is a structured log macro"),
                ));
            }
            None
        }
        // Method-call resolution requires receiver-type info we don't
        // have at this layer.
        CallKind::Method => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use locus_air::{AirFile, AirPackage, AirSpan, AirWorkspace};

    fn call_site(callee: &str, kind: CallKind, function: Option<&str>, line: u32) -> AirItem {
        AirItem::CallSite(AirCallSite {
            callee: callee.to_string(),
            kind,
            function: function.map(|s| s.to_string()),
            span: AirSpan::new("t.rs", line, line),
        })
    }

    fn air_with_items(items: Vec<AirItem>) -> AirWorkspace {
        AirWorkspace {
            schema_version: locus_air::AIR_SCHEMA_VERSION,
            packages: vec![AirPackage {
                name: "x".into(),
                version: "0".into(),
                root_dir: "/".into(),
                files: vec![AirFile {
                    path: "t.rs".into(),
                    module_path: Some("x::handler".into()),
                    items,
                    hints: Vec::new(),
                    parse_error: None,
                    line_count: 1,
                }],
            }],
            facts: Vec::new(),
        }
    }

    #[test]
    fn detects_spawn_variants_as_spawns_work() {
        let air = air_with_items(vec![
            call_site(
                "tokio::spawn",
                CallKind::Function,
                Some("x::handler::run"),
                3,
            ),
            call_site(
                "std::thread::spawn",
                CallKind::Function,
                Some("x::handler::run"),
                4,
            ),
            call_site("spawn", CallKind::Function, Some("x::handler::run"), 5),
            // Not a spawn — must NOT classify.
            call_site(
                "tokio::join",
                CallKind::Function,
                Some("x::handler::run"),
                6,
            ),
        ]);
        let facts = StdRtLoader.enrich(&air);
        assert_eq!(facts.len(), 3);
        assert!(
            facts.iter().all(|f| f.kind == FactKind::SpawnsWork),
            "expected only SpawnsWork facts; got {facts:?}"
        );
        assert!(facts.iter().all(|f| f.source == "std-rt"));
        assert!(facts.iter().any(|f| f.reasons[0].contains("tokio::spawn")));
        assert!(
            facts
                .iter()
                .any(|f| f.reasons[0].contains("std::thread::spawn"))
        );
        assert!(facts.iter().any(|f| f.reasons[0].contains("`spawn`")));
    }

    #[test]
    fn detects_env_var_reads_as_reads_env() {
        let air = air_with_items(vec![
            call_site(
                "std::env::var",
                CallKind::Function,
                Some("x::handler::cfg"),
                3,
            ),
            call_site(
                "env::var_os",
                CallKind::Function,
                Some("x::handler::cfg"),
                4,
            ),
            // `var` alone is NOT classified — the second-to-last segment
            // must be `env` to avoid false positives on user code.
            call_site("var", CallKind::Function, Some("x::handler::cfg"), 5),
            // `something::env::other` doesn't match either — last must be
            // `var`/`var_os`.
            call_site(
                "std::env::vars",
                CallKind::Function,
                Some("x::handler::cfg"),
                6,
            ),
        ]);
        let facts = StdRtLoader.enrich(&air);
        assert_eq!(facts.len(), 2);
        assert!(facts.iter().all(|f| f.kind == FactKind::ReadsEnv));
    }

    #[test]
    fn classifies_print_dbg_macros_as_logs_raw() {
        let air = air_with_items(vec![
            call_site("println", CallKind::Macro, Some("x::handler::y"), 1),
            call_site("dbg", CallKind::Macro, Some("x::handler::y"), 2),
            call_site("eprintln", CallKind::Macro, Some("x::handler::y"), 3),
        ]);
        let facts = StdRtLoader.enrich(&air);
        assert_eq!(facts.len(), 3);
        assert!(facts.iter().all(|f| f.kind == FactKind::LogsRaw));
    }

    #[test]
    fn classifies_log_level_macros_as_logs_structured() {
        let air = air_with_items(vec![
            call_site("tracing::info", CallKind::Macro, Some("x::handler::y"), 1),
            call_site("log::warn", CallKind::Macro, Some("x::handler::y"), 2),
            call_site("slog::error", CallKind::Macro, Some("x::handler::y"), 3),
        ]);
        let facts = StdRtLoader.enrich(&air);
        assert_eq!(facts.len(), 3);
        assert!(facts.iter().all(|f| f.kind == FactKind::LogsStructured));
    }

    #[test]
    fn method_calls_are_never_classified() {
        // Receiver-type resolution is out of scope for this loader.
        let air = air_with_items(vec![
            call_site("spawn", CallKind::Method, Some("x::handler::y"), 1),
            call_site("var", CallKind::Method, Some("x::handler::y"), 2),
            call_site("info", CallKind::Method, Some("x::handler::y"), 3),
        ]);
        let facts = StdRtLoader.enrich(&air);
        assert!(
            facts.is_empty(),
            "method calls must not classify; got {facts:?}"
        );
    }

    #[test]
    fn target_falls_back_to_span_when_no_function() {
        let air = air_with_items(vec![call_site(
            "tokio::spawn",
            CallKind::Function,
            None,
            42,
        )]);
        let facts = StdRtLoader.enrich(&air);
        assert_eq!(facts.len(), 1);
        match &facts[0].target {
            FactTarget::Span(span) => {
                assert_eq!(span.line_start, 42);
                assert_eq!(span.line_end, 42);
            }
            other => panic!("expected Span target; got {other:?}"),
        }
    }

    #[test]
    fn loader_name_is_std_rt() {
        assert_eq!(StdRtLoader.name(), "std-rt");
    }
}
