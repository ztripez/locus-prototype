use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Subcommand;
use locus_core::Lockfile;
use locus_core::paradigms::error_taxonomy::{edit::add_domain_path, lockfile_schema::ErSection};

// locus: ot boundary cli.er cli
#[derive(Subcommand, Debug)]
pub enum ErCommand {
    /// Mark a module pattern as part of the domain layer (ER003).
    AddDomainPath(ErAddDomainPathArgs),
}

// locus: ot boundary cli.er-add-domain-path cli
#[derive(clap::Args, Debug)]
pub struct ErAddDomainPathArgs {
    /// Module path glob, e.g. `"crate::domain::*"`.
    pub pattern: String,
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
}

pub fn run(cmd: ErCommand) -> Result<()> {
    match cmd {
        ErCommand::AddDomainPath(args) => add_domain_path_cmd(args),
    }
}

fn add_domain_path_cmd(args: ErAddDomainPathArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: ErSection = lockfile
        .paradigm_section("ER")
        .context("ER lockfile section is malformed")?;

    add_domain_path(&mut section, &args.pattern)
        .with_context(|| format!("add ER domain path `{}`", args.pattern))?;

    let value = serde_json::to_value(&section).context("serialize ER section")?;
    lockfile.paradigms.insert("ER".to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("added ER domain path pattern `{}`", args.pattern);
    println!("updated {}", written.display());
    Ok(())
}
