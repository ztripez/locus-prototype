use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Subcommand;
use locus_core::Lockfile;
use locus_core::paradigms::module_ownership::{
    MO_PREFIX,
    edit::{
        add_override as mo_add_override,
        set_default_max_public_types as mo_set_default_max_public_types,
    },
    lockfile_schema::MoSection,
};

// locus: ot boundary cli.mo cli
#[derive(Subcommand, Debug)]
pub enum MoCommand {
    /// Set the workspace-wide public-types-per-file budget (MO001).
    SetDefault(MoSetDefaultArgs),
    /// Add a per-module public-types budget override (MO001).
    AddOverride(MoAddOverrideArgs),
}

// locus: ot boundary cli.mo-set-default cli
#[derive(clap::Args, Debug)]
pub struct MoSetDefaultArgs {
    /// Maximum number of `pub` top-level types per file.
    #[arg(long)]
    pub max_types: u32,
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
}

// locus: ot boundary cli.mo-add-override cli
#[derive(clap::Args, Debug)]
pub struct MoAddOverrideArgs {
    /// Module pattern this override applies to.
    #[arg(long)]
    pub module: String,
    /// Override budget in number of public types.
    #[arg(long)]
    pub max_types: u32,
    /// Update the budget on an existing override instead of erroring.
    #[arg(long)]
    pub force: bool,
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
}

pub fn run(cmd: MoCommand) -> Result<()> {
    match cmd {
        MoCommand::SetDefault(args) => set_default_cmd(args),
        MoCommand::AddOverride(args) => add_override_cmd(args),
    }
}

fn set_default_cmd(args: MoSetDefaultArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: MoSection = lockfile
        .paradigm_section(MO_PREFIX)
        .context("MO lockfile section is malformed")?;

    mo_set_default_max_public_types(&mut section, args.max_types);

    let value = serde_json::to_value(&section).context("serialize MO section")?;
    lockfile.paradigms.insert(MO_PREFIX.to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("set MO default public-types budget to {}", args.max_types);
    println!("updated {}", written.display());
    Ok(())
}

fn add_override_cmd(args: MoAddOverrideArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: MoSection = lockfile
        .paradigm_section(MO_PREFIX)
        .context("MO lockfile section is malformed")?;

    mo_add_override(&mut section, &args.module, args.max_types, args.force)
        .with_context(|| format!("add MO override for `{}`", args.module))?;

    let value = serde_json::to_value(&section).context("serialize MO section")?;
    lockfile.paradigms.insert(MO_PREFIX.to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!(
        "added MO override `{}` -> {} types",
        args.module, args.max_types
    );
    println!("updated {}", written.display());
    Ok(())
}
