//! CR rule implementations.
//!
//! Implemented:
//! - [`cr001`]: service-shaped construction outside any declared composition
//!   root.
//! - [`cr002`]: high-density wiring inside a composition root — a single
//!   function emits more `Construct` actions than `wiring_density_threshold`.
//!
//! All CR rules are lockfile-driven: they stay silent until the user has
//! populated `composition_root_paths` (otherwise we have no idea which
//! modules are legitimately wiring concrete services).

use std::collections::BTreeMap;

use locus_air::{ActionKind, AirItem, AirSpan, AirWorkspace};

use super::lockfile_schema::{CrSection, effective_service_suffixes};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

/// CR001 — service-shaped construction outside composition root.
///
/// For every `AirItem::TruthAction` with `action == Construct`, fires when:
/// - the file's `module_path` does NOT match any `composition_root_paths`
///   pattern, and
/// - the construction target's last `::` segment ends with one of the
///   accepted service suffixes (heuristic: `Service`, `Client`, `Repository`,
///   `Adapter`, `Connection`, `Pool`, `Manager` by default).
///
/// Always Fatal: composition-root violations are a layered-architecture
/// issue — concrete services must not be wired in handlers, services, or
/// feature modules.
///
/// Silent when `composition_root_paths` is empty: we wait for the user to
/// declare where their roots live before flagging anything.
pub fn cr001(air: &AirWorkspace, section: &CrSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.composition_root_paths.is_empty() {
        return Vec::new();
    }
    let suffixes = effective_service_suffixes(section);
    if suffixes.is_empty() {
        // Defensive: an explicitly user-cleared override could in principle
        // produce this, but `effective_service_suffixes` falls back to the
        // canonical seven on empty input. Either way: nothing to match.
        return Vec::new();
    }

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let module_path = file.module_path.as_deref().unwrap_or("");
            if section
                .composition_root_paths
                .iter()
                .any(|pat| matches_pattern(pat, module_path))
            {
                continue; // file is itself a composition root
            }
            for item in &file.items {
                let AirItem::TruthAction(a) = item else {
                    continue;
                };
                if a.action != ActionKind::Construct {
                    continue;
                }
                let short = a
                    .target
                    .rsplit("::")
                    .next()
                    .unwrap_or(a.target.as_str())
                    .trim();
                let Some(matched_suffix) = suffixes.iter().find(|s| short.ends_with(s.as_str()))
                else {
                    continue;
                };
                let function_label = a
                    .function
                    .as_deref()
                    .unwrap_or("(no enclosing function recorded)");
                let module_label = if module_path.is_empty() {
                    "(unknown module)"
                } else {
                    module_path
                };
                out.push(Diagnostic {
                    rule_id: "CR001".to_string(),
                    severity: mode.elevate(Severity::Fatal),
                    span: a.span.clone(),
                    concept: None,
                    message: format!(
                        "service-shaped construction of `{}` in module `{module_label}` \
                         (matched suffix `{matched_suffix}`) outside any declared \
                         composition root",
                        a.target
                    ),
                    why: vec![
                        format!(
                            "module `{module_label}` matches none of the \
                             `composition_root_paths` patterns"
                        ),
                        format!("target `{}` ends with `{matched_suffix}`", a.target),
                        format!("enclosing function: `{function_label}`"),
                    ],
                    suggested_fix: Some(format!(
                        "move the construction of `{}` into a composition root \
                         (e.g. `main`, a `wire` module, or a declared composition \
                         module), or accept this file by adding its module to \
                         `paradigms.CR.composition_root_paths`",
                        a.target
                    )),
                });
            }
        }
    }
    out
}

