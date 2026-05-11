use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Subcommand;
use locus_core::Lockfile;
use locus_core::paradigms::runtime_work::{
    edit::add_runtime_owner_path, lockfile_schema::RwSection,
};

// locus: ot boundary cli.rw cli
#[derive(Subcommand, Debug)]
pub enum RwCommand {
    /// Mark a module pattern as a runtime owner (RW001).
    AcceptRuntimeOwner(RwAcceptRuntimeOwnerArgs),
}

// locus: ot boundary cli.rw-accept-runtime-owner cli
#[derive(clap::Args, Debug)]
pub struct RwAcceptRuntimeOwnerArgs {
    /// Module path glob.
    pub pattern: String,
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
}

pub fn run(cmd: RwCommand) -> Result<()> {
    match cmd {
        RwCommand::AcceptRuntimeOwner(args) => accept_runtime_owner_cmd(args),
    }
}

fn accept_runtime_owner_cmd(args: RwAcceptRuntimeOwnerArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: RwSection = lockfile
        .paradigm_section("RW")
        .context("RW lockfile section is malformed")?;

    add_runtime_owner_path(&mut section, &args.pattern)
        .with_context(|| format!("add RW runtime owner path `{}`", args.pattern))?;

    let value = serde_json::to_value(&section).context("serialize RW section")?;
    lockfile.paradigms.insert("RW".to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("added RW runtime owner pattern `{}`", args.pattern);
    println!("updated {}", written.display());
    Ok(())
}
