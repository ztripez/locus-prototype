use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Subcommand;
use locus_core::Lockfile;
use locus_core::paradigms::test_architecture::{
    TA_PREFIX, edit::add_test_path as ta_add_test_path, lockfile_schema::TaSection,
};

// locus: ot boundary cli.ta cli
#[derive(Subcommand, Debug)]
pub enum TaCommand {
    /// Mark a module pattern as test code (TA001).
    AddTestPath(TaAddTestPathArgs),
}

// locus: ot boundary cli.ta-add-test-path cli
#[derive(clap::Args, Debug)]
pub struct TaAddTestPathArgs {
    /// Module pattern matching test files.
    pub pattern: String,
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
}

pub fn run(cmd: TaCommand) -> Result<()> {
    match cmd {
        TaCommand::AddTestPath(args) => add_test_path_cmd(args),
    }
}

fn add_test_path_cmd(args: TaAddTestPathArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: TaSection = lockfile
        .paradigm_section(TA_PREFIX)
        .context("TA lockfile section is malformed")?;

    ta_add_test_path(&mut section, &args.pattern)
        .with_context(|| format!("add test path `{}`", args.pattern))?;

    let value = serde_json::to_value(&section).context("serialize TA section")?;
    lockfile.paradigms.insert(TA_PREFIX.to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("added test path pattern `{}`", args.pattern);
    println!("updated {}", written.display());
    Ok(())
}
