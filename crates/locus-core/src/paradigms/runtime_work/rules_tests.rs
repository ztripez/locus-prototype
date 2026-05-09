//! Tests for [`super`] rule implementations.
//!
//! Extracted from `rules.rs` to keep the production module within the
//! CX002 line budget. Re-attached via `#[path = "rules_tests.rs"] mod
//! tests;` at the bottom of `rules.rs`.

use super::*;
use locus_air::{
    AIR_SCHEMA_VERSION, AirField, AirFile, AirFunction, AirPackage, AirSpan, AirWorkspace,
    Visibility,
};

fn func(symbol: &str, file: &str, line: u32) -> AirItem {
    AirItem::Function(AirFunction {
        name: symbol.rsplit("::").next().unwrap_or(symbol).into(),
        symbol: symbol.into(),
        visibility: Visibility::Public,
        params: Vec::new(),
        return_type: None,
        span: AirSpan::new(file, line, line + 5),
        line_count: 6,
        decorators: Vec::new(),
        symbol_segments: Vec::new(),
        doc: None,
    })
}

fn spawn_fact(symbol: &str, reason: &str) -> AirFact {
    AirFact {
        kind: FactKind::SpawnedWork,
        target: FactTarget::Function {
            symbol: symbol.into(),
        },
        source: "test".into(),
        confidence: 1.0,
        reasons: vec![reason.into()],
        evidence: Some("tokio::spawn".into()),
    }
}

fn air_with_file(
    module_path: Option<&str>,
    file_path: &str,
    items: Vec<AirItem>,
    facts: Vec<AirFact>,
) -> AirWorkspace {
    AirWorkspace {
        schema_version: AIR_SCHEMA_VERSION,
        packages: vec![AirPackage {
            name: "x".into(),
            version: "0".into(),
            root_dir: "/".into(),
            files: vec![AirFile {
                path: file_path.into(),
                module_path: module_path.map(|s| s.into()),
                items,
                hints: Vec::new(),
                parse_error: None,
                line_count: 1,
            }],
        }],
        facts,
    }
}

#[test]
fn rw001_fires_on_spawn_in_non_runtime_owner_file() {
    let air = air_with_file(
        Some("crate::handler"),
        "src/handler.rs",
        vec![func("crate::handler::create_user", "src/handler.rs", 17)],
        vec![spawn_fact(
            "crate::handler::create_user",
            "`tokio::spawn` is a spawn-shaped call",
        )],
    );
    let section = RwSection {
        runtime_owner_paths: vec!["crate::runtime::*".into(), "bin::*".into()],
        ..RwSection::default()
    };
    let diags = rw001(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "RW001");
    assert_eq!(diags[0].severity, Severity::Fatal);
    assert_eq!(diags[0].span.line_start, 17);
    assert!(diags[0].message.contains("crate::handler"));
    assert!(diags[0].message.contains("crate::handler::create_user"));
    assert!(
        diags[0]
            .why
            .iter()
            .any(|w| w.contains("runtime_owner_paths")),
        "expected lockfile pattern reason; got {:?}",
        diags[0].why
    );
    assert!(
        diags[0].why.iter().any(|w| w.contains("spawn-shaped")),
        "expected spawn-shape reason; got {:?}",
        diags[0].why
    );
    assert!(
        diags[0].why.iter().any(|w| w.contains("create_user")),
        "expected enclosing-function reason; got {:?}",
        diags[0].why
    );
}

