use std::io::{self, BufWriter, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use locus_core::governance::Decision;
use locus_core::{CheckMode, Diagnostic, Lockfile, apply_exceptions, governance, today_utc};
use locus_report::{DecisionMetadata, DecisionRecord};

use crate::diff;

// locus: ot boundary cli.check cli
#[derive(clap::ValueEnum, Clone, Debug, Default, PartialEq, Eq)]
pub enum OutputFormat {
    /// Human-readable per-diagnostic blocks plus a severity summary.
    #[default]
    Text,
    /// Stable JSON shape (`locus_report::json`). Schema-versioned.
    Json,
    /// SARIF v2.1.0 (`locus_report::sarif`). Suitable for GitHub
    /// code-scanning and other static-analysis ingest pipelines.
    Sarif,
}

// locus: ot boundary cli.check cli
#[derive(clap::Args, Debug)]
pub struct CheckArgs {
    /// Workspace root (containing Cargo.toml).
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
    /// Treat warnings as fatal. Use this for LLM-generated patches.
    #[arg(long)]
    pub agent_strict: bool,
    /// Output format. `text` for humans, `json` for tooling, `sarif`
    /// for CI ingest (e.g. GitHub code-scanning). SARIF and JSON
    /// represent the final policy decisions, not raw rule findings —
    /// see issue #29.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
    /// Deprecated alias for `--format json`. Hidden from `--help` and
    /// scheduled for removal in a later release; kept now so pre-#29
    /// CI scripts and editor integrations keep working. When set,
    /// overrides `--format`.
    #[arg(long, hide = true)]
    pub json: bool,
    /// Filter diagnostics to files modified since the baseline ref.
    /// Combines tracked changes between baseline and HEAD, working-tree
    /// changes, and untracked-but-not-ignored files. Useful in CI to
    /// fail only on PR-introduced violations, not legacy noise.
    #[arg(long)]
    pub changed: bool,
    /// Baseline ref for `--changed`. Defaults to the first ref that
    /// resolves from `origin/main`, `origin/master`, `main`, `master`,
    /// `HEAD~1`. Pass an explicit ref (e.g. `--baseline origin/develop`)
    /// to override. Also used by Policy Guard (`PG001`-`PG004`) to read
    /// the baseline `.locus/lock.json`.
    #[arg(long)]
    pub baseline: Option<String>,
    /// Acknowledge that this run is calibrating policy (raising budgets,
    /// adding overrides, expanding `acknowledged_empty`, or widening
    /// `OT.converter_paths`). Without this flag, Policy Guard fails
    /// `--agent-strict` on any policy widening vs the baseline lockfile.
    /// With it, PG001/PG002/PG003/PG004/PG008 fire as Advisory and a
    /// structured calibration report is printed alongside the normal
    /// output. PG006 (missing debt metadata) is **not** affected by
    /// calibration — calibration legitimizes the addition itself, but
    /// does not waive the requirement to record `reason` / `expires` /
    /// `owner`. See issue #44.
    #[arg(long)]
    pub allow_policy_calibration: bool,
    /// Acknowledge that no baseline lockfile is available for the
    /// Policy Guard audit (e.g. shallow CI clone, first commit before
    /// `.locus/lock.json` existed). Without this flag, PG000 fires Fatal
    /// under `--agent-strict` so that a missing audit can't silently
    /// disable the gate. See issue #44.
    #[arg(long)]
    pub allow_missing_policy_baseline: bool,
    /// Merge `RustdocJsonBackend`'s resolved `impl From` / `impl
    /// TryFrom` facts into the AIR before paradigm rules run. Requires
    /// nightly `cargo +nightly`; falls back to syntactic-only with a
    /// stderr advisory if unavailable. Off by default. See #111.
    #[arg(long)]
    pub semantic_rust: bool,
}

pub fn run(args: CheckArgs) -> Result<()> {
    let mut air = locus_rust::scan(&args.workspace)
        .with_context(|| format!("scan failed: {}", args.workspace.display()))?;
    if args.semantic_rust {
        // Backend failures fall back to syntactic facts with a stderr
        // advisory — never errors. See `crate::semantic_facts`.
        crate::semantic_facts::merge_semantic_conversions(&mut air, &args.workspace);
    }
    let lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mode = if args.agent_strict {
        CheckMode::AgentStrict
    } else {
        CheckMode::Human
    };

    let mut records = governance_records(&air, &lockfile, mode, &args.workspace);

    // Apply exceptions BEFORE Policy Guard — PG must not be suppressible by
    // the same lockfile it audits. See #44.
    let today = today_utc();
    records = apply_exceptions_to_records(records, &air, &lockfile, &today);

    // --changed filter is applied before PG so PG diagnostics bypass it
    // (PG is global; it must not be hidden by a PR-scoped diff filter).
    records = apply_changed_filter_records(records, &args)?;

    // Policy Guard appended last: after apply_exceptions and --changed.
    append_policy_guard_records(&mut records, &lockfile, &args, mode)?;

    let format = resolve_format(&args);
    emit_output(&records, &format)?;

    let any_fatal = records.iter().any(|r| r.diagnostic.severity.is_fatal());
    if any_fatal {
        std::process::exit(1);
    }
    Ok(())
}

/// Run the governance pipeline and pair each emitted diagnostic with
/// the decision metadata that produced it. `run_with_workspace_root`
/// loads `.locus/arch.json` from the workspace and threads the outcome
/// into `RegistryCoherencePolicy` (LOCUS004). Output is byte-identical
/// to the legacy paradigm loop under `DefaultPassThroughPolicy`; the
/// extra metadata only surfaces in `--format json|sarif`.
fn governance_records(
    air: &locus_air::AirWorkspace,
    lockfile: &Lockfile,
    mode: CheckMode,
    workspace_root: &std::path::Path,
) -> Vec<DecisionRecord> {
    let out = governance::run_with_workspace_root(air, lockfile, mode, workspace_root);
    out.diagnostics
        .iter()
        .zip(&out.emitted_decisions)
        .map(|(diag, dec)| DecisionRecord::with_decision(diag.clone(), decision_to_metadata(dec)))
        .collect()
}

/// Resolve the effective output format. `--json` is a deprecated alias
/// for `--format json` kept so pre-#29 CI scripts don't break; when
/// set it overrides `--format`. Removal will be a deliberate
/// follow-up after a deprecation cycle.
fn resolve_format(args: &CheckArgs) -> OutputFormat {
    if args.json {
        OutputFormat::Json
    } else {
        args.format.clone()
    }
}

fn decision_to_metadata(d: &Decision) -> DecisionMetadata {
    DecisionMetadata {
        policy_id: d.policy.as_str().to_string(),
        status: d.status.clone(),
        severity_change: d.severity_change.clone(),
        rationale: d.rationale.clone(),
    }
}

/// Re-apply lockfile exceptions to `(diagnostic, decision?)` records.
/// `apply_exceptions` operates on plain `Diagnostic`s, so we strip the
/// decision metadata, call it, and re-pair survivors against the
/// pre-filter list. Matching uses full-`Diagnostic` equality (rule_id +
/// severity + span + concept + message + why + suggested_fix) walked in
/// order so duplicate findings — same rule + span but a different
/// underlying decision — can't have their metadata swapped, and
/// freshly-synthesized LOCUS001 expired-exception warnings fall through
/// as decision-free records. Survivors preserve input order
/// (`apply_exceptions` only filters or appends; it doesn't reorder),
/// so a one-pass parallel walk is sufficient.
fn apply_exceptions_to_records(
    records: Vec<DecisionRecord>,
    air: &locus_air::AirWorkspace,
    lockfile: &Lockfile,
    today: &str,
) -> Vec<DecisionRecord> {
    let mut pre: Vec<(Diagnostic, Option<DecisionMetadata>)> = records
        .into_iter()
        .map(|r| (r.diagnostic, r.decision))
        .collect();
    let pre_diagnostics: Vec<Diagnostic> = pre.iter().map(|(d, _)| d.clone()).collect();
    let post = apply_exceptions(pre_diagnostics, air, lockfile, Some(today));

    let mut cursor = 0usize;
    let mut out = Vec::with_capacity(post.len());
    for d in post {
        let mut matched: Option<DecisionRecord> = None;
        while cursor < pre.len() {
            if pre[cursor].0 == d {
                let meta = pre[cursor].1.take();
                cursor += 1;
                matched = Some(match meta {
                    Some(m) => DecisionRecord::with_decision(d.clone(), m),
                    None => DecisionRecord::from_diagnostic(d.clone()),
                });
                break;
            }
            cursor += 1;
        }
        out.push(matched.unwrap_or_else(|| DecisionRecord::from_diagnostic(d)));
    }
    out
}

fn apply_changed_filter_records(
    records: Vec<DecisionRecord>,
    args: &CheckArgs,
) -> Result<Vec<DecisionRecord>> {
    if !args.changed {
        return Ok(records);
    }
    let workspace_abs = args
        .workspace
        .canonicalize()
        .unwrap_or_else(|_| args.workspace.clone());
    let changed =
        diff::changed_files(&workspace_abs, args.baseline.as_deref()).with_context(|| {
            format!(
                "computing changed files in {} (--changed)",
                workspace_abs.display()
            )
        })?;
    Ok(records
        .into_iter()
        .filter(|r| {
            changed
                .iter()
                .any(|rel| diff::paths_match(&r.diagnostic.span.file, rel, &workspace_abs))
        })
        .collect())
}

fn append_policy_guard_records(
    records: &mut Vec<DecisionRecord>,
    lockfile: &Lockfile,
    args: &CheckArgs,
    mode: CheckMode,
) -> Result<()> {
    let baseline_lockfile = diff::read_baseline_lockfile(&args.workspace, args.baseline.as_deref());
    let pg = locus_core::check_policy_mutation(
        lockfile,
        baseline_lockfile.as_ref(),
        mode,
        args.allow_policy_calibration,
        args.allow_missing_policy_baseline,
    );
    if args.allow_policy_calibration && !pg.is_empty() {
        report_policy_calibration(&pg)?;
    }
    records.extend(pg.into_iter().map(DecisionRecord::from_diagnostic));
    Ok(())
}

/// Print a structured before/after report for Policy Guard diagnostics
/// when `--allow-policy-calibration` is set. The report is informational
/// — the diagnostics themselves are also rendered in normal output, but
/// at Advisory severity. Per #44 §"Calibration mode".
fn report_policy_calibration(pg: &[Diagnostic]) -> Result<()> {
    let stderr = io::stderr();
    let mut w = stderr.lock();
    writeln!(w, "Policy calibration report ({} mutation(s)):", pg.len())?;
    for d in pg {
        writeln!(w, "  [{}] {}", d.rule_id, d.message)?;
        for line in &d.why {
            writeln!(w, "    why: {line}")?;
        }
    }
    writeln!(
        w,
        "(invoked with --allow-policy-calibration; PG001-PG004/PG008 fire as \
         Advisory. PG000 (missing baseline) and PG006 (missing debt \
         metadata) remain strict — calibration legitimizes intentional \
         widening, not a missing audit or missing justification.)"
    )?;
    Ok(())
}

pub fn emit_output(records: &[DecisionRecord], format: &OutputFormat) -> Result<()> {
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    match format {
        OutputFormat::Text => locus_report::text::write(&mut out, records)?,
        OutputFormat::Json => locus_report::json::write(&mut out, records)?,
        OutputFormat::Sarif => locus_report::sarif::write(&mut out, records)?,
    }
    out.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use locus_air::{AirSpan, AirWorkspace};
    use locus_core::Severity;
    use locus_core::governance::{DecisionStatus, SeverityChange};

    fn diag_at(rule_id: &str, line_start: u32, line_end: u32, message: &str) -> Diagnostic {
        Diagnostic {
            rule_id: rule_id.into(),
            severity: Severity::Warning,
            span: AirSpan::new("src/foo.rs", line_start, line_end),
            concept: None,
            message: message.into(),
            why: Vec::new(),
            suggested_fix: None,
        }
    }

    fn meta(policy: &str) -> DecisionMetadata {
        DecisionMetadata {
            policy_id: policy.into(),
            status: DecisionStatus::Active,
            severity_change: SeverityChange::Unchanged,
            rationale: Vec::new(),
        }
    }

    /// Two diagnostics share `(rule_id, file, line_start)` but differ in
    /// `line_end` + message. The pre-fix HashMap re-pair (keyed only on
    /// `(rule_id, file, line_start)`) would smear one decision over both
    /// records. The full-fingerprint walk must keep them straight.
    #[test]
    fn apply_exceptions_to_records_keeps_distinct_decisions_for_same_span_start() {
        let a = diag_at("OT001", 10, 12, "first");
        let b = diag_at("OT001", 10, 15, "second");
        let records = vec![
            DecisionRecord::with_decision(a, meta("alpha")),
            DecisionRecord::with_decision(b, meta("beta")),
        ];
        let air = AirWorkspace::new(Vec::new());
        let lf = Lockfile::empty();
        // No exceptions or hints → every record survives in input order.
        let post = apply_exceptions_to_records(records, &air, &lf, "2026-05-13");
        assert_eq!(post.len(), 2);
        assert_eq!(
            post[0].decision.as_ref().unwrap().policy_id,
            "alpha",
            "first record must retain its own policy id"
        );
        assert_eq!(
            post[1].decision.as_ref().unwrap().policy_id,
            "beta",
            "second record must retain its own policy id, not get smeared from the first"
        );
    }

    /// Two diagnostics with fully-identical fingerprints (rare but
    /// allowed when governance happens to emit twice). The walk should
    /// pair each post-entry with its own pre-entry in order, not lose
    /// any decision metadata.
    #[test]
    fn apply_exceptions_to_records_pairs_identical_diagnostics_in_order() {
        let d = diag_at("CX001", 5, 6, "over budget");
        let records = vec![
            DecisionRecord::with_decision(d.clone(), meta("first")),
            DecisionRecord::with_decision(d.clone(), meta("second")),
        ];
        let air = AirWorkspace::new(Vec::new());
        let lf = Lockfile::empty();
        let post = apply_exceptions_to_records(records, &air, &lf, "2026-05-13");
        assert_eq!(post.len(), 2);
        // For identical fingerprints we can only assert that each post
        // record carries SOME decision metadata; metadata identity
        // between the two is governance-level concern.
        assert!(post[0].decision.is_some());
        assert!(post[1].decision.is_some());
    }

    fn args_with(format: OutputFormat, json: bool) -> CheckArgs {
        CheckArgs {
            workspace: ".".into(),
            agent_strict: false,
            format,
            json,
            changed: false,
            baseline: None,
            allow_policy_calibration: false,
            allow_missing_policy_baseline: false,
            semantic_rust: false,
        }
    }

    #[test]
    fn resolve_format_prefers_deprecated_json_alias_over_format_flag() {
        let args = args_with(OutputFormat::Text, true);
        assert_eq!(resolve_format(&args), OutputFormat::Json);
    }

    #[test]
    fn resolve_format_falls_back_to_format_flag_when_json_unset() {
        let args = args_with(OutputFormat::Sarif, false);
        assert_eq!(resolve_format(&args), OutputFormat::Sarif);
    }
}
