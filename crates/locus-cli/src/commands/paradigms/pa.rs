use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Subcommand;
use locus_core::Lockfile;
use locus_core::paradigms::port_adapter::{
    PA_PREFIX,
    edit::{add_accepted_colocated as pa_add_accepted_colocated, add_application_path},
    lockfile_schema::PaSection,
};

// locus: ot boundary cli.pa cli
#[derive(Subcommand, Debug)]
pub enum PaCommand {
    /// Mark a trait pattern as an accepted co-located trait (PA001).
    AcceptColocated(PaAcceptColocatedArgs),
    /// Add a module pattern declaring the application layer (PA002).
    AddApplicationPath(PaAddApplicationPathArgs),
}

// locus: ot boundary cli.pa-accept-colocated cli
#[derive(clap::Args, Debug)]
pub struct PaAcceptColocatedArgs {
    /// Trait symbol pattern (full path or short name).
    pub pattern: String,
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
}

// locus: ot boundary cli.pa-add-application-path cli
#[derive(clap::Args, Debug)]
pub struct PaAddApplicationPathArgs {
    /// Module path glob, e.g. `"crate::application::*"`.
    pub pattern: String,
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
}

pub fn run(cmd: PaCommand) -> Result<()> {
    match cmd {
        PaCommand::AcceptColocated(args) => accept_colocated_cmd(args),
        PaCommand::AddApplicationPath(args) => add_application_path_cmd(args),
    }
}

fn accept_colocated_cmd(args: PaAcceptColocatedArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: PaSection = lockfile
        .paradigm_section(PA_PREFIX)
        .context("PA lockfile section is malformed")?;

    pa_add_accepted_colocated(&mut section, &args.pattern)
        .with_context(|| format!("accept co-located trait `{}`", args.pattern))?;

    let value = serde_json::to_value(&section).context("serialize PA section")?;
    lockfile.paradigms.insert(PA_PREFIX.to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("accepted co-located trait pattern `{}`", args.pattern);
    println!("updated {}", written.display());
    Ok(())
}

fn add_application_path_cmd(args: PaAddApplicationPathArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: PaSection = lockfile
        .paradigm_section(PA_PREFIX)
        .context("PA lockfile section is malformed")?;

    add_application_path(&mut section, &args.pattern)
        .with_context(|| format!("add PA application path `{}`", args.pattern))?;

    let value = serde_json::to_value(&section).context("serialize PA section")?;
    lockfile.paradigms.insert(PA_PREFIX.to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("added PA application path pattern `{}`", args.pattern);
    println!("updated {}", written.display());
    Ok(())
}