#[test]
fn rw001_quiet_on_spawn_in_runtime_owner_pattern_file() {
    let air = air_with_file(
        Some("crate::runtime::pool"),
        "src/runtime/pool.rs",
        vec![func("crate::runtime::pool::run", "src/runtime/pool.rs", 4)],
        vec![spawn_fact("crate::runtime::pool::run", "spawn detected")],
    );
    let section = RwSection {
        runtime_owner_paths: vec!["crate::runtime::*".into()],
        ..RwSection::default()
    };
    assert!(rw001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn rw001_quiet_on_non_spawnswork_facts() {
    let air = air_with_file(
        Some("crate::handler"),
        "src/handler.rs",
        vec![func("crate::handler::cfg", "src/handler.rs", 5)],
        vec![
            AirFact {
                kind: FactKind::ConfigRead,
                target: FactTarget::Function {
                    symbol: "crate::handler::cfg".into(),
                },
                source: "test".into(),
                confidence: 1.0,
                reasons: Vec::new(),
                evidence: None,
            },
            AirFact {
                kind: FactKind::Logging,
                target: FactTarget::Function {
                    symbol: "crate::handler::cfg".into(),
                },
                source: "test".into(),
                confidence: 1.0,
                reasons: Vec::new(),
                evidence: None,
            },
        ],
    );
    let section = RwSection {
        runtime_owner_paths: vec!["crate::runtime::*".into()],
        ..RwSection::default()
    };
    assert!(rw001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn rw001_silent_when_runtime_owner_paths_empty() {
    let air = air_with_file(
        Some("crate::handler"),
        "src/handler.rs",
        vec![func("crate::handler::create_user", "src/handler.rs", 17)],
        vec![spawn_fact("crate::handler::create_user", "spawn detected")],
    );
    let section = RwSection::default();
    assert!(
        rw001(&air, &section, CheckMode::Human).is_empty(),
        "rule should wait for explicit runtime_owner_paths declaration"
    );
}

#[test]
fn rw001_skips_files_without_module_path() {
    let air = air_with_file(
        None,
        "src/build.rs",
        vec![func("build::main", "src/build.rs", 2)],
        vec![spawn_fact("build::main", "spawn detected")],
    );
    let section = RwSection {
        runtime_owner_paths: vec!["crate::runtime::*".into()],
        ..RwSection::default()
    };
    assert!(rw001(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn rw001_agent_strict_keeps_fatal() {
    let air = air_with_file(
        Some("crate::handler"),
        "src/handler.rs",
        vec![func("crate::handler::process", "src/handler.rs", 12)],
        vec![spawn_fact(
            "crate::handler::process",
            "`rayon::spawn` is a spawn-shaped call",
        )],
    );
    let section = RwSection {
        runtime_owner_paths: vec!["crate::runtime::*".into()],
        ..RwSection::default()
    };
    let diags = rw001(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

// ---------- RW002 ----------

fn blocking_fact(symbol: &str, evidence: &str, reason: &str) -> AirFact {
    AirFact {
        kind: FactKind::BlockingCall,
        target: FactTarget::Function {
            symbol: symbol.into(),
        },
        source: "std-rt".into(),
        confidence: 1.0,
        reasons: vec![reason.into()],
        evidence: Some(evidence.into()),
    }
}

#[test]
fn rw002_fires_on_blocking_in_non_runtime_owner_file() {
    let air = air_with_file(
        Some("crate::handler"),
        "src/handler.rs",
        vec![func("crate::handler::create_user", "src/handler.rs", 17)],
        vec![blocking_fact(
            "crate::handler::create_user",
            "std::fs::read",
            "`std::fs::read` is a blocking-shaped call",
        )],
    );
    let section = RwSection {
        runtime_owner_paths: vec!["crate::runtime::*".into()],
        ..RwSection::default()
    };
    let diags = rw002(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    let d = &diags[0];
    assert_eq!(d.rule_id, "RW002");
    assert_eq!(d.severity, Severity::Warning);
    assert_eq!(d.span.line_start, 17);
    assert!(d.message.contains("crate::handler"));
    assert!(d.message.contains("crate::handler::create_user"));
    assert!(d.message.contains("std::fs::read"));
    assert!(
        d.why.iter().any(|w| w.contains("runtime_owner_paths")),
        "expected lockfile pattern reason; got {:?}",
        d.why
    );
    assert!(
        d.why.iter().any(|w| w.contains("blocking-shaped")),
        "expected loader reason; got {:?}",
        d.why
    );
}

#[test]
fn rw002_quiet_in_runtime_owner_file() {
    let air = air_with_file(
        Some("crate::runtime::worker"),
        "src/runtime/worker.rs",
        vec![func(
            "crate::runtime::worker::run",
            "src/runtime/worker.rs",
            4,
        )],
        vec![blocking_fact(
            "crate::runtime::worker::run",
            "std::thread::sleep",
            "blocking detected",
        )],
    );
    let section = RwSection {
        runtime_owner_paths: vec!["crate::runtime::*".into()],
        ..RwSection::default()
    };
    assert!(rw002(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn rw002_quiet_on_other_fact_kinds() {
    // Don't react to spawn/log/config/persistence/io facts — those are
    // other rules' jobs.
    let air = air_with_file(
        Some("crate::handler"),
        "src/handler.rs",
        vec![func("crate::handler::touch", "src/handler.rs", 5)],
        vec![
            AirFact {
                kind: FactKind::SpawnedWork,
                target: FactTarget::Function {
                    symbol: "crate::handler::touch".into(),
                },
                source: "std-rt".into(),
                confidence: 1.0,
                reasons: Vec::new(),
                evidence: Some("tokio::spawn".into()),
            },
            AirFact {
                kind: FactKind::Logging,
                target: FactTarget::Function {
                    symbol: "crate::handler::touch".into(),
                },
                source: "std-rt".into(),
                confidence: 1.0,
                reasons: Vec::new(),
                evidence: None,
            },
            AirFact {
                kind: FactKind::PersistenceWrite,
                target: FactTarget::Function {
                    symbol: "crate::handler::touch".into(),
                },
                source: "std-rt".into(),
                confidence: 1.0,
                reasons: Vec::new(),
                evidence: Some("std::fs::write".into()),
            },
        ],
    );
    let section = RwSection {
        runtime_owner_paths: vec!["crate::runtime::*".into()],
        ..RwSection::default()
    };
    assert!(rw002(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn rw002_silent_when_runtime_owner_paths_empty() {
    let air = air_with_file(
        Some("crate::handler"),
        "src/handler.rs",
        vec![func("crate::handler::create_user", "src/handler.rs", 17)],
        vec![blocking_fact(
            "crate::handler::create_user",
            "std::fs::read",
            "blocking detected",
        )],
    );
    let section = RwSection::default();
    assert!(
        rw002(&air, &section, CheckMode::Human).is_empty(),
        "rule should wait for explicit runtime_owner_paths declaration"
    );
}

#[test]
fn rw002_agent_strict_elevates_warning_to_fatal() {
    let air = air_with_file(
        Some("crate::handler"),
        "src/handler.rs",
        vec![func("crate::handler::process", "src/handler.rs", 12)],
        vec![blocking_fact(
            "crate::handler::process",
            "Command::output",
            "blocking detected",
        )],
    );
    let section = RwSection {
        runtime_owner_paths: vec!["crate::runtime::*".into()],
        ..RwSection::default()
    };
    let diags = rw002(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

#[test]
fn rw002_segment_anywhere_pattern_exempts_inline_test_module() {
    // Inline `mod tests {}` blocks live at a deeper symbol path than
    // the file; the function-symbol check has to catch them when the
    // file's `module_path` doesn't itself match.
    let air = air_with_file(
        Some("crate::handler"),
        "src/handler.rs",
        vec![func(
            "crate::handler::tests::reads_fixture",
            "src/handler.rs",
            42,
        )],
        vec![blocking_fact(
            "crate::handler::tests::reads_fixture",
            "std::fs::read",
            "blocking detected",
        )],
    );
    let section = RwSection {
        runtime_owner_paths: vec!["*::tests::*".into()],
        ..RwSection::default()
    };
    assert!(
        rw002(&air, &section, CheckMode::Human).is_empty(),
        "function-symbol match should exempt inline test modules"
    );
}

// ---------- RW003 / RW004 helpers ----------

fn ty(name: &str, kind: TypeKind, fields: Vec<(&str, &str)>, file: &str, line: u32) -> AirItem {
    AirItem::Type(AirType {
        kind,
        name: name.into(),
        symbol: format!("crate::{name}"),
        visibility: Visibility::Public,
        fields: fields
            .into_iter()
            .map(|(n, t)| AirField {
                name: n.into(),
                type_text: t.into(),
                visibility: Visibility::Public,
            })
            .collect(),
        variants: Vec::new(),
        decorators: Vec::new(),
        symbol_segments: Vec::new(),
        span: AirSpan::new(file, line, line + 4),
        doc: None,
    })
}

fn air_with_types(module: Option<&str>, file: &str, items: Vec<AirItem>) -> AirWorkspace {
    AirWorkspace {
        schema_version: AIR_SCHEMA_VERSION,
        packages: vec![AirPackage {
            name: "x".into(),
            version: "0".into(),
            root_dir: "/".into(),
            files: vec![AirFile {
                path: file.into(),
                module_path: module.map(|s| s.into()),
                items,
                hints: Vec::new(),
                parse_error: None,
                line_count: 1,
            }],
        }],
        facts: Vec::new(),
    }
}

// ---------- RW003 ----------

#[test]
fn rw003_fires_on_mutex_field_outside_owner() {
    let air = air_with_types(
        Some("crate::handler"),
        "src/handler.rs",
        vec![ty(
            "ServiceState",
            TypeKind::Struct,
            vec![("inner", "Mutex<HashMap<u64,User>>")],
            "src/handler.rs",
            4,
        )],
    );
    let section = RwSection {
        runtime_owner_paths: vec!["crate::runtime::*".into()],
        ..RwSection::default()
    };
    let diags = rw003(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    let d = &diags[0];
    assert_eq!(d.rule_id, "RW003");
    assert_eq!(d.severity, Severity::Warning);
    assert!(d.message.contains("crate::ServiceState"));
    assert!(d.message.contains("Mutex"));
    assert!(
        d.why.iter().any(|w| w.contains("Mutex<*")),
        "expected matched pattern in why; got {:?}",
        d.why
    );
}

#[test]
fn rw003_quiet_inside_runtime_owner_module() {
    let air = air_with_types(
        Some("crate::runtime::pool"),
        "src/runtime/pool.rs",
        vec![ty(
            "Pool",
            TypeKind::Struct,
            vec![("guard", "Mutex<()>")],
            "src/runtime/pool.rs",
            4,
        )],
    );
    let section = RwSection {
        runtime_owner_paths: vec!["crate::runtime::*".into()],
        ..RwSection::default()
    };
    assert!(rw003(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn rw003_quiet_when_no_field_matches_patterns() {
    let air = air_with_types(
        Some("crate::handler"),
        "src/handler.rs",
        vec![ty(
            "Plain",
            TypeKind::Struct,
            vec![("name", "String"), ("count", "u64")],
            "src/handler.rs",
            4,
        )],
    );
    let section = RwSection {
        runtime_owner_paths: vec!["crate::runtime::*".into()],
        ..RwSection::default()
    };
    assert!(rw003(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn rw003_silent_when_runtime_owner_paths_empty() {
    let air = air_with_types(
        Some("crate::handler"),
        "src/handler.rs",
        vec![ty(
            "ServiceState",
            TypeKind::Struct,
            vec![("inner", "Arc<RwLock<State>>")],
            "src/handler.rs",
            4,
        )],
    );
    let section = RwSection::default();
    assert!(rw003(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn rw003_matches_arc_mutex_via_pattern_seed() {
    let air = air_with_types(
        Some("crate::handler"),
        "src/handler.rs",
        vec![ty(
            "Service",
            TypeKind::Struct,
            vec![("state", "Arc<Mutex<Inner>>")],
            "src/handler.rs",
            4,
        )],
    );
    let section = RwSection {
        runtime_owner_paths: vec!["crate::runtime::*".into()],
        ..RwSection::default()
    };
    let diags = rw003(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
}

#[test]
fn rw003_agent_strict_elevates_to_fatal() {
    let air = air_with_types(
        Some("crate::handler"),
        "src/handler.rs",
        vec![ty(
            "ServiceState",
            TypeKind::Struct,
            vec![("inner", "RwLock<u64>")],
            "src/handler.rs",
            4,
        )],
    );
    let section = RwSection {
        runtime_owner_paths: vec!["crate::runtime::*".into()],
        ..RwSection::default()
    };
    let diags = rw003(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

// ---------- RW004 ----------

#[test]
fn rw004_fires_on_singleton_name_outside_owner() {
    let air = air_with_types(
        Some("crate::handler"),
        "src/handler.rs",
        vec![ty(
            "AppSingleton",
            TypeKind::Struct,
            vec![("config", "Config")],
            "src/handler.rs",
            4,
        )],
    );
    let section = RwSection {
        runtime_owner_paths: vec!["crate::runtime::*".into()],
        ..RwSection::default()
    };
    let diags = rw004(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    let d = &diags[0];
    assert_eq!(d.rule_id, "RW004");
    assert_eq!(d.severity, Severity::Warning);
    assert!(d.message.contains("AppSingleton"));
    assert!(
        d.why.iter().any(|w| w.contains("singleton_name_patterns")),
        "expected name-pattern reason in why; got {:?}",
        d.why
    );
}

#[test]
fn rw004_fires_on_single_field_oncecell_struct_outside_owner() {
    let air = air_with_types(
        Some("crate::handler"),
        "src/handler.rs",
        vec![ty(
            "Config",
            TypeKind::Struct,
            vec![("inner", "OnceCell<Inner>")],
            "src/handler.rs",
            4,
        )],
    );
    let section = RwSection {
        runtime_owner_paths: vec!["crate::runtime::*".into()],
        ..RwSection::default()
    };
    let diags = rw004(&air, &section, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id, "RW004");
    assert!(
        diags[0]
            .why
            .iter()
            .any(|w| w.contains("single-field struct")),
        "expected shape-based reason in why; got {:?}",
        diags[0].why
    );
}

#[test]
fn rw004_quiet_inside_runtime_owner_module() {
    let air = air_with_types(
        Some("crate::runtime::globals"),
        "src/runtime/globals.rs",
        vec![ty(
            "AppSingleton",
            TypeKind::Struct,
            vec![("config", "Config")],
            "src/runtime/globals.rs",
            4,
        )],
    );
    let section = RwSection {
        runtime_owner_paths: vec!["crate::runtime::*".into()],
        ..RwSection::default()
    };
    assert!(rw004(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn rw004_quiet_on_plain_struct() {
    let air = air_with_types(
        Some("crate::handler"),
        "src/handler.rs",
        vec![ty(
            "User",
            TypeKind::Struct,
            vec![("name", "String"), ("age", "u32")],
            "src/handler.rs",
            4,
        )],
    );
    let section = RwSection {
        runtime_owner_paths: vec!["crate::runtime::*".into()],
        ..RwSection::default()
    };
    assert!(rw004(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn rw004_silent_when_runtime_owner_paths_empty() {
    let air = air_with_types(
        Some("crate::handler"),
        "src/handler.rs",
        vec![ty(
            "AppSingleton",
            TypeKind::Struct,
            vec![("config", "Config")],
            "src/handler.rs",
            4,
        )],
    );
    let section = RwSection::default();
    assert!(rw004(&air, &section, CheckMode::Human).is_empty());
}

#[test]
fn rw004_agent_strict_elevates_to_fatal() {
    let air = air_with_types(
        Some("crate::handler"),
        "src/handler.rs",
        vec![ty(
            "Globals",
            TypeKind::Struct,
            vec![("conf", "Config")],
            "src/handler.rs",
            4,
        )],
    );
    // `*Globals` is in the default singleton_name_patterns seed.
    let section = RwSection {
        runtime_owner_paths: vec!["crate::runtime::*".into()],
        ..RwSection::default()
    };
    let diags = rw004(&air, &section, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Fatal);
}

// ---------- RW005 / RW006 helpers ----------

fn hot_path_marker_fact(symbol: &str) -> AirFact {
    AirFact {
        kind: FactKind::HotPath,
        target: FactTarget::Function {
            symbol: symbol.into(),
        },
        source: "markers".into(),
        confidence: 1.0,
        reasons: vec!["test marker".into()],
        evidence: None,
    }
}

fn blocking_call_fact(symbol: &str, callee: &str) -> AirFact {
    AirFact {
        kind: FactKind::BlockingCall,
        target: FactTarget::Function {
            symbol: symbol.into(),
        },
        source: "std-rt".into(),
        confidence: 0.9,
        reasons: vec![format!("`{callee}` is a blocking-shaped call")],
        evidence: Some(callee.into()),
    }
}

fn spawned_work_fact(symbol: &str, callee: &str) -> AirFact {
    AirFact {
        kind: FactKind::SpawnedWork,
        target: FactTarget::Function {
            symbol: symbol.into(),
        },
        source: "std-rt".into(),
        confidence: 0.9,
        reasons: vec![format!("`{callee}` is a spawn-shaped call")],
        evidence: Some(callee.into()),
    }
}

// ---------- RW005 ----------

#[test]
fn rw005_fires_when_hot_path_function_has_blocking_call() {
    let air = air_with_file(
        Some("crate::frame"),
        "src/frame.rs",
        vec![func("crate::frame::tick", "src/frame.rs", 17)],
        vec![
            hot_path_marker_fact("crate::frame::tick"),
            blocking_call_fact("crate::frame::tick", "std::fs::read"),
        ],
    );
    let diags = rw005(&air, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    let d = &diags[0];
    assert_eq!(d.rule_id, "RW005");
    assert_eq!(d.severity, Severity::Fatal);
    assert_eq!(d.span.line_start, 17);
    assert!(d.message.contains("crate::frame::tick"));
    assert!(d.message.contains("std::fs::read"));
    assert!(
        d.why.iter().any(|w| w.contains("HotPath")),
        "expected HotPath marker reason; got {:?}",
        d.why
    );
    assert!(
        d.why.iter().any(|w| w.contains("blocking-shaped")),
        "expected loader reason; got {:?}",
        d.why
    );
    assert!(
        d.why
            .iter()
            .any(|w| w.contains("starve") || w.contains("non-blocking")),
        "expected hot-path explanation in why; got {:?}",
        d.why
    );
    assert!(
        d.suggested_fix
            .as_deref()
            .map(|s| s.contains("tokio::fs") || s.contains("worker"))
            .unwrap_or(false),
        "expected actionable fix; got {:?}",
        d.suggested_fix
    );
}

#[test]
fn rw005_quiet_when_hot_path_has_no_blocking_call() {
    let air = air_with_file(
        Some("crate::frame"),
        "src/frame.rs",
        vec![func("crate::frame::tick", "src/frame.rs", 17)],
        vec![hot_path_marker_fact("crate::frame::tick")],
    );
    assert!(rw005(&air, CheckMode::Human).is_empty());
}

#[test]
fn rw005_quiet_when_blocking_call_outside_hot_path() {
    let air = air_with_file(
        Some("crate::handler"),
        "src/handler.rs",
        vec![func("crate::handler::do_it", "src/handler.rs", 5)],
        vec![blocking_call_fact("crate::handler::do_it", "std::fs::read")],
    );
    assert!(rw005(&air, CheckMode::Human).is_empty());
}

#[test]
fn rw005_emits_one_diagnostic_per_blocking_fact() {
    let air = air_with_file(
        Some("crate::frame"),
        "src/frame.rs",
        vec![func("crate::frame::tick", "src/frame.rs", 17)],
        vec![
            hot_path_marker_fact("crate::frame::tick"),
            blocking_call_fact("crate::frame::tick", "std::fs::read"),
            blocking_call_fact("crate::frame::tick", "std::thread::sleep"),
            blocking_call_fact("crate::frame::tick", "Command::output"),
        ],
    );
    let diags = rw005(&air, CheckMode::Human);
    assert_eq!(diags.len(), 3);
    for d in &diags {
        assert_eq!(d.rule_id, "RW005");
    }
}

#[test]
fn rw005_silent_when_no_hot_path_facts() {
    let air = air_with_file(
        Some("crate::handler"),
        "src/handler.rs",
        vec![func("crate::handler::create_user", "src/handler.rs", 17)],
        vec![blocking_call_fact(
            "crate::handler::create_user",
            "std::fs::read",
        )],
    );
    assert!(
        rw005(&air, CheckMode::Human).is_empty(),
        "no HotPath markers anywhere in the workspace → silent"
    );
}

#[test]
fn rw005_agent_strict_keeps_fatal() {
    let air = air_with_file(
        Some("crate::frame"),
        "src/frame.rs",
        vec![func("crate::frame::tick", "src/frame.rs", 17)],
        vec![
            hot_path_marker_fact("crate::frame::tick"),
            blocking_call_fact("crate::frame::tick", "std::fs::read"),
        ],
    );
    let diags = rw005(&air, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(
        diags[0].severity,
        Severity::Fatal,
        "RW005 is already Fatal in Human mode; agent-strict must not lower it"
    );
}

// ---------- RW006 ----------

#[test]
fn rw006_fires_when_hot_path_function_spawns() {
    let air = air_with_file(
        Some("crate::frame"),
        "src/frame.rs",
        vec![func("crate::frame::tick", "src/frame.rs", 21)],
        vec![
            hot_path_marker_fact("crate::frame::tick"),
            spawned_work_fact("crate::frame::tick", "tokio::spawn"),
        ],
    );
    let diags = rw006(&air, CheckMode::Human);
    assert_eq!(diags.len(), 1);
    let d = &diags[0];
    assert_eq!(d.rule_id, "RW006");
    assert_eq!(d.severity, Severity::Fatal);
    assert_eq!(d.span.line_start, 21);
    assert!(d.message.contains("crate::frame::tick"));
    assert!(d.message.contains("tokio::spawn"));
    assert!(d.message.contains("uncontrolled"));
    assert!(
        d.why.iter().any(|w| w.contains("HotPath")),
        "expected HotPath marker reason; got {:?}",
        d.why
    );
    assert!(
        d.why.iter().any(|w| w.contains("spawn-shaped")),
        "expected loader reason; got {:?}",
        d.why
    );
    assert!(
        d.why.iter().any(|w| w.contains("unbounded task pressure")),
        "expected hot-loop spawn explanation; got {:?}",
        d.why
    );
}

#[test]
fn rw006_quiet_when_hot_path_has_no_spawn() {
    let air = air_with_file(
        Some("crate::frame"),
        "src/frame.rs",
        vec![func("crate::frame::tick", "src/frame.rs", 21)],
        vec![hot_path_marker_fact("crate::frame::tick")],
    );
    assert!(rw006(&air, CheckMode::Human).is_empty());
}

#[test]
fn rw006_quiet_when_spawn_outside_hot_path() {
    let air = air_with_file(
        Some("crate::handler"),
        "src/handler.rs",
        vec![func("crate::handler::create", "src/handler.rs", 5)],
        vec![spawned_work_fact("crate::handler::create", "tokio::spawn")],
    );
    assert!(rw006(&air, CheckMode::Human).is_empty());
}

#[test]
fn rw006_emits_one_diagnostic_per_spawn_fact() {
    let air = air_with_file(
        Some("crate::frame"),
        "src/frame.rs",
        vec![func("crate::frame::tick", "src/frame.rs", 21)],
        vec![
            hot_path_marker_fact("crate::frame::tick"),
            spawned_work_fact("crate::frame::tick", "tokio::spawn"),
            spawned_work_fact("crate::frame::tick", "std::thread::spawn"),
        ],
    );
    let diags = rw006(&air, CheckMode::Human);
    assert_eq!(diags.len(), 2);
    for d in &diags {
        assert_eq!(d.rule_id, "RW006");
    }
}

#[test]
fn rw006_silent_when_no_hot_path_facts() {
    let air = air_with_file(
        Some("crate::handler"),
        "src/handler.rs",
        vec![func("crate::handler::create", "src/handler.rs", 5)],
        vec![spawned_work_fact("crate::handler::create", "tokio::spawn")],
    );
    assert!(
        rw006(&air, CheckMode::Human).is_empty(),
        "no HotPath markers anywhere in the workspace → silent"
    );
}

#[test]
fn rw006_agent_strict_keeps_fatal() {
    let air = air_with_file(
        Some("crate::frame"),
        "src/frame.rs",
        vec![func("crate::frame::tick", "src/frame.rs", 21)],
        vec![
            hot_path_marker_fact("crate::frame::tick"),
            spawned_work_fact("crate::frame::tick", "tokio::spawn"),
        ],
    );
    let diags = rw006(&air, CheckMode::AgentStrict);
    assert_eq!(diags.len(), 1);
    assert_eq!(
        diags[0].severity,
        Severity::Fatal,
        "RW006 is already Fatal in Human mode; agent-strict must not lower it"
    );
}
