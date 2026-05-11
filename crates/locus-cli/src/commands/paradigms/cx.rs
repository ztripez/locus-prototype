use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Subcommand;
use locus_core::Lockfile;
use locus_core::paradigms::complexity_budget::{
    CX_PREFIX,
    edit::{add_override, set_default_max_lines},
    lockfile_schema::CxSection,
};

// locus: ot boundary cli.cx cli
#[derive(Subcommand, Debug)]
pub enum CxCommand {
    /// Set the workspace-wide function-line budget (CX001).
    SetDefault(CxSetDefaultArgs),
    /// Add a per-module function-line override (CX001).
    AddOverride(CxAddOverrideArgs),
}

// locus: ot boundary cli.cx-set-default cli
#[derive(clap::Args, Debug)]
pub struct CxSetDefaultArgs {
    /// Maximum number of lines a single function may span.
    #[arg(long)]
    pub max_lines: u32,
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
}

// locus: ot boundary cli.cx-add-override cli
#[derive(clap::Args, Debug)]
pub struct CxAddOverrideArgs {
    /// Module pattern this override applies to.
    #[arg(long)]
    pub module: String,
    /// Override budget in lines.
    #[arg(long)]
    pub max_lines: u32,
    /// Update the budget on an existing override instead of erroring.
    #[arg(long)]
    pub force: bool,
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
}

pub fn run(cmd: CxCommand) -> Result<()> {
    match cmd {
        CxCommand::SetDefault(args) => set_default_cmd(args),
        CxCommand::AddOverride(args) => add_override_cmd(args),
    }
}

fn set_default_cmd(args: CxSetDefaultArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: CxSection = lockfile
        .paradigm_section(CX_PREFIX)
        .context("CX lockfile section is malformed")?;

    set_default_max_lines(&mut section, args.max_lines);

    let value = serde_json::to_value(&section).context("serialize CX section")?;
    lockfile.paradigms.insert(CX_PREFIX.to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("set CX default function-line budget to {}", args.max_lines);
    println!("updated {}", written.display());
    Ok(())
}

fn add_override_cmd(args: CxAddOverrideArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: CxSection = lockfile
        .paradigm_section(CX_PREFIX)
        .context("CX lockfile section is malformed")?;

    add_override(&mut section, &args.module, args.max_lines, args.force)
        .with_context(|| format!("add CX override for `{}`", args.module))?;

    let value = serde_json::to_value(&section).context("serialize CX section")?;
    lockfile.paradigms.insert(CX_PREFIX.to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!(
        "added CX override `{}` -> {} lines",
        args.module, args.max_lines
    );
    println!("updated {}", written.display());
    Ok(())
}
