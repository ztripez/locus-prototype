use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Subcommand;
use locus_core::Lockfile;
use locus_core::paradigms::dependency_graph::{
    DG_PREFIX,
    edit::{add_shared_path, define_feature, forbid_edge},
    lockfile_schema::DgSection,
};

// locus: ot boundary cli.dg cli
#[derive(Subcommand, Debug)]
pub enum DgCommand {
    /// Forbid imports matching `from` -> `to` patterns.
    ForbidEdge(DgForbidEdgeArgs),
    /// Define a named feature with optional public-API patterns.
    DefineFeature(DgDefineFeatureArgs),
    /// Mark a module pattern as shared infrastructure (DG004).
    AddSharedPath(DgAddSharedPathArgs),
}

// locus: ot boundary cli.dg-define-feature cli
#[derive(clap::Args, Debug)]
pub struct DgDefineFeatureArgs {
    /// Feature name (`billing`, `identity`, …).
    #[arg(long)]
    pub name: String,
    /// Module pattern matching everything that belongs to this feature.
    #[arg(long)]
    pub module: String,
    /// Public-API pattern. Repeat to add more than one.
    #[arg(long)]
    pub public_api: Vec<String>,
    /// Overwrite an existing feature with this name.
    #[arg(long)]
    pub force: bool,
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
}

// locus: ot boundary cli.dg-add-shared-path cli
#[derive(clap::Args, Debug)]
pub struct DgAddSharedPathArgs {
    /// Module pattern matching shared infrastructure.
    pub pattern: String,
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
}

// locus: ot boundary cli.dg-forbid-edge cli
#[derive(clap::Args, Debug)]
pub struct DgForbidEdgeArgs {
    /// Module pattern of the importer, e.g. `lore::domain::*`.
    #[arg(long)]
    pub from: String,
    /// Pattern of the import path the importer must not reach.
    #[arg(long)]
    pub to: String,
    /// Optional reason — surfaced in DG001 diagnostics.
    #[arg(long)]
    pub reason: Option<String>,
    /// Update the reason on an existing edge instead of erroring.
    #[arg(long)]
    pub force: bool,
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
}

pub fn run(cmd: DgCommand) -> Result<()> {
    match cmd {
        DgCommand::ForbidEdge(args) => forbid_edge_cmd(args),
        DgCommand::DefineFeature(args) => define_feature_cmd(args),
        DgCommand::AddSharedPath(args) => add_shared_path_cmd(args),
    }
}

fn define_feature_cmd(args: DgDefineFeatureArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: DgSection = lockfile
        .paradigm_section(DG_PREFIX)
        .context("DG lockfile section is malformed")?;

    define_feature(
        &mut section,
        &args.name,
        &args.module,
        &args.public_api,
        args.force,
    )
    .with_context(|| format!("define feature `{}`", args.name))?;

    let value = serde_json::to_value(&section).context("serialize DG section")?;
    lockfile.paradigms.insert(DG_PREFIX.to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    let api_label = if args.public_api.is_empty() {
        " (no public_api — every cross-feature import will be flagged)".to_string()
    } else {
        format!(" with public_api = [{}]", args.public_api.join(", "))
    };
    println!(
        "defined feature `{}` matching `{}`{api_label}",
        args.name, args.module
    );
    println!("updated {}", written.display());
    Ok(())
}

fn add_shared_path_cmd(args: DgAddSharedPathArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: DgSection = lockfile
        .paradigm_section(DG_PREFIX)
        .context("DG lockfile section is malformed")?;

    add_shared_path(&mut section, &args.pattern)
        .with_context(|| format!("add shared path `{}`", args.pattern))?;

    let value = serde_json::to_value(&section).context("serialize DG section")?;
    lockfile.paradigms.insert(DG_PREFIX.to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("added shared path pattern `{}`", args.pattern);
    println!("updated {}", written.display());
    Ok(())
}

fn forbid_edge_cmd(args: DgForbidEdgeArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: DgSection = lockfile
        .paradigm_section(DG_PREFIX)
        .context("DG lockfile section is malformed")?;

    forbid_edge(
        &mut section,
        &args.from,
        &args.to,
        args.reason.as_deref(),
        args.force,
    )
    .with_context(|| format!("forbid edge {} -> {}", args.from, args.to))?;

    let value = serde_json::to_value(&section).context("serialize DG section")?;
    lockfile.paradigms.insert(DG_PREFIX.to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!(
        "forbade edge `{}` -> `{}`{}",
        args.from,
        args.to,
        args.reason
            .as_deref()
            .map(|r| format!(" (reason: `{r}`)"))
            .unwrap_or_default()
    );
    println!("updated {}", written.display());
    Ok(())
}
