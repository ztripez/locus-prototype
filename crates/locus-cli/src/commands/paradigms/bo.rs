use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Subcommand;
use locus_core::Lockfile;
use locus_core::paradigms::boundary_ownership::{
    BO_PREFIX,
    edit::{add_domain_path, add_forbidden_import},
    lockfile_schema::BoSection,
};

// locus: ot boundary cli.bo cli
#[derive(Subcommand, Debug)]
pub enum BoCommand {
    /// Mark a module pattern as domain/application code (BO001).
    AddDomainPath(BoAddDomainPathArgs),
    /// Mark an import-path pattern as forbidden inside the domain layer (BO001).
    AddForbiddenImport(BoAddForbiddenImportArgs),
}

// locus: ot boundary cli.bo-add-domain-path cli
#[derive(clap::Args, Debug)]
pub struct BoAddDomainPathArgs {
    /// Module pattern matching domain/application files.
    pub pattern: String,
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
}

// locus: ot boundary cli.bo-add-forbidden-import cli
#[derive(clap::Args, Debug)]
pub struct BoAddForbiddenImportArgs {
    /// Import-path pattern that domain code must not reach.
    pub pattern: String,
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
}

pub fn run(cmd: BoCommand) -> Result<()> {
    match cmd {
        BoCommand::AddDomainPath(args) => add_domain_path_cmd(args),
        BoCommand::AddForbiddenImport(args) => add_forbidden_import_cmd(args),
    }
}

fn add_domain_path_cmd(args: BoAddDomainPathArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: BoSection = lockfile
        .paradigm_section(BO_PREFIX)
        .context("BO lockfile section is malformed")?;

    add_domain_path(&mut section, &args.pattern)
        .with_context(|| format!("add domain path `{}`", args.pattern))?;

    let value = serde_json::to_value(&section).context("serialize BO section")?;
    lockfile.paradigms.insert(BO_PREFIX.to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("added domain path pattern `{}`", args.pattern);
    println!("updated {}", written.display());
    Ok(())
}

fn add_forbidden_import_cmd(args: BoAddForbiddenImportArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: BoSection = lockfile
        .paradigm_section(BO_PREFIX)
        .context("BO lockfile section is malformed")?;

    add_forbidden_import(&mut section, &args.pattern)
        .with_context(|| format!("add forbidden import `{}`", args.pattern))?;

    let value = serde_json::to_value(&section).context("serialize BO section")?;
    lockfile.paradigms.insert(BO_PREFIX.to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("added forbidden import pattern `{}`", args.pattern);
    println!("updated {}", written.display());
    Ok(())
}