/// CR002 — high-density wiring inside a composition root.
///
/// Counts `AirItem::TruthAction` entries with `action == Construct` per
/// enclosing function (`AirTruthAction.function`), but only inside files
/// whose `module_path` matches a `composition_root_paths` pattern. Fires on
/// every function whose count is `>= wiring_density_threshold`.
///
/// Why warning, not fatal: even a legitimate composition root that wires a
/// dozen services in one function still works. But a single function
/// constructing 20+ services is a code-smell signal that the root needs to
/// be split into sub-roots — recommend the user refactor, don't block
/// builds. Elevated to Fatal under `--agent-strict`.
///
/// Silent when `composition_root_paths` is empty (we have no idea which
/// functions are roots in the first place).
pub fn cr002(air: &AirWorkspace, section: &CrSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.composition_root_paths.is_empty() {
        return Vec::new();
    }
    if section.wiring_density_threshold == 0 {
        // Defensive: a 0 threshold would fire on every wiring root and is
        // almost certainly a config error. Stay silent rather than spam.
        return Vec::new();
    }

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let module_path = file.module_path.as_deref().unwrap_or("");
            if !section
                .composition_root_paths
                .iter()
                .any(|pat| matches_pattern(pat, module_path))
            {
                continue;
            }

            // Group Construct actions by enclosing function. Use a
            // `BTreeMap` keyed by (function-name, first-span-file) so output
            // ordering is deterministic.
            let mut counts: BTreeMap<String, (u32, AirSpan)> = BTreeMap::new();
            for item in &file.items {
                let AirItem::TruthAction(a) = item else {
                    continue;
                };
                if a.action != ActionKind::Construct {
                    continue;
                }
                let func = a
                    .function
                    .clone()
                    .unwrap_or_else(|| "(no enclosing function recorded)".to_string());
                let entry = counts.entry(func).or_insert((0, a.span.clone()));
                entry.0 += 1;
            }

            for (func, (count, span)) in counts {
                if count < section.wiring_density_threshold {
                    continue;
                }
                out.push(Diagnostic {
                    rule_id: "CR002".to_string(),
                    severity: mode.elevate(Severity::Warning),
                    span,
                    concept: None,
                    message: format!(
                        "function `{func}` in composition root `{module_path}` \
                         constructs {count} services in a single function \
                         (threshold {})",
                        section.wiring_density_threshold
                    ),
                    why: vec![
                        format!(
                            "module `{module_path}` matches a \
                             `composition_root_paths` pattern"
                        ),
                        format!(
                            "{count} `Construct` actions are recorded with \
                             enclosing function `{func}`"
                        ),
                        format!(
                            "threshold is `wiring_density_threshold = {}`",
                            section.wiring_density_threshold
                        ),
                    ],
                    suggested_fix: Some(format!(
                        "split `{func}` into sub-functions or sub-modules \
                         (e.g. `wire_persistence`, `wire_http`); the \
                         composition root remains the single owner of \
                         construction, but the wiring stops being a wall of \
                         text. If this density is intentional, raise \
                         `paradigms.CR.wiring_density_threshold` in `locus.lock`"
                    )),
                });
            }
        }
    }
    out
}

/// Pattern matching duplicated locally to mirror DG/UT (suffix wildcards).
/// Kept private to this module so any future tweak to CR's matcher doesn't
/// silently affect DG/UT.
fn matches_pattern(pattern: &str, path: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix("::*") {
        return path == prefix || path.starts_with(&format!("{prefix}::"));
    }
    pattern == path
}

#[cfg(test)]
mod tests {
    use super::*;
    use locus_air::{
        AIR_SCHEMA_VERSION, AirFile, AirPackage, AirSpan, AirTruthAction, AirWorkspace,
    };

    fn construct(target: &str, function: &str, file_path: &str, line: u32) -> AirItem {
        AirItem::TruthAction(AirTruthAction {
            action: ActionKind::Construct,
            target: target.into(),
            function: Some(function.into()),
            span: AirSpan::new(file_path, line, line),
            confidence: 0.95,
            reasons: vec!["struct literal".into()],
        })
    }

