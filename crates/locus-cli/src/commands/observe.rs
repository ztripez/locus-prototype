//! `locus observe` — read-only architecture survey + advisory pressure.
//!
//! Answers three questions, in this exact order:
//!
//! 1. **What structure does Locus see?** Top-level modules, candidate
//!    concept clusters, detected layer candidates, largest modules,
//!    cross-crate edges.
//! 2. **What architectural pressure would Locus report if governance were
//!    enabled?** Rule findings grouped by paradigm/rule with counts. All
//!    findings rendered as Advisory regardless of underlying severity —
//!    `observe` is not a gate.
//! 3. **What should the user consider declaring?** Reuses
//!    `Paradigm::suggest` and `cross_paradigm_suggestions` to point at
//!    declarations the user might want to add to `.locus/lock.json` or
//!    `.locus/arch.json`.
//!
//! Always exits 0. Use `locus check` once you're ready to enforce.

use std::collections::BTreeMap;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use locus_core::{CheckMode, Lockfile, Severity, apply_exceptions, governance, today_utc};

// locus: ot boundary cli.observe cli
#[derive(clap::Args, Debug)]
pub struct ObserveArgs {
    /// Workspace root (default: current directory).
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
}

pub fn run(args: ObserveArgs) -> Result<()> {
    let air = locus_rust::scan(&args.workspace)
        .with_context(|| format!("scan failed: {}", args.workspace.display()))?;
    let lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;

    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());

    render_survey(&mut out, &args.workspace, &air)?;
    writeln!(out)?;
    render_pressure(&mut out, &air, &lockfile, &args.workspace)?;
    writeln!(out)?;
    render_declarations(&mut out, &air, &lockfile)?;

    out.flush()?;
    Ok(())
}

// ---------------------------------------------------------------------
// Section 1 — Architecture Survey
// ---------------------------------------------------------------------

fn render_survey<W: Write>(
    out: &mut W,
    workspace: &Path,
    air: &locus_air::AirWorkspace,
) -> io::Result<()> {
    writeln!(out, "Architecture survey")?;
    writeln!(out, "===================")?;
    writeln!(out)?;
    writeln!(out, "Workspace: {}", workspace.display())?;

    render_top_level_modules(out, air)?;
    render_concept_clusters(out, air)?;
    render_layers(out, air)?;
    render_largest_modules(out, air)?;
    render_crate_edges(out, air)?;
    Ok(())
}

fn render_top_level_modules<W: Write>(
    out: &mut W,
    air: &locus_air::AirWorkspace,
) -> io::Result<()> {
    let modules = locus_core::init::top_level_modules(air);
    if modules.is_empty() {
        writeln!(out, "Top-level modules: (none detected)")?;
    } else {
        writeln!(out, "Top-level modules: {}", modules.join(", "))?;
    }
    Ok(())
}

fn render_concept_clusters<W: Write>(
    out: &mut W,
    air: &locus_air::AirWorkspace,
) -> io::Result<()> {
    let mut clusters =
        locus_core::paradigms::one_truth::infer::cluster_concepts(air);
    // Sort by member count descending, then by stem for determinism.
    clusters.sort_by(|a, b| {
        b.members
            .len()
            .cmp(&a.members.len())
            .then_with(|| a.stem.cmp(&b.stem))
    });
    writeln!(out)?;
    writeln!(out, "Candidate concept clusters (top 5):")?;
    if clusters.is_empty() {
        writeln!(out, "  (none detected)")?;
        return Ok(());
    }
    for cluster in clusters.iter().take(5) {
        let names: Vec<&str> = cluster
            .members
            .iter()
            .map(|m| m.name.as_str())
            .collect();
        writeln!(
            out,
            "  {}: {} ({} members)",
            cluster.stem,
            names.join(", "),
            cluster.members.len()
        )?;
    }
    Ok(())
}

