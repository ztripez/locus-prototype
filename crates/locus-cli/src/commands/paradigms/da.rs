use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Subcommand;
use locus_core::Lockfile;
use locus_core::paradigms::demand_driven::{
    DA_PREFIX,
    edit::{add_accepted_single_impl, set_enabled},
    lockfile_schema::DaSection,
};

// locus: ot boundary cli.da cli
#[derive(Subcommand, Debug)]
pub enum DaCommand {
    /// Enable DA paradigm checks.
    Enable(DaToggleArgs),
    /// Disable DA paradigm checks.
    Disable(DaToggleArgs),
    /// Mark a trait pattern as an accepted single-impl abstraction (DA001).
    AcceptSingleImpl(DaAcceptSingleImplArgs),
}

// locus: ot boundary cli.da-toggle cli
#[derive(clap::Args, Debug)]
pub struct DaToggleArgs {
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
}

// locus: ot boundary cli.da-accept-single-impl cli
#[derive(clap::Args, Debug)]
pub struct DaAcceptSingleImplArgs {
    /// Trait symbol pattern (full path or short name).
    pub pattern: String,
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
}

pub fn run(cmd: DaCommand) -> Result<()> {
    match cmd {
        DaCommand::Enable(args) => set_enabled_cmd(args, true),
        DaCommand::Disable(args) => set_enabled_cmd(args, false),
        DaCommand::AcceptSingleImpl(args) => accept_single_impl_cmd(args),
    }
}

fn set_enabled_cmd(args: DaToggleArgs, enabled: bool) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: DaSection = lockfile
        .paradigm_section(DA_PREFIX)
        .context("DA lockfile section is malformed")?;

    set_enabled(&mut section, enabled);

    let value = serde_json::to_value(&section).context("serialize DA section")?;
    lockfile.paradigms.insert(DA_PREFIX.to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!(
        "DA paradigm {}",
        if enabled { "enabled" } else { "disabled" }
    );
    println!("updated {}", written.display());
    Ok(())
}

fn accept_single_impl_cmd(args: DaAcceptSingleImplArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: DaSection = lockfile
        .paradigm_section(DA_PREFIX)
        .context("DA lockfile section is malformed")?;

    add_accepted_single_impl(&mut section, &args.pattern)
        .with_context(|| format!("accept single-impl trait `{}`", args.pattern))?;

    let value = serde_json::to_value(&section).context("serialize DA section")?;
    lockfile.paradigms.insert(DA_PREFIX.to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("accepted single-impl trait pattern `{}`", args.pattern);
    println!("updated {}", written.display());
    Ok(())
}
