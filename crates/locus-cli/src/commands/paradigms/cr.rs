use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Subcommand;
use locus_core::Lockfile;
use locus_core::paradigms::composition_root::{
    CR_PREFIX, edit::add_composition_root, lockfile_schema::CrSection,
};

// locus: ot boundary cli.cr cli
#[derive(Subcommand, Debug)]
pub enum CrCommand {
    /// Declare a module pattern as a composition root (CR001).
    AddCompositionRoot(CrAddCompositionRootArgs),
}

// locus: ot boundary cli.cr-add-composition-root cli
#[derive(clap::Args, Debug)]
pub struct CrAddCompositionRootArgs {
    /// Module pattern matching composition-root files.
    pub pattern: String,
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
}

pub fn run(cmd: CrCommand) -> Result<()> {
    match cmd {
        CrCommand::AddCompositionRoot(args) => add_composition_root_cmd(args),
    }
}

fn add_composition_root_cmd(args: CrAddCompositionRootArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: CrSection = lockfile
        .paradigm_section(CR_PREFIX)
        .context("CR lockfile section is malformed")?;

    add_composition_root(&mut section, &args.pattern)
        .with_context(|| format!("add composition root `{}`", args.pattern))?;

    let value = serde_json::to_value(&section).context("serialize CR section")?;
    lockfile.paradigms.insert(CR_PREFIX.to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("added composition root pattern `{}`", args.pattern);
    println!("updated {}", written.display());
    Ok(())
}