fn render_layers<W: Write>(out: &mut W, air: &locus_air::AirWorkspace) -> io::Result<()> {
    let layers = locus_core::init::detect_layers(air);
    writeln!(out)?;
    writeln!(out, "Detected layers:")?;
    let rows: &[(&str, &Vec<String>)] = &[
        ("domain candidates", &layers.domain),
        ("api/boundary candidates", &layers.api_or_boundary),
        ("application candidates", &layers.application),
        ("composition candidates", &layers.composition),
        ("test candidates", &layers.tests),
        ("utility candidates", &layers.utilities),
        ("config candidates", &layers.config),
    ];
    let any_present = rows.iter().any(|(_, v)| !v.is_empty());
    if !any_present {
        writeln!(out, "  (none detected)")?;
        return Ok(());
    }
    for (label, globs) in rows {
        if globs.is_empty() {
            continue;
        }
        writeln!(out, "  {label}: {}", globs.join(", "))?;
    }
    Ok(())
}

fn render_largest_modules<W: Write>(
    out: &mut W,
    air: &locus_air::AirWorkspace,
) -> io::Result<()> {
    let mut files: Vec<(&str, u32)> = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            files.push((file.path.as_str(), file.line_count));
        }
    }
    // Sort by line_count descending, then by path for determinism.
    files.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    writeln!(out)?;
    writeln!(out, "Largest modules (top 5):")?;
    if files.is_empty() {
        writeln!(out, "  (none detected)")?;
        return Ok(());
    }
    for (path, lines) in files.iter().take(5) {
        writeln!(out, "  {path} ({lines} lines)")?;
    }
    Ok(())
}

fn render_crate_edges<W: Write>(
    out: &mut W,
    air: &locus_air::AirWorkspace,
) -> io::Result<()> {
    let edges =
        locus_core::paradigms::dependency_graph::collect_crate_edges(air);
    writeln!(out)?;
    writeln!(out, "Cross-crate edges:")?;
    if edges.is_empty() {
        writeln!(out, "  (none detected)")?;
        return Ok(());
    }
    // `edges` is a BTreeMap keyed on `(importer, imported)` so iteration is
    // already lexicographic.
    for (importer, imported) in edges.keys() {
        writeln!(out, "  {importer} -> {imported}")?;
    }
    Ok(())
}

// ---------------------------------------------------------------------
// Section 2 — Advisory Pressure
// ---------------------------------------------------------------------

fn render_pressure<W: Write>(
    out: &mut W,
    air: &locus_air::AirWorkspace,
    lockfile: &Lockfile,
    workspace: &Path,
) -> io::Result<()> {
    writeln!(out, "Advisory pressure")?;
    writeln!(out, "=================")?;
    writeln!(out)?;

    let governance_codes = governance::GovernanceDiagnosticRegistry::standard();
    let diags = collect_advisory_diagnostics(air, lockfile, workspace);

    if diags.is_empty() {
        writeln!(out, "(no advisory pressure detected)")?;
        return Ok(());
    }

    let by_paradigm = group_diagnostics(&diags, &governance_codes);
    write_pressure_summary(out, &diags, &by_paradigm, &governance_codes)?;
    writeln!(
        out,
        "Run `locus check --workspace <path>` for full diagnostic detail."
    )?;
    Ok(())
}

/// Collect the post-exception, severity-overridden diagnostics for
/// `render_pressure`. Runs governance in `CheckMode::Human` then forces
/// every diagnostic's severity to Advisory — `observe` is read-only.
fn collect_advisory_diagnostics(
    air: &locus_air::AirWorkspace,
    lockfile: &Lockfile,
    workspace: &Path,
) -> Vec<locus_core::Diagnostic> {
    let governance_out =
        governance::run_with_workspace_root(air, lockfile, CheckMode::Human, workspace);
    let today = today_utc();
    let mut all = apply_exceptions(governance_out.diagnostics, air, lockfile, Some(&today));
    for d in all.iter_mut() {
        d.severity = Severity::Advisory;
    }
    all
}

fn group_diagnostics(
    diags: &[locus_core::Diagnostic],
    governance_codes: &governance::GovernanceDiagnosticRegistry,
) -> BTreeMap<String, BTreeMap<String, usize>> {
    let mut by_paradigm: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    for d in diags {
        let prefix = paradigm_prefix_for(&d.rule_id, governance_codes);
        *by_paradigm
            .entry(prefix)
            .or_default()
            .entry(d.rule_id.clone())
            .or_insert(0) += 1;
    }
    by_paradigm
}

