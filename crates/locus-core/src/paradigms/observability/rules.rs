//! OB rules.
//!
//! Implemented:
//! - [`ob001`]: raw `println!` / `dbg!` (and equivalents) outside test,
//!   example, or other observer-declared modules. The "agent stitched in
//!   ad-hoc logs while patching" anti-pattern: raw print/debug macros bypass
//!   any structured logging facility, leak to stdout/stderr in production,
//!   and signal that observability isn't owned. Structured facilities like
//!   `tracing::info!` / `log::warn!` are intentionally NOT flagged — only the
//!   facility-bypassing macros listed in `forbidden_log_targets`.

use locus_air::{ActionKind, AirItem, AirWorkspace};

use super::lockfile_schema::{ObSection, matches_pattern};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

/// OB001 — raw print/dbg in non-test, non-observer code.
///
/// For every `AirFile` whose `module_path` does *not* match any pattern in
/// `observer_paths`, walk its `AirItem::TruthAction` items. Fire when the
/// action's `kind` is `Log` and its `target` matches any pattern in the
/// effective `forbidden_log_targets` list.
///
/// Severity: Warning by default; Fatal under `--agent-strict`. The spec is
/// explicit that observability-ownership violations are heuristic — a stray
/// `println!` in scratch code shouldn't take CI down by default, but
/// agent-introduced raw prints in domain code should be caught aggressively.
///
/// Silent until the user populates `observer_paths`. Even though the
/// default `forbidden_log_targets` is non-empty (the print/dbg family),
/// firing on raw prints in *any* file before the user has classified
/// which files are observers / CLIs / tests would explode noise in
/// every workspace that hadn't yet onboarded — same UX choice as every
/// other lockfile-driven rule (DG/MO/UT/CR/CX/...). Once `observer_paths`
/// is populated, the rule starts firing on non-observer files.
pub fn ob001(air: &AirWorkspace, section: &ObSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.observer_paths.is_empty() {
        return Vec::new();
    }
    let forbidden = section.effective_forbidden_log_targets();
    if forbidden.is_empty() {
        // User cleared the forbidden list — nothing to flag.
        return Vec::new();
    }

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                // Files without a resolved module path can't be matched
                // against `observer_paths`; skip rather than guess.
                continue;
            };
            // File is an observer / test / example / CLI — every log call
            // here is, by user assertion, legitimate.
            if section
                .observer_paths
                .iter()
                .any(|pat| matches_pattern(pat, module_path))
            {
                continue;
            }
            for item in &file.items {
                let AirItem::TruthAction(a) = item else {
                    continue;
                };
                if a.action != ActionKind::Log {
                    continue;
                }
                let Some(forbidden_pattern) =
                    forbidden.iter().find(|pat| matches_pattern(pat, &a.target))
                else {
                    continue;
                };
                let function_label = a.function.as_deref().unwrap_or("<unknown>");
                out.push(Diagnostic {
                    rule_id: "OB001".to_string(),
                    severity: mode.elevate(Severity::Warning),
                    span: a.span.clone(),
                    concept: None,
                    message: format!(
                        "raw log call `{}!` in `{module_path}` (fn `{function_label}`) — \
                         bypasses structured logging",
                        a.target
                    ),
                    why: vec![
                        format!(
                            "module `{module_path}` does not match any \
                             `paradigms.OB.observer_paths` pattern"
                        ),
                        format!(
                            "log target `{}` matches forbidden pattern `{forbidden_pattern}`",
                            a.target
                        ),
                        format!("enclosing function: `{function_label}`"),
                    ],
                    suggested_fix: Some(format!(
                        "route this through the project's structured logging facility \
                         (e.g. `tracing::info!` / `log::warn!` with the accepted spans \
                         and fields), or, if `{module_path}` legitimately owns user-facing \
                         or test output, accept it via `paradigms.OB.observer_paths` in \
                         `locus.lock`"
                    )),
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::lockfile_schema::default_forbidden_log_targets;
    use super::*;
    use locus_air::{
        AIR_SCHEMA_VERSION, AirFile, AirPackage, AirSpan, AirTruthAction, AirWorkspace,
    };

    fn log_action(target: &str, function: &str, file: &str, line: u32) -> AirItem {
        AirItem::TruthAction(AirTruthAction {
            action: ActionKind::Log,
            target: target.into(),
            function: Some(function.into()),
            span: AirSpan::new(file, line, line),
            confidence: 0.9,
            reasons: Vec::new(),
        })
    }

    fn air_with_module(module: Option<&str>, items: Vec<AirItem>) -> AirWorkspace {
        AirWorkspace {
            schema_version: AIR_SCHEMA_VERSION,
            packages: vec![AirPackage {
                name: "x".into(),
                version: "0".into(),
                root_dir: "/".into(),
                files: vec![AirFile {
                    path: "t.rs".into(),
                    module_path: module.map(|m| m.into()),
                    items,
                    hints: Vec::new(),
                    parse_error: None,
                    line_count: 1,
                }],
            }],
        }
    }

    /// Onboarded baseline: a single observer pattern that doesn't match any
    /// of the test fixture's `x::domain::*` modules, plus the default
    /// forbidden targets. With OB silent until `observer_paths` is
    /// populated (mirrors every other lockfile-driven rule), tests need at
    /// least one observer pattern declared.
    fn default_section() -> ObSection {
        ObSection {
            observer_paths: vec!["x::cli::*".into()],
            forbidden_log_targets: default_forbidden_log_targets(),
        }
    }

    #[test]
    fn ob001_fires_on_raw_println_in_non_observer_file() {
        let air = air_with_module(
            Some("x::domain::user"),
            vec![log_action("println", "x::domain::user::greet", "t.rs", 5)],
        );
        let diags = ob001(&air, &default_section(), CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "OB001");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(
            diags[0].message.contains("println"),
            "expected target in message; got {}",
            diags[0].message
        );
        assert!(
            diags[0].message.contains("x::domain::user"),
            "expected module_path in message; got {}",
            diags[0].message
        );
        assert!(
            diags[0].message.contains("greet"),
            "expected function in message; got {}",
            diags[0].message
        );
        assert!(
            diags[0].why.iter().any(|w| w.contains("observer_paths")),
            "expected observer_paths reasoning in why; got {:?}",
            diags[0].why
        );
        assert!(
            diags[0].why.iter().any(|w| w.contains("println")),
            "expected target reasoning in why; got {:?}",
            diags[0].why
        );
    }

    #[test]
    fn ob001_quiet_on_tracing_info_with_default_forbidden_targets() {
        // `tracing::info!` is a Log action but its target isn't in the
        // default forbidden list — structured logging is the canonical
        // facility, not the violation.
        let air = air_with_module(
            Some("x::domain::user"),
            vec![log_action(
                "tracing::info",
                "x::domain::user::greet",
                "t.rs",
                5,
            )],
        );
        assert!(ob001(&air, &default_section(), CheckMode::Human).is_empty());
    }

    #[test]
    fn ob001_quiet_on_println_in_observer_path_matching_file() {
        // CLI / test / example modules are allowed to use println: that's
        // exactly what observer_paths is for.
        let air = air_with_module(
            Some("x::cli::main"),
            vec![log_action("println", "x::cli::main::run", "t.rs", 5)],
        );
        let section = ObSection {
            observer_paths: vec!["x::cli::*".into()],
            forbidden_log_targets: default_forbidden_log_targets(),
        };
        assert!(ob001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ob001_custom_forbidden_targets_override_defaults() {
        // User adds tracing::info to forbidden (e.g. enforcing "only the
        // dedicated logger module may call tracing::info!"). The default
        // print family is gone in this section — only the custom entry.
        let air = air_with_module(
            Some("x::domain::user"),
            vec![
                log_action("tracing::info", "x::domain::user::greet", "t.rs", 5),
                // println is NOT in this section's forbidden list, so it
                // mustn't fire under this configuration.
                log_action("println", "x::domain::user::greet", "t.rs", 6),
            ],
        );
        let section = ObSection {
            observer_paths: vec!["x::cli::*".into()], // non-matching pattern → rule active
            forbidden_log_targets: vec!["tracing::info".into()],
        };
        let diags = ob001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("tracing::info"));
    }

    #[test]
    fn ob001_skips_files_without_module_path() {
        let air = air_with_module(None, vec![log_action("println", "anon::fn", "t.rs", 5)]);
        assert!(ob001(&air, &default_section(), CheckMode::Human).is_empty());
    }

    #[test]
    fn ob001_agent_strict_elevates_warning_to_fatal() {
        let air = air_with_module(
            Some("x::domain::user"),
            vec![log_action("println", "x::domain::user::greet", "t.rs", 5)],
        );
        let diags = ob001(&air, &default_section(), CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    #[test]
    fn ob001_multiple_raw_prints_produce_one_diagnostic_per_call_site() {
        let air = air_with_module(
            Some("x::domain::user"),
            vec![
                log_action("println", "x::domain::user::greet", "t.rs", 5),
                log_action("dbg", "x::domain::user::greet", "t.rs", 7),
                log_action("eprintln", "x::domain::user::oops", "t.rs", 12),
                // tracing::info is NOT forbidden under defaults; must not contribute.
                log_action("tracing::info", "x::domain::user::ok", "t.rs", 14),
            ],
        );
        let diags = ob001(&air, &default_section(), CheckMode::Human);
        assert_eq!(diags.len(), 3);
        // Each diagnostic should pin to its own span line.
        let lines: Vec<u32> = diags.iter().map(|d| d.span.line_start).collect();
        assert!(lines.contains(&5));
        assert!(lines.contains(&7));
        assert!(lines.contains(&12));
        assert!(!lines.contains(&14));
    }

    #[test]
    fn ob001_silent_when_observer_paths_empty() {
        // OB stays silent on un-onboarded codebases (empty observer_paths),
        // even with the default print/dbg forbidden targets present —
        // mirrors the lockfile-driven UX of DG/MO/UT/CR/CX/etc.
        let air = air_with_module(
            Some("x::domain::user"),
            vec![log_action("println", "x::domain::user::greet", "t.rs", 5)],
        );
        let section = ObSection {
            observer_paths: Vec::new(),
            forbidden_log_targets: default_forbidden_log_targets(),
        };
        assert!(ob001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn ob001_silent_when_forbidden_list_empty() {
        // Observer paths declared but the forbidden list is cleared — no
        // targets to check, nothing fires.
        let air = air_with_module(
            Some("x::domain::user"),
            vec![log_action("println", "x::domain::user::greet", "t.rs", 5)],
        );
        let section = ObSection {
            observer_paths: vec!["x::cli::*".into()],
            forbidden_log_targets: Vec::new(),
        };
        assert!(ob001(&air, &section, CheckMode::Human).is_empty());
    }
}
