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
#[path = "rules_tests.rs"]
mod rules_tests;
