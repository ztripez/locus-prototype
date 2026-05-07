use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use locus_core::paradigms::one_truth::{
    OT_PREFIX,
    accept::{accept_boundary, accept_canonical},
    lockfile_schema::OtSection,
};
use locus_core::{CheckMode, Diagnostic, Lockfile, Severity, registry};

// ot: boundary cli.invocation cli
#[derive(Parser, Debug)]
#[command(name = "locus", version, about = "Locus — architecture verifier")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

// ot: boundary cli.command cli
#[derive(Subcommand, Debug)]
enum Command {
    /// Scan a Rust workspace and emit AIR JSON.
    EmitAir(EmitAirArgs),
    /// Build `locus.lock` from a fresh workspace scan.
    Init(InitArgs),
    /// Run all enabled paradigms against a workspace and report diagnostics.
    Check(CheckArgs),
    /// Record a symbol's accepted ownership in `locus.lock` (OT paradigm).
    #[command(subcommand)]
    Accept(AcceptCommand),
}

// ot: boundary cli.accept cli
#[derive(Subcommand, Debug)]
enum AcceptCommand {
    /// Accept a symbol as the canonical type for a concept.
    Canonical(AcceptCanonicalArgs),
    /// Accept a symbol as a boundary adapter for an existing concept.
    Boundary(AcceptBoundaryArgs),
}

// ot: boundary cli.accept-canonical cli
#[derive(clap::Args, Debug)]
struct AcceptCanonicalArgs {
    /// Fully-qualified symbol of the canonical type, e.g. `crate::domain::User`.
    symbol: String,
    /// Concept id to bind to. Defaults to the symbol's name stem.
    #[arg(long)]
    concept: Option<String>,
    /// Replace an existing canonical for the concept.
    #[arg(long)]
    force: bool,
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}

// ot: boundary cli.accept-boundary cli
#[derive(clap::Args, Debug)]
struct AcceptBoundaryArgs {
    /// Fully-qualified symbol of the boundary type, e.g. `crate::api::UserDto`.
    symbol: String,
    /// Concept id this boundary belongs to. Required.
    #[arg(long)]
    concept: String,
    /// Boundary label, e.g. `api.v1`, `persistence`, `proto`.
    #[arg(long)]
    boundary: Option<String>,
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}

// ot: boundary cli.init cli
#[derive(clap::Args, Debug)]
struct InitArgs {
    /// Workspace root (containing Cargo.toml).
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
    /// Refuse to overwrite an existing locus.lock.
    #[arg(long)]
    no_overwrite: bool,
}

// ot: boundary cli.emit-air cli
#[derive(clap::Args, Debug)]
struct EmitAirArgs {
    /// Workspace root (containing Cargo.toml).
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
    /// Output file. Defaults to stdout.
    #[arg(long)]
    output: Option<PathBuf>,
    /// Pretty-print JSON.
    #[arg(long)]
    pretty: bool,
}

// ot: boundary cli.check cli
#[derive(clap::Args, Debug)]
struct CheckArgs {
    /// Workspace root (containing Cargo.toml).
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
    /// Treat warnings as fatal. Use this for LLM-generated patches.
    #[arg(long)]
    agent_strict: bool,
    /// Emit diagnostics as JSON instead of human-readable text.
    #[arg(long)]
    json: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::EmitAir(args) => emit_air(args),
        Command::Init(args) => init(args),
        Command::Check(args) => check(args),
        Command::Accept(cmd) => accept(cmd),
    }
}

fn accept(cmd: AcceptCommand) -> Result<()> {
    let workspace = match &cmd {
        AcceptCommand::Canonical(a) => a.workspace.clone(),
        AcceptCommand::Boundary(a) => a.workspace.clone(),
    };
    let air = locus_rust::scan(&workspace)
        .with_context(|| format!("scan failed: {}", workspace.display()))?;
    let mut lockfile = Lockfile::load_or_empty(&workspace)
        .with_context(|| format!("load lockfile from {}", workspace.display()))?;

    let mut section: OtSection = lockfile
        .paradigm_section(OT_PREFIX)
        .context("OT lockfile section is malformed")?;

    let summary = match cmd {
        AcceptCommand::Canonical(a) => {
            let cid =
                accept_canonical(&mut section, &air, &a.symbol, a.concept.as_deref(), a.force)
                    .with_context(|| format!("accept canonical `{}`", a.symbol))?;
            format!("accepted `{}` as canonical for concept `{cid}`", a.symbol)
        }
        AcceptCommand::Boundary(a) => {
            accept_boundary(
                &mut section,
                &air,
                &a.symbol,
                &a.concept,
                a.boundary.as_deref(),
            )
            .with_context(|| format!("accept boundary `{}`", a.symbol))?;
            format!(
                "accepted `{}` as boundary for concept `{}`{}",
                a.symbol,
                a.concept,
                a.boundary
                    .as_deref()
                    .map(|b| format!(" (label `{b}`)"))
                    .unwrap_or_default()
            )
        }
    };

    let value = serde_json::to_value(&section).context("serialize OT section")?;
    lockfile.paradigms.insert(OT_PREFIX.to_string(), value);
    let written = lockfile
        .save(&workspace)
        .with_context(|| format!("write lockfile to {}", workspace.display()))?;

    println!("{summary}");
    println!("updated {}", written.display());
    Ok(())
}

