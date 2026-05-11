use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Subcommand;
use locus_core::Lockfile;
use locus_core::paradigms::documentation::{
    DC_PREFIX,
    edit::{add_exempt_path, set_require_public_docs},
    lockfile_schema::DcSection,
};

// locus: ot boundary cli.dc cli
#[derive(Subcommand, Debug)]
pub enum DcCommand {
    /// Turn DC001's "public API must be documented" check on.
    Enable(DcToggleArgs),
    /// Turn DC001's "public API must be documented" check off.
    Disable(DcToggleArgs),
    /// Add a module pattern exempt from the public-doc requirement (DC001).
    AddExemptPath(DcAddExemptPathArgs),
}

// locus: ot boundary cli.dc-toggle cli
#[derive(clap::Args, Debug)]
pub struct DcToggleArgs {
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
}

// locus: ot boundary cli.dc-add-exempt-path cli
#[derive(clap::Args, Debug)]
pub struct DcAddExemptPathArgs {
    /// Module pattern exempt from DC001.
    pub pattern: String,
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
}

pub fn run(cmd: DcCommand) -> Result<()> {
    match cmd {
        DcCommand::Enable(args) => set_require_cmd(args, true),
        DcCommand::Disable(args) => set_require_cmd(args, false),
        DcCommand::AddExemptPath(args) => add_exempt_path_cmd(args),
    }
}

fn set_require_cmd(args: DcToggleArgs, value: bool) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: DcSection = lockfile
        .paradigm_section(DC_PREFIX)
        .context("DC lockfile section is malformed")?;

    set_require_public_docs(&mut section, value);

    let serialized = serde_json::to_value(&section).context("serialize DC section")?;
    lockfile.paradigms.insert(DC_PREFIX.to_string(), serialized);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!(
        "DC require_public_docs {}",
        if value { "enabled" } else { "disabled" }
    );
    println!("updated {}", written.display());
    Ok(())
}

fn add_exempt_path_cmd(args: DcAddExemptPathArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: DcSection = lockfile
        .paradigm_section(DC_PREFIX)
        .context("DC lockfile section is malformed")?;

    add_exempt_path(&mut section, &args.pattern)
        .with_context(|| format!("add exempt path `{}`", args.pattern))?;

    let value = serde_json::to_value(&section).context("serialize DC section")?;
    lockfile.paradigms.insert(DC_PREFIX.to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("added exempt path pattern `{}`", args.pattern);
    println!("updated {}", written.display());
    Ok(())
}