    fn air_with_file(module_path: &str, file_path: &str, items: Vec<AirItem>) -> AirWorkspace {
        AirWorkspace {
            schema_version: AIR_SCHEMA_VERSION,
            packages: vec![AirPackage {
                name: "x".into(),
                version: "0".into(),
                root_dir: "/".into(),
                files: vec![AirFile {
                    path: file_path.into(),
                    module_path: Some(module_path.into()),
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
    fn cr001_fires_on_service_shaped_construct_outside_root() {
        let air = air_with_file(
            "crate::handler",
            "src/handler.rs",
            vec![construct(
                "UserRepository",
                "crate::handler::create_user",
                "src/handler.rs",
                12,
            )],
        );
        let section = CrSection {
            composition_root_paths: vec!["crate::wire".into(), "bin::*".into()],
            service_suffixes: Vec::new(),
            ..Default::default()
        };
        let diags = cr001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "CR001");
        assert_eq!(diags[0].severity, Severity::Fatal);
        assert!(diags[0].message.contains("UserRepository"));
        assert!(diags[0].message.contains("crate::handler"));
        assert!(diags[0].message.contains("Repository"));
    }

    #[test]
    fn cr001_quiet_inside_composition_root() {
        let air = air_with_file(
            "crate::wire",
            "src/wire.rs",
            vec![construct(
                "UserRepository",
                "crate::wire::build_app",
                "src/wire.rs",
                3,
            )],
        );
        let section = CrSection {
            composition_root_paths: vec!["crate::wire".into()],
            service_suffixes: Vec::new(),
            ..Default::default()
        };
        assert!(cr001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn cr001_quiet_on_non_service_shaped_target() {
        let air = air_with_file(
            "crate::handler",
            "src/handler.rs",
            vec![construct(
                "User",
                "crate::handler::create_user",
                "src/handler.rs",
                7,
            )],
        );
        let section = CrSection {
            composition_root_paths: vec!["crate::wire".into()],
            service_suffixes: Vec::new(),
            ..Default::default()
        };
        assert!(cr001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn cr001_silent_when_composition_root_paths_empty() {
        let air = air_with_file(
            "crate::handler",
            "src/handler.rs",
            vec![construct(
                "UserRepository",
                "crate::handler::create_user",
                "src/handler.rs",
                4,
            )],
        );
        let section = CrSection::default();
        assert!(
            cr001(&air, &section, CheckMode::Human).is_empty(),
            "rule should wait for explicit composition_root_paths declaration"
        );
    }

    #[test]
    fn cr001_agent_strict_keeps_fatal() {
        let air = air_with_file(
            "crate::handler",
            "src/handler.rs",
            vec![construct(
                "PaymentClient",
                "crate::handler::charge",
                "src/handler.rs",
                9,
            )],
        );
        let section = CrSection {
            composition_root_paths: vec!["crate::wire".into()],
            service_suffixes: Vec::new(),
            ..Default::default()
        };
        let diags = cr001(&air, &section, CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    #[test]
    fn cr001_custom_service_suffixes_override_defaults() {
        // Default suffixes would NOT catch `Gateway`; a `Repository` would.
        // With a user override that drops `Repository` and adds `Gateway`,
        // the behaviour flips.
        let air = air_with_file(
            "crate::handler",
            "src/handler.rs",
            vec![
                construct(
                    "PaymentGateway",
                    "crate::handler::charge",
                    "src/handler.rs",
                    11,
                ),
                construct(
                    "UserRepository",
                    "crate::handler::create_user",
                    "src/handler.rs",
                    22,
                ),
            ],
        );
        let section = CrSection {
            composition_root_paths: vec!["crate::wire".into()],
            service_suffixes: vec!["Gateway".into()],
            ..Default::default()
        };
        let diags = cr001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1, "only `Gateway` should match; got {diags:?}");
        assert!(diags[0].message.contains("PaymentGateway"));
        assert!(!diags[0].message.contains("UserRepository"));
    }

    #[test]
    fn cr001_matches_path_qualified_target() {
        // Constructions like `crate::infra::DbConnection { ... }` appear in
        // AIR with the full path; the suffix check uses the last `::` segment.
        let air = air_with_file(
            "crate::handler",
            "src/handler.rs",
            vec![construct(
                "crate::infra::DbConnection",
                "crate::handler::open",
                "src/handler.rs",
                5,
            )],
        );
        let section = CrSection {
            composition_root_paths: vec!["crate::wire".into()],
            service_suffixes: Vec::new(),
            ..Default::default()
        };
        let diags = cr001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("crate::infra::DbConnection"));
        assert!(diags[0].message.contains("Connection"));
    }

    // ----- CR002 -----

    fn many_constructs(targets: &[&str], function: &str, file_path: &str) -> Vec<AirItem> {
        targets
            .iter()
            .enumerate()
            .map(|(i, t)| construct(t, function, file_path, (i as u32) + 1))
            .collect()
    }

    #[test]
    fn cr002_fires_when_wiring_density_meets_threshold() {
        let targets: Vec<&str> = (0..12).map(|_| "ServiceX").collect();
        let items = many_constructs(&targets, "crate::wire::build_app", "src/wire.rs");
        let air = air_with_file("crate::wire", "src/wire.rs", items);
        let section = CrSection {
            composition_root_paths: vec!["crate::wire".into()],
            wiring_density_threshold: 12,
            ..Default::default()
        };
        let diags = cr002(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "CR002");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(diags[0].message.contains("12"));
        assert!(diags[0].message.contains("crate::wire::build_app"));
    }

    #[test]
    fn cr002_quiet_below_threshold() {
        let targets: Vec<&str> = (0..11).map(|_| "ServiceX").collect();
        let items = many_constructs(&targets, "crate::wire::build_app", "src/wire.rs");
        let air = air_with_file("crate::wire", "src/wire.rs", items);
        let section = CrSection {
            composition_root_paths: vec!["crate::wire".into()],
            wiring_density_threshold: 12,
            ..Default::default()
        };
        assert!(cr002(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn cr002_silent_when_composition_root_paths_empty() {
        let targets: Vec<&str> = (0..30).map(|_| "ServiceX").collect();
        let items = many_constructs(&targets, "crate::handler::run", "src/handler.rs");
        let air = air_with_file("crate::handler", "src/handler.rs", items);
        let section = CrSection::default();
        assert!(cr002(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn cr002_quiet_for_function_outside_root_modules() {
        // Even with a populated `composition_root_paths`, a non-root file
        // doesn't trigger CR002 (CR001's job to flag wiring there).
        let targets: Vec<&str> = (0..30).map(|_| "ServiceX").collect();
        let items = many_constructs(&targets, "crate::handler::run", "src/handler.rs");
        let air = air_with_file("crate::handler", "src/handler.rs", items);
        let section = CrSection {
            composition_root_paths: vec!["crate::wire".into()],
            wiring_density_threshold: 12,
            ..Default::default()
        };
        assert!(cr002(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn cr002_groups_counts_per_enclosing_function() {
        // Two functions in the same root file, each below threshold,
        // shouldn't accumulate together.
        let mut items = many_constructs(
            &["A", "B", "C", "D", "E", "F"],
            "crate::wire::build_a",
            "src/wire.rs",
        );
        items.extend(many_constructs(
            &["A", "B", "C", "D", "E", "F", "G"],
            "crate::wire::build_b",
            "src/wire.rs",
        ));
        let air = air_with_file("crate::wire", "src/wire.rs", items);
        let section = CrSection {
            composition_root_paths: vec!["crate::wire".into()],
            wiring_density_threshold: 12,
            ..Default::default()
        };
        assert!(cr002(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn cr002_agent_strict_elevates_warning_to_fatal() {
        let targets: Vec<&str> = (0..15).map(|_| "ServiceX").collect();
        let items = many_constructs(&targets, "crate::wire::build_app", "src/wire.rs");
        let air = air_with_file("crate::wire", "src/wire.rs", items);
        let section = CrSection {
            composition_root_paths: vec!["crate::wire".into()],
            wiring_density_threshold: 12,
            ..Default::default()
        };
        let diags = cr002(&air, &section, CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    #[test]
    fn cr002_threshold_zero_stays_silent() {
        // Defensive: a 0 threshold is almost certainly a config bug; rule
        // refuses to spam the user.
        let items = many_constructs(&["A"], "crate::wire::build", "src/wire.rs");
        let air = air_with_file("crate::wire", "src/wire.rs", items);
        let section = CrSection {
            composition_root_paths: vec!["crate::wire".into()],
            wiring_density_threshold: 0,
            ..Default::default()
        };
        assert!(cr002(&air, &section, CheckMode::Human).is_empty());
    }
}
