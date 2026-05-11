use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Subcommand;
use locus_core::Lockfile;
use locus_core::paradigms::config_data::{
    CF_PREFIX, edit::add_config_path, lockfile_schema::CfSection,
};

// locus: ot boundary cli.cf cli
#[derive(Subcommand, Debug)]
pub enum CfCommand {
    /// Mark a module pattern as part of the config layer (CF001).
    AddConfigPath(CfAddConfigPathArgs),
}

// locus: ot boundary cli.cf-add-config-path cli
#[derive(clap::Args, Debug)]
pub struct CfAddConfigPathArgs {
    /// Module pattern matching config-owning files.
    pub pattern: String,
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
}

pub fn run(cmd: CfCommand) -> Result<()> {
    match cmd {
        CfCommand::AddConfigPath(args) => add_config_path_cmd(args),
    }
}

fn add_config_path_cmd(args: CfAddConfigPathArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: CfSection = lockfile
        .paradigm_section(CF_PREFIX)
        .context("CF lockfile section is malformed")?;

    add_config_path(&mut section, &args.pattern)
        .with_context(|| format!("add config path `{}`", args.pattern))?;

    let value = serde_json::to_value(&section).context("serialize CF section")?;
    lockfile.paradigms.insert(CF_PREFIX.to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("added config path pattern `{}`", args.pattern);
    println!("updated {}", written.display());
    Ok(())
}
