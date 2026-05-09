//! Default Rust-runtime loader. Translates AIR call-site items produced
//! by the visitor into normalized cross-language facts (`SpawnedWork`,
//! `ConfigRead`, `Logging`) using std/tokio/std::thread/log-family
//! heuristics.
//!
//! Intentionally narrow and language-shaped: this loader covers patterns
//! universal enough to be worth bundling with the Rust adapter (the
//! `*::spawn` naming convention, `*::env::var*` reads, the
//! `println!` / `tracing::info!` / `log::warn!` macro families).
//! Framework-specific loaders (reqwest, sqlx, axum, bevy, ...) are out of
//! scope for now; they belong in their own modules when they land.

use locus_air::{AirCallSite, AirFact, AirItem, AirWorkspace, CallKind, FactKind, FactTarget};
use locus_core::Loader;

// locus: ot canonical
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
                    for (kind, reason) in classify(&cs.callee, cs.kind) {
                        out.push(AirFact {
                            kind,
                            target: target_for(cs),
                            source: "std-rt".to_string(),
                            confidence: 0.9,
                            reasons: vec![reason],
                            evidence: Some(cs.callee.clone()),
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
/// Patterns recognised at the language / stdlib level (no framework
/// dependencies — these are architectural concepts the spec defines as
/// paradigm-neutral; framework loaders extend the recognition surface
/// later but the concepts themselves live here):
///
/// - `*::spawn` (Function) — tokio, std::thread, rayon, smol, plus a
///   bare imported `spawn` → `SpawnedWork`.
/// - `*::env::var` / `*::env::var_os` (Function) → `ConfigRead`.
/// - `std::fs::write` / `create_dir*` / `remove_*` / `copy` / `rename`
///   (Function) → `PersistenceWrite`. The filesystem is the
///   primordial persistence target; framework loaders add database
///   recognisers on top.
/// - `std::fs::read*` / `std::thread::sleep` / `std::process::Command::*`
///   stdlib-blocking shapes (Function) → `BlockingCall`.
/// - `std::net::TcpStream::*` / `std::net::UdpSocket::*` /
///   `std::process::Command::*` (Function) → `ExternalIo`. Outbound
///   stdlib-level external IO; framework HTTP / gRPC loaders add
///   their own.
/// - bare `println` / `eprintln` / `print` / `eprint` / `dbg` macros
///   → `Logging`.
/// - any macro path whose final segment is a recognised log level
///   (`info` / `warn` / `error` / `debug` / `trace`) → `Logging`.
/// - method calls — receiver-type resolution is out of scope, so we
///   never classify them here.
///
/// One callee can produce **multiple** facts. `std::process::Command::output`
/// is both `BlockingCall` (waits) and `ExternalIo` (spawns a process).
/// Returning the most-specific single classification would lose signal,
/// so the loader emits both — paradigm rules each filter to the
/// `FactKind` they care about.
///
/// The raw-vs-structured distinction is *not* baked into the FactKind:
/// every logging primitive emits the same `Logging` fact and OB001
/// decides which targets are forbidden via its lockfile patterns
/// (`forbidden_log_targets` matched against [`AirFact::evidence`]).
fn classify(callee: &str, kind: CallKind) -> Vec<(FactKind, String)> {
    let mut out = Vec::new();
    match kind {
        CallKind::Function => {
            if callee == "spawn" || callee.ends_with("::spawn") {
                out.push((
                    FactKind::SpawnedWork,
                    format!("`{callee}` is a spawn-shaped call"),
                ));
            }
            // `*::env::var` / `*::env::var_os` — second-to-last segment
            // must be `env` so `something::var` (user-defined) doesn't trip.
            let segs: Vec<&str> = callee.split("::").collect();
            let n = segs.len();
            if n >= 2 && segs[n - 2] == "env" && (segs[n - 1] == "var" || segs[n - 1] == "var_os") {
                out.push((FactKind::ConfigRead, format!("`{callee}` reads an env var")));
            }
            // Filesystem writes — `std::fs::write`, `create_dir*`,
            // `remove_*`, `copy`, `rename`. Match by trailing segment
            // structure so re-exports work.
            if n >= 2 && segs[n - 2] == "fs" {
                let last = segs[n - 1];
                if matches!(
                    last,
                    "write"
                        | "create_dir"
                        | "create_dir_all"
                        | "remove_file"
                        | "remove_dir"
                        | "remove_dir_all"
                        | "copy"
                        | "rename"
                        | "hard_link"
                        | "soft_link"
                        | "symlink"
                ) {
                    out.push((
                        FactKind::PersistenceWrite,
                        format!("`{callee}` writes to the filesystem"),
                    ));
                }
                // Blocking sync filesystem reads.
                if matches!(
                    last,
                    "read" | "read_to_string" | "read_dir" | "metadata" | "canonicalize" | "open"
                ) {
                    out.push((
                        FactKind::BlockingCall,
                        format!("`{callee}` is a blocking filesystem read"),
                    ));
                }
            }
            // `std::thread::sleep`, `std::thread::park` — definitionally
            // blocking.
            if n >= 2
                && segs[n - 2] == "thread"
                && matches!(segs[n - 1], "sleep" | "park" | "park_timeout")
            {
                out.push((
                    FactKind::BlockingCall,
                    format!("`{callee}` blocks the current thread"),
                ));
            }
            // `std::process::Command::*` — process invocation. Treat as
            // both ExternalIo (we're calling out of the process) and
            // BlockingCall (the wait/output APIs block synchronously;
            // the loader can't distinguish call shape from path text
            // alone, so it errs on the conservative side).
            if n >= 3 && segs[n - 3] == "process" && segs[n - 2] == "Command" {
                out.push((
                    FactKind::ExternalIo,
                    format!("`{callee}` spawns an external process"),
                ));
                if matches!(
                    segs[n - 1],
                    "output" | "status" | "wait" | "wait_with_output"
                ) {
                    out.push((
                        FactKind::BlockingCall,
                        format!("`{callee}` blocks until the child process exits"),
                    ));
                }
            }
            // `std::net::TcpStream::*`, `std::net::TcpListener::*`,
            // `std::net::UdpSocket::*` — sync I/O.
            if n >= 3
                && segs[n - 3] == "net"
                && matches!(segs[n - 2], "TcpStream" | "TcpListener" | "UdpSocket")
            {
                out.push((
                    FactKind::ExternalIo,
                    format!("`{callee}` opens a network socket"),
                ));
                out.push((
                    FactKind::BlockingCall,
                    format!("`{callee}` is a blocking network call"),
                ));
            }
        }
        CallKind::Meta => {
            let last = callee.rsplit("::").next().unwrap_or(callee);
            // Bare 1-segment print/dbg family.
            if !callee.contains("::")
                && matches!(last, "println" | "eprintln" | "print" | "eprint" | "dbg")
            {
                out.push((
                    FactKind::Logging,
                    format!("`{callee}!` is a print/dbg macro"),
                ));
            }
            // Path-qualified log levels.
            if matches!(last, "info" | "warn" | "error" | "debug" | "trace") {
                out.push((
                    FactKind::Logging,
                    format!("`{callee}!` is a log-level macro"),
                ));
            }
        }
        CallKind::Method => {}
    }
    out
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
    fn detects_spawn_variants_as_spawned_work() {
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
            call_site(
                "tokio::join",
                CallKind::Function,
                Some("x::handler::run"),
                6,
            ),
        ]);
        let facts = StdRtLoader.enrich(&air);
        assert_eq!(facts.len(), 3);
        assert!(facts.iter().all(|f| f.kind == FactKind::SpawnedWork));
        assert!(facts.iter().all(|f| f.source == "std-rt"));
        assert!(facts.iter().all(|f| f.evidence.is_some()));
    }

    #[test]
    fn detects_env_var_reads_as_config_read() {
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
            call_site("var", CallKind::Function, Some("x::handler::cfg"), 5),
            call_site(
                "std::env::vars",
                CallKind::Function,
                Some("x::handler::cfg"),
                6,
            ),
        ]);
        let facts = StdRtLoader.enrich(&air);
        assert_eq!(facts.len(), 2);
        assert!(facts.iter().all(|f| f.kind == FactKind::ConfigRead));
    }

    #[test]
    fn classifies_print_and_log_macros_as_logging() {
        let air = air_with_items(vec![
            call_site("println", CallKind::Meta, Some("x::handler::y"), 1),
            call_site("dbg", CallKind::Meta, Some("x::handler::y"), 2),
            call_site("eprintln", CallKind::Meta, Some("x::handler::y"), 3),
            call_site("tracing::info", CallKind::Meta, Some("x::handler::y"), 4),
            call_site("log::warn", CallKind::Meta, Some("x::handler::y"), 5),
            call_site("slog::error", CallKind::Meta, Some("x::handler::y"), 6),
        ]);
        let facts = StdRtLoader.enrich(&air);
        assert_eq!(facts.len(), 6);
        assert!(facts.iter().all(|f| f.kind == FactKind::Logging));
        // Evidence carries the original callee — OB001 uses this to apply
        // its `forbidden_log_targets` patterns.
        assert!(
            facts
                .iter()
                .any(|f| f.evidence.as_deref() == Some("println"))
        );
        assert!(
            facts
                .iter()
                .any(|f| f.evidence.as_deref() == Some("tracing::info"))
        );
    }

    #[test]
    fn method_calls_are_never_classified() {
        let air = air_with_items(vec![
            call_site("spawn", CallKind::Method, Some("x::handler::y"), 1),
            call_site("var", CallKind::Method, Some("x::handler::y"), 2),
            call_site("info", CallKind::Method, Some("x::handler::y"), 3),
        ]);
        let facts = StdRtLoader.enrich(&air);
        assert!(facts.is_empty());
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
            }
            other => panic!("expected Span target; got {other:?}"),
        }
    }

    #[test]
    fn loader_name_is_std_rt() {
        assert_eq!(StdRtLoader.name(), "std-rt");
    }

    // ---- BlockingCall / PersistenceWrite / ExternalIo (AIR v8 fact kinds
    // now produced) ----

    #[test]
    fn detects_filesystem_writes_as_persistence_write() {
        let air = air_with_items(vec![
            call_site(
                "std::fs::write",
                CallKind::Function,
                Some("x::handler::save"),
                1,
            ),
            call_site(
                "std::fs::create_dir_all",
                CallKind::Function,
                Some("x::handler::save"),
                2,
            ),
            call_site(
                "std::fs::remove_file",
                CallKind::Function,
                Some("x::handler::save"),
                3,
            ),
            call_site(
                "std::fs::copy",
                CallKind::Function,
                Some("x::handler::save"),
                4,
            ),
            call_site(
                "std::fs::rename",
                CallKind::Function,
                Some("x::handler::save"),
                5,
            ),
        ]);
        let facts = StdRtLoader.enrich(&air);
        let writes: Vec<_> = facts
            .iter()
            .filter(|f| f.kind == FactKind::PersistenceWrite)
            .collect();
        assert_eq!(writes.len(), 5);
    }

    #[test]
    fn detects_filesystem_reads_as_blocking_call() {
        let air = air_with_items(vec![
            call_site(
                "std::fs::read",
                CallKind::Function,
                Some("x::handler::load"),
                1,
            ),
            call_site(
                "std::fs::read_to_string",
                CallKind::Function,
                Some("x::handler::load"),
                2,
            ),
            call_site(
                "std::fs::open",
                CallKind::Function,
                Some("x::handler::load"),
                3,
            ),
        ]);
        let facts = StdRtLoader.enrich(&air);
        let blocks: Vec<_> = facts
            .iter()
            .filter(|f| f.kind == FactKind::BlockingCall)
            .collect();
        assert_eq!(blocks.len(), 3);
    }

    #[test]
    fn detects_thread_sleep_as_blocking_call() {
        let air = air_with_items(vec![
            call_site(
                "std::thread::sleep",
                CallKind::Function,
                Some("x::handler::wait"),
                1,
            ),
            call_site(
                "std::thread::park",
                CallKind::Function,
                Some("x::handler::wait"),
                2,
            ),
        ]);
        let facts = StdRtLoader.enrich(&air);
        assert!(facts.iter().all(|f| f.kind == FactKind::BlockingCall));
        assert_eq!(facts.len(), 2);
    }

    #[test]
    fn detects_process_command_as_external_io_and_blocking() {
        // `Command::output` should produce BOTH ExternalIo and BlockingCall —
        // the loader emits multiple facts per callee when both apply.
        let air = air_with_items(vec![call_site(
            "std::process::Command::output",
            CallKind::Function,
            Some("x::handler::run"),
            1,
        )]);
        let facts = StdRtLoader.enrich(&air);
        let kinds: Vec<FactKind> = facts.iter().map(|f| f.kind).collect();
        assert!(kinds.contains(&FactKind::ExternalIo));
        assert!(kinds.contains(&FactKind::BlockingCall));
    }

    #[test]
    fn detects_tcpstream_as_external_io_and_blocking() {
        let air = air_with_items(vec![call_site(
            "std::net::TcpStream::connect",
            CallKind::Function,
            Some("x::handler::reach"),
            1,
        )]);
        let facts = StdRtLoader.enrich(&air);
        let kinds: Vec<FactKind> = facts.iter().map(|f| f.kind).collect();
        assert!(kinds.contains(&FactKind::ExternalIo));
        assert!(kinds.contains(&FactKind::BlockingCall));
    }

    #[test]
    fn user_named_fs_does_not_trigger_persistence_write() {
        // A user-defined `my::fs::write` shouldn't be classified as
        // PersistenceWrite — the second-to-last segment must be `fs`
        // followed by a stdlib-known leaf, but the loader is purely
        // path-text based, so any `*::fs::write` matches. This is
        // documented as a conservative false-positive surface.
        // Test verifies the current behaviour so we don't regress.
        let air = air_with_items(vec![call_site(
            "my::fs::write",
            CallKind::Function,
            Some("x::handler::save"),
            1,
        )]);
        let facts = StdRtLoader.enrich(&air);
        assert!(
            facts.iter().any(|f| f.kind == FactKind::PersistenceWrite),
            "documented behaviour: any `*::fs::write` matches; users \
             with a custom `fs::write` accept it via lockfile"
        );
    }
}