fn init(args: InitArgs) -> Result<()> {
    use locus_core::lockfile::LOCKFILE_NAME;

    let lockfile_path = args.workspace.join(LOCKFILE_NAME);
    if args.no_overwrite && lockfile_path.exists() {
        anyhow::bail!(
            "{} already exists; rerun without --no-overwrite to replace it",
            lockfile_path.display()
        );
    }

    let air = locus_rust::scan(&args.workspace)
        .with_context(|| format!("scan failed: {}", args.workspace.display()))?;

    let mut lockfile = Lockfile::empty();
    let registry = registry();
    for paradigm in &registry {
        let section = paradigm.init(&air);
        if !section_is_empty(&section) {
            lockfile
                .paradigms
                .insert(paradigm.rule_prefix().to_string(), section);
        }
    }

    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("wrote {}", written.display());
    for paradigm in &registry {
        let count = lockfile
            .paradigms
            .get(paradigm.rule_prefix())
            .map(summarize_section)
            .unwrap_or_else(|| "(empty)".to_string());
        println!(
            "  {} {}: {}",
            paradigm.rule_prefix(),
            paradigm.name(),
            count
        );
    }
    Ok(())
}

fn section_is_empty(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Null => true,
        serde_json::Value::Object(m) => m.is_empty() || m.values().all(section_is_empty),
        serde_json::Value::Array(a) => a.is_empty(),
        _ => false,
    }
}

fn summarize_section(v: &serde_json::Value) -> String {
    // Best-effort summary; specific paradigms can override later by exposing
    // their own renderer when there's enough variety to justify it.
    if let Some(concepts) = v.get("concepts").and_then(|c| c.as_object()) {
        let canonicals = concepts.len();
        let boundaries: usize = concepts
            .values()
            .filter_map(|c| c.get("boundaries").and_then(|b| b.as_array()))
            .map(|a| a.len())
            .sum();
        let converters: usize = concepts
            .values()
            .filter_map(|c| c.get("converters").and_then(|b| b.as_array()))
            .map(|a| a.len())
            .sum();
        return format!(
            "{canonicals} concept(s), {boundaries} boundary(ies), {converters} converter(s)"
        );
    }
    "section recorded".to_string()
}

fn emit_air(args: EmitAirArgs) -> Result<()> {
    let air = locus_rust::scan(&args.workspace)
        .with_context(|| format!("scan failed: {}", args.workspace.display()))?;

    let mut writer: Box<dyn Write> = match args.output {
        Some(path) => Box::new(BufWriter::new(
            File::create(&path).with_context(|| format!("create {}", path.display()))?,
        )),
        None => Box::new(BufWriter::new(io::stdout().lock())),
    };

    if args.pretty {
        serde_json::to_writer_pretty(&mut writer, &air)?;
    } else {
        serde_json::to_writer(&mut writer, &air)?;
    }
    writer.write_all(b"\n")?;
    Ok(())
}

fn check(args: CheckArgs) -> Result<()> {
    let air = locus_rust::scan(&args.workspace)
        .with_context(|| format!("scan failed: {}", args.workspace.display()))?;
    let lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mode = if args.agent_strict {
        CheckMode::AgentStrict
    } else {
        CheckMode::Human
    };

    let mut all = Vec::new();
    for paradigm in registry() {
        all.extend(paradigm.check(&air, &lockfile, mode));
    }

    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    if args.json {
        serde_json::to_writer_pretty(&mut out, &all)?;
        writeln!(out)?;
    } else {
        report_text(&mut out, &all)?;
    }
    out.flush()?;
    drop(out);

    let any_fatal = all.iter().any(|d| d.severity.is_fatal());
    if any_fatal {
        std::process::exit(1);
    }
    Ok(())
}

fn report_text<W: Write>(out: &mut W, diags: &[Diagnostic]) -> io::Result<()> {
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
