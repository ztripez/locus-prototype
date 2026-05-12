use std::io::{self, BufWriter, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use locus_core::{
    CheckMode, Diagnostic, Lockfile, Severity, apply_exceptions, governance, today_utc,
};

use crate::diff;

// locus: ot boundary cli.check cli
#[derive(clap::Args, Debug)]
pub struct CheckArgs {
    /// Workspace root (containing Cargo.toml).
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
    /// Treat warnings as fatal. Use this for LLM-generated patches.
    #[arg(long)]
    pub agent_strict: bool,
    /// Emit diagnostics as JSON instead of human-readable text.
    #[arg(long)]
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

    // Run the governance pipeline. Returns diagnostics already
    // materialized through DefaultPassThroughPolicy — byte-identical to
    // the prior `for paradigm in registry() { paradigm.check(...) }` loop
    // under P1's empty rule registry.
    let governance_out = governance::run(&air, &lockfile, mode);
    let all = governance_out.diagnostics;

    // Apply exceptions BEFORE Policy Guard — PG must not be suppressible by
    // the same lockfile it audits. See #44.
    let today = today_utc();
    let all = apply_exceptions(all, &air, &lockfile, Some(&today));

    // --changed filter is applied before PG so PG diagnostics bypass it
    // (PG is global; it must not be hidden by a PR-scoped diff filter).
    let mut all = apply_changed_filter(all, &args)?;

    // Policy Guard appended last: after apply_exceptions and --changed.
    append_policy_guard(&mut all, &lockfile, &args, mode)?;

    emit_output(&all, args.json)?;

    let any_fatal = all.iter().any(|d| d.severity.is_fatal());
    if any_fatal {
        std::process::exit(1);
    }
    Ok(())
}

fn apply_changed_filter(all: Vec<Diagnostic>, args: &CheckArgs) -> Result<Vec<Diagnostic>> {
    if !args.changed {
        return Ok(all);
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
    Ok(all
        .into_iter()
        .filter(|d| {
            changed
                .iter()
                .any(|rel| diff::paths_match(&d.span.file, rel, &workspace_abs))
        })
        .collect())
}

fn append_policy_guard(
    all: &mut Vec<Diagnostic>,
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
    all.extend(pg);
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

pub fn emit_output(all: &[Diagnostic], json: bool) -> Result<()> {
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    if json {
        serde_json::to_writer_pretty(&mut out, all)?;
        writeln!(out)?;
    } else {
        report_text(&mut out, all)?;
    }
    out.flush()?;
    Ok(())
}

pub fn report_text<W: Write>(out: &mut W, diags: &[Diagnostic]) -> io::Result<()> {
    if diags.is_empty() {
        writeln!(out, "no diagnostics — workspace is clean.")?;
        return Ok(());
    }
    let mut fatal = 0usize;
    let mut warning = 0usize;
    let mut advisory = 0usize;
    for d in diags {
        let label = match d.severity {
            Severity::Fatal => {
                fatal += 1;
                "error"
            }
            Severity::Warning => {
                warning += 1;
                "warning"
            }
            Severity::Advisory => {
                advisory += 1;
                "info"
            }
        };
        writeln!(
            out,
            "{label}[{}]: {}\n  --> {}:{}",
            d.rule_id, d.message, d.span.file, d.span.line_start
        )?;
        if let Some(c) = &d.concept {
            writeln!(out, "  concept: {c}")?;
        }
        for reason in &d.why {
            writeln!(out, "  - {reason}")?;
        }
        if let Some(fix) = &d.suggested_fix {
            writeln!(out, "  fix: {fix}")?;
        }
        writeln!(out)?;
    }
    writeln!(
        out,
        "summary: {fatal} error(s), {warning} warning(s), {advisory} advisory."
    )?;
    Ok(())
}
