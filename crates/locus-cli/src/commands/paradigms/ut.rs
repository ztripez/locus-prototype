use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Subcommand;
use locus_core::Lockfile;
use locus_core::paradigms::utility_discipline::{
    UT_PREFIX, edit::add_utility_path, lockfile_schema::UtSection,
};

// locus: ot boundary cli.ut cli
#[derive(Subcommand, Debug)]
pub enum UtCommand {
    /// Mark a module pattern as a utility module (UT001).
    AddUtilityPath(UtAddUtilityPathArgs),
}

// locus: ot boundary cli.ut-add-utility-path cli
#[derive(clap::Args, Debug)]
pub struct UtAddUtilityPathArgs {
    /// Module pattern matching utility modules.
    pub pattern: String,
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
}

pub fn run(cmd: UtCommand) -> Result<()> {
    match cmd {
        UtCommand::AddUtilityPath(args) => add_utility_path_cmd(args),
    }
}

fn add_utility_path_cmd(args: UtAddUtilityPathArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: UtSection = lockfile
        .paradigm_section(UT_PREFIX)
        .context("UT lockfile section is malformed")?;

    add_utility_path(&mut section, &args.pattern)
        .with_context(|| format!("add utility path `{}`", args.pattern))?;

    let value = serde_json::to_value(&section).context("serialize UT section")?;
    lockfile.paradigms.insert(UT_PREFIX.to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("added utility path pattern `{}`", args.pattern);
    println!("updated {}", written.display());
    Ok(())
}
