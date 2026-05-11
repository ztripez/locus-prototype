use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Subcommand;
use locus_core::Lockfile;
use locus_core::paradigms::responsibility::{
    RM_PREFIX,
    edit::{
        add_domain_path as rm_add_domain_path, add_exempt_path as rm_add_exempt_path,
        set_default_max_action_kinds as rm_set_default_max_action_kinds,
    },
    lockfile_schema::RmSection,
};

// locus: ot boundary cli.rm cli
#[derive(Subcommand, Debug)]
pub enum RmCommand {
    /// Set the workspace-wide per-function action-kind cap (RM001).
    SetDefault(RmSetDefaultArgs),
    /// Add a module pattern exempt from RM checks.
    AddExemptPath(RmAddExemptPathArgs),
    /// Add a module pattern declaring the domain layer (RM006).
    AddDomainPath(RmAddDomainPathArgs),
}

// locus: ot boundary cli.rm-set-default cli
#[derive(clap::Args, Debug)]
pub struct RmSetDefaultArgs {
    /// Maximum number of distinct action kinds a single function may produce.
    #[arg(long)]
    pub max_kinds: u32,
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
}

// locus: ot boundary cli.rm-add-exempt-path cli
#[derive(clap::Args, Debug)]
pub struct RmAddExemptPathArgs {
    /// Module pattern exempt from RM checks.
    pub pattern: String,
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
}

// locus: ot boundary cli.rm-add-domain-path cli
#[derive(clap::Args, Debug)]
pub struct RmAddDomainPathArgs {
    /// Module path glob, e.g. `"crate::domain::*"`.
    pub pattern: String,
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
}

pub fn run(cmd: RmCommand) -> Result<()> {
    match cmd {
        RmCommand::SetDefault(args) => set_default_cmd(args),
        RmCommand::AddExemptPath(args) => add_exempt_path_cmd(args),
        RmCommand::AddDomainPath(args) => add_domain_path_cmd(args),
    }
}

fn set_default_cmd(args: RmSetDefaultArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: RmSection = lockfile
        .paradigm_section(RM_PREFIX)
        .context("RM lockfile section is malformed")?;

    rm_set_default_max_action_kinds(&mut section, args.max_kinds);

    let value = serde_json::to_value(&section).context("serialize RM section")?;
    lockfile.paradigms.insert(RM_PREFIX.to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("set RM default action-kind cap to {}", args.max_kinds);
    println!("updated {}", written.display());
    Ok(())
}

fn add_exempt_path_cmd(args: RmAddExemptPathArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: RmSection = lockfile
        .paradigm_section(RM_PREFIX)
        .context("RM lockfile section is malformed")?;

    rm_add_exempt_path(&mut section, &args.pattern)
        .with_context(|| format!("add exempt path `{}`", args.pattern))?;

    let value = serde_json::to_value(&section).context("serialize RM section")?;
    lockfile.paradigms.insert(RM_PREFIX.to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("added exempt path pattern `{}`", args.pattern);
    println!("updated {}", written.display());
    Ok(())
}

fn add_domain_path_cmd(args: RmAddDomainPathArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: RmSection = lockfile
        .paradigm_section(RM_PREFIX)
        .context("RM lockfile section is malformed")?;

    rm_add_domain_path(&mut section, &args.pattern)
        .with_context(|| format!("add RM domain path `{}`", args.pattern))?;

    let value = serde_json::to_value(&section).context("serialize RM section")?;
    lockfile.paradigms.insert(RM_PREFIX.to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("added RM domain path pattern `{}`", args.pattern);
    println!("updated {}", written.display());
    Ok(())
}
