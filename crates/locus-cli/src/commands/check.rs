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
}

pub fn run(args: CheckArgs) -> Result<()> {
    let air = locus_rust::scan(&args.workspace)
        .with_context(|| format!("scan failed: {}", args.workspace.display()))?;
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

    emit_output(&records, &args.format)?;

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

fn decision_to_metadata(d: &Decision) -> DecisionMetadata {
    DecisionMetadata {
        policy_id: d.policy.as_str().to_string(),
        status: d.status.clone(),
        severity_change: d.severity_change.clone(),
        rationale: d.rationale.clone(),
    }
}

/// Re-apply lockfile exceptions to `(diagnostic, decision?)` records.
/// `apply_exceptions` operates on plain `Diagnostic`s; we strip the
/// decision metadata to call it, then re-pair survivors by their
/// `(rule_id, span)` key. Filtered-out diagnostics drop their pairing
/// naturally. Expired-exception LOCUS001 warnings inserted by
/// `apply_exceptions` are wrapped without decision metadata since they
/// don't flow through the governance pipeline today.
fn apply_exceptions_to_records(
    records: Vec<DecisionRecord>,
    air: &locus_air::AirWorkspace,
    lockfile: &Lockfile,
    today: &str,
) -> Vec<DecisionRecord> {
    use std::collections::HashMap;
    type Key = (String, String, u32);
    let pre: Vec<Diagnostic> = records.iter().map(|r| r.diagnostic.clone()).collect();
    let mut decision_by_key: HashMap<Key, DecisionMetadata> = HashMap::new();
    for r in &records {
        if let Some(dec) = &r.decision {
            let key = (
                r.diagnostic.rule_id.clone(),
                r.diagnostic.span.file.clone(),
                r.diagnostic.span.line_start,
            );
            decision_by_key.entry(key).or_insert_with(|| dec.clone());
        }
    }
    let post = apply_exceptions(pre, air, lockfile, Some(today));
    post.into_iter()
        .map(|d| {
            let key = (d.rule_id.clone(), d.span.file.clone(), d.span.line_start);
            match decision_by_key.get(&key) {
                Some(dec) => DecisionRecord::with_decision(d, dec.clone()),
                None => DecisionRecord::from_diagnostic(d),
            }
        })
        .collect()
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
