use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Subcommand;
use locus_core::Lockfile;
use locus_core::paradigms::feature_ownership::{
    FO_PREFIX, edit::define_feature as fo_define_feature, lockfile_schema::FoSection,
};

// locus: ot boundary cli.fo cli
#[derive(Subcommand, Debug)]
pub enum FoCommand {
    /// Define a named feature region (FO001).
    DefineFeature(FoDefineFeatureArgs),
}

// locus: ot boundary cli.fo-define-feature cli
#[derive(clap::Args, Debug)]
pub struct FoDefineFeatureArgs {
    /// Feature name.
    #[arg(long)]
    pub name: String,
    /// Module pattern matching everything that belongs to this feature.
    #[arg(long)]
    pub module: String,
    /// Overwrite an existing feature with this name.
    #[arg(long)]
    pub force: bool,
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
}

pub fn run(cmd: FoCommand) -> Result<()> {
    match cmd {
        FoCommand::DefineFeature(args) => define_feature_cmd(args),
    }
}

fn define_feature_cmd(args: FoDefineFeatureArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: FoSection = lockfile
        .paradigm_section(FO_PREFIX)
        .context("FO lockfile section is malformed")?;

    fo_define_feature(&mut section, &args.name, &args.module, args.force)
        .with_context(|| format!("define feature `{}`", args.name))?;

    let value = serde_json::to_value(&section).context("serialize FO section")?;
    lockfile.paradigms.insert(FO_PREFIX.to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("defined feature `{}` matching `{}`", args.name, args.module);
    println!("updated {}", written.display());
    Ok(())
}
