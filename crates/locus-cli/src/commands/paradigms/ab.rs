use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Subcommand;
use locus_core::Lockfile;
use locus_core::paradigms::abstraction_discipline::{
    AB_PREFIX, edit::add_accepted_single_impl, lockfile_schema::AbSection,
};

// locus: ot boundary cli.ab cli
#[derive(Subcommand, Debug)]
pub enum AbCommand {
    /// Mark a trait pattern as an accepted single-impl trait (AB001).
    AcceptSingleImpl(AbAcceptSingleImplArgs),
}

// locus: ot boundary cli.ab-accept-single-impl cli
#[derive(clap::Args, Debug)]
pub struct AbAcceptSingleImplArgs {
    /// Trait symbol pattern (full path or short name).
    pub pattern: String,
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
}

pub fn run(cmd: AbCommand) -> Result<()> {
    match cmd {
        AbCommand::AcceptSingleImpl(args) => accept_single_impl_cmd(args),
    }
}

fn accept_single_impl_cmd(args: AbAcceptSingleImplArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: AbSection = lockfile
        .paradigm_section(AB_PREFIX)
        .context("AB lockfile section is malformed")?;

    add_accepted_single_impl(&mut section, &args.pattern)
        .with_context(|| format!("accept single-impl trait `{}`", args.pattern))?;

    let value = serde_json::to_value(&section).context("serialize AB section")?;
    lockfile.paradigms.insert(AB_PREFIX.to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("accepted single-impl trait pattern `{}`", args.pattern);
    println!("updated {}", written.display());
    Ok(())
}