fn write_pressure_summary<W: Write>(
    out: &mut W,
    diags: &[locus_core::Diagnostic],
    by_paradigm: &BTreeMap<String, BTreeMap<String, usize>>,
    governance_codes: &governance::GovernanceDiagnosticRegistry,
) -> io::Result<()> {
    let registry = governance::RuleRegistry::standard();
    let paradigm_reg = governance::ParadigmRegistry::standard();
    let legacy_paradigms = locus_core::registry();

    writeln!(
        out,
        "{} findings across {} paradigms (all advisory; nothing blocks):",
        diags.len(),
        by_paradigm.len()
    )?;
    writeln!(out)?;

    for (prefix, rules) in by_paradigm {
        let header = paradigm_header(prefix, &paradigm_reg, &legacy_paradigms);
        writeln!(out, "  {header}")?;
        for (rule_id, count) in rules {
            let title = rule_title(rule_id, &registry, governance_codes);
            let suffix = if *count == 1 { "finding" } else { "findings" };
            writeln!(out, "    {rule_id} — {title}: {count} {suffix}")?;
        }
        writeln!(out)?;
    }
    Ok(())
}

fn paradigm_prefix_for(
    rule_id: &str,
    governance_codes: &governance::GovernanceDiagnosticRegistry,
) -> String {
    if governance_codes.contains(rule_id) {
        return "GOV".to_string();
    }
    // Rule ids follow `<PREFIX><digits>` shape, e.g. `OT001`. PG codes follow
    // `PG<digits>` shape — treat the PG prefix the same way.
    let prefix: String = rule_id
        .chars()
        .take_while(|c| c.is_ascii_uppercase())
        .collect();
    if prefix.is_empty() {
        "(unknown)".to_string()
    } else {
        prefix
    }
}

fn paradigm_header(
    prefix: &str,
    paradigm_reg: &governance::ParadigmRegistry,
    legacy: &[Box<dyn locus_core::Paradigm>],
) -> String {
    if prefix == "GOV" {
        return "GOV (Governance health-checks)".to_string();
    }
    if prefix == "PG" {
        return "PG (Policy Guard)".to_string();
    }
    // First try the governance-spine `ParadigmRegistry`. IDs are
    // `&'static str`-backed so we string-compare via the iterator.
    if let Some(p) = paradigm_reg.iter().find(|p| p.id().as_str() == prefix) {
        return format!("{prefix} ({})", p.title());
    }
    // Fallback to legacy paradigm registry (still the source of truth for
    // human-facing names).
    if let Some(p) = legacy.iter().find(|p| p.rule_prefix() == prefix) {
        return format!("{prefix} ({})", p.name());
    }
    prefix.to_string()
}

fn rule_title(
    rule_id: &str,
    registry: &governance::RuleRegistry,
    governance_codes: &governance::GovernanceDiagnosticRegistry,
) -> String {
    if let Some(rule) = registry.iter().find(|r| r.id().as_str() == rule_id) {
        return rule.title().to_string();
    }
    if governance_codes.contains(rule_id) {
        if let Some(owner) = governance_codes.owner(rule_id) {
            return format!("(governance: {})", owner.as_str());
        }
        return "(governance)".to_string();
    }
    "(legacy)".to_string()
}

// ---------------------------------------------------------------------
// Section 3 — Next Declarations
// ---------------------------------------------------------------------

fn render_declarations<W: Write>(
    out: &mut W,
    air: &locus_air::AirWorkspace,
    lockfile: &Lockfile,
) -> io::Result<()> {
    writeln!(out, "Next declarations to consider")?;
    writeln!(out, "=============================")?;
    writeln!(out)?;

    let registry = locus_core::registry();
    let mut suggestions: Vec<locus_core::Suggestion> = Vec::new();
    for paradigm in &registry {
        suggestions.extend(paradigm.suggest(air, lockfile));
    }
    suggestions.extend(locus_core::init::cross_paradigm_suggestions(air, lockfile));
    let seeds = locus_core::init::default_vacancy_seeds();
    suggestions.extend(locus_core::init::vacancy_seeds(
        air,
        lockfile,
        seeds,
        &suggestions,
    ));
    let suggestions = locus_core::init::aggregate(suggestions);

    if suggestions.is_empty() {
        writeln!(out, "(nothing to declare — workspace is fully onboarded)")?;
        return Ok(());
    }

    for (i, s) in suggestions.iter().enumerate() {
        if i > 0 {
            writeln!(out)?;
        }
        writeln!(out, "{}", s.render())?;
    }
    Ok(())
}
