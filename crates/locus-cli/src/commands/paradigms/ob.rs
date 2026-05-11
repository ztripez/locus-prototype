use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Subcommand;
use locus_core::Lockfile;
use locus_core::paradigms::observability::{
    OB_PREFIX,
    edit::{
        add_forbidden_log_target as ob_add_forbidden_log_target,
        add_observer_path as ob_add_observer_path,
    },
    lockfile_schema::ObSection,
};

// locus: ot boundary cli.ob cli
#[derive(Subcommand, Debug)]
pub enum ObCommand {
    /// Declare a module pattern as a legitimate observer (OB001).
    AddObserverPath(ObAddObserverPathArgs),
    /// Add a macro pattern to the forbidden log targets list (OB001).
    AddForbiddenLogTarget(ObAddForbiddenLogTargetArgs),
}

// locus: ot boundary cli.ob-add-observer-path cli
#[derive(clap::Args, Debug)]
pub struct ObAddObserverPathArgs {
    /// Module pattern matching observer files.
    pub pattern: String,
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
}

// locus: ot boundary cli.ob-add-forbidden-log-target cli
#[derive(clap::Args, Debug)]
pub struct ObAddForbiddenLogTargetArgs {
    /// Macro path pattern considered raw/inappropriate.
    pub pattern: String,
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
}

pub fn run(cmd: ObCommand) -> Result<()> {
    match cmd {
        ObCommand::AddObserverPath(args) => add_observer_path_cmd(args),
        ObCommand::AddForbiddenLogTarget(args) => add_forbidden_log_target_cmd(args),
    }
}

fn add_observer_path_cmd(args: ObAddObserverPathArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: ObSection = lockfile
        .paradigm_section(OB_PREFIX)
        .context("OB lockfile section is malformed")?;

    ob_add_observer_path(&mut section, &args.pattern)
        .with_context(|| format!("add observer path `{}`", args.pattern))?;

    let value = serde_json::to_value(&section).context("serialize OB section")?;
    lockfile.paradigms.insert(OB_PREFIX.to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("added observer path pattern `{}`", args.pattern);
    println!("updated {}", written.display());
    Ok(())
}

fn add_forbidden_log_target_cmd(args: ObAddForbiddenLogTargetArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: ObSection = lockfile
        .paradigm_section(OB_PREFIX)
        .context("OB lockfile section is malformed")?;

    ob_add_forbidden_log_target(&mut section, &args.pattern)
        .with_context(|| format!("add forbidden log target `{}`", args.pattern))?;

    let value = serde_json::to_value(&section).context("serialize OB section")?;
    lockfile.paradigms.insert(OB_PREFIX.to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("added forbidden log target pattern `{}`", args.pattern);
    println!("updated {}", written.display());
    Ok(())
}
