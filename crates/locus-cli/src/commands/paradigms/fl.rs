use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Subcommand;
use locus_core::Lockfile;
use locus_core::paradigms::failure_lineage::{
    FL_PREFIX,
    edit::{add_boundary_error_pattern, add_domain_path},
    lockfile_schema::FlSection,
};

// locus: ot boundary cli.fl cli
#[derive(Subcommand, Debug)]
pub enum FlCommand {
    /// Mark a module pattern as domain code (FL001).
    AddDomainPath(FlAddDomainPathArgs),
    /// Mark an error-type pattern as a boundary error that must not escape the domain (FL001).
    AddBoundaryError(FlAddBoundaryErrorArgs),
}

// locus: ot boundary cli.fl-add-domain-path cli
#[derive(clap::Args, Debug)]
pub struct FlAddDomainPathArgs {
    /// Module pattern matching domain files.
    pub pattern: String,
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
}

// locus: ot boundary cli.fl-add-boundary-error cli
#[derive(clap::Args, Debug)]
pub struct FlAddBoundaryErrorArgs {
    /// Pattern matching the error type that must not appear in domain signatures.
    pub pattern: String,
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
}

pub fn run(cmd: FlCommand) -> Result<()> {
    match cmd {
        FlCommand::AddDomainPath(args) => add_domain_path_cmd(args),
        FlCommand::AddBoundaryError(args) => add_boundary_error_cmd(args),
    }
}

fn add_domain_path_cmd(args: FlAddDomainPathArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: FlSection = lockfile
        .paradigm_section(FL_PREFIX)
        .context("FL lockfile section is malformed")?;

    add_domain_path(&mut section, &args.pattern)
        .with_context(|| format!("add domain path `{}`", args.pattern))?;

    let value = serde_json::to_value(&section).context("serialize FL section")?;
    lockfile.paradigms.insert(FL_PREFIX.to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("added domain path pattern `{}`", args.pattern);
    println!("updated {}", written.display());
    Ok(())
}

fn add_boundary_error_cmd(args: FlAddBoundaryErrorArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: FlSection = lockfile
        .paradigm_section(FL_PREFIX)
        .context("FL lockfile section is malformed")?;

    add_boundary_error_pattern(&mut section, &args.pattern)
        .with_context(|| format!("add boundary error pattern `{}`", args.pattern))?;

    let value = serde_json::to_value(&section).context("serialize FL section")?;
    lockfile.paradigms.insert(FL_PREFIX.to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("added boundary error pattern `{}`", args.pattern);
    println!("updated {}", written.display());
    Ok(())
}
