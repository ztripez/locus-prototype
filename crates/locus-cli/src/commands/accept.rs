use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Subcommand;
use locus_core::Lockfile;
use locus_core::paradigms::one_truth::{
    OT_PREFIX,
    accept::{accept_boundary, accept_canonical, accept_converter},
    lockfile_schema::OtSection,
};

// locus: ot boundary cli.accept cli
#[derive(Subcommand, Debug)]
pub enum AcceptCommand {
    /// Accept a symbol as canonical for a concept.
    Canonical(AcceptCanonicalArgs),
    /// Accept a symbol as a boundary adapter for an existing concept.
    Boundary(AcceptBoundaryArgs),
    /// Accept a converter symbol for an existing concept.
    Converter(AcceptConverterArgs),
}

// locus: ot boundary cli.accept-canonical cli
#[derive(clap::Args, Debug)]
pub struct AcceptCanonicalArgs {
    /// Fully-qualified symbol of the canonical type, e.g. `crate::domain::User`.
    pub symbol: String,
    /// Concept id to bind to. Defaults to the symbol's name stem.
    #[arg(long)]
    pub concept: Option<String>,
    /// Replace an existing canonical for the concept.
    #[arg(long)]
    pub force: bool,
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
}

// locus: ot boundary cli.accept-boundary cli
#[derive(clap::Args, Debug)]
pub struct AcceptBoundaryArgs {
    /// Fully-qualified symbol of the boundary type, e.g. `crate::api::UserDto`.
    pub symbol: String,
    /// Concept id this boundary belongs to. Required.
    #[arg(long)]
    pub concept: String,
    /// Boundary label, e.g. `api.v1`, `persistence`, `proto`.
    #[arg(long)]
    pub boundary: Option<String>,
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
}

// locus: ot boundary cli.accept-converter cli
#[derive(clap::Args, Debug)]
pub struct AcceptConverterArgs {
    /// The converter symbol — e.g. `"impl TryFrom<UserDto> for User"` or a free fn path.
    pub symbol: String,
    /// Concept id the converter belongs to.
    #[arg(long)]
    pub concept: String,
    /// Optional source-side symbol hint.
    #[arg(long)]
    pub from: Option<String>,
    /// Optional target-side symbol hint.
    #[arg(long)]
    pub to: Option<String>,
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
}

pub fn run(cmd: AcceptCommand) -> Result<()> {
    let workspace = match &cmd {
        AcceptCommand::Canonical(a) => a.workspace.clone(),
        AcceptCommand::Boundary(a) => a.workspace.clone(),
        AcceptCommand::Converter(a) => a.workspace.clone(),
    };
    let air = locus_rust::scan(&workspace)
        .with_context(|| format!("scan failed: {}", workspace.display()))?;
    let mut lockfile = Lockfile::load_or_empty(&workspace)
        .with_context(|| format!("load lockfile from {}", workspace.display()))?;

    let mut section: OtSection = lockfile
        .paradigm_section(OT_PREFIX)
        .context("OT lockfile section is malformed")?;

    let summary = apply_command(cmd, &mut section, &air)?;

    let value = serde_json::to_value(&section).context("serialize OT section")?;
    lockfile.paradigms.insert(OT_PREFIX.to_string(), value);
    let written = lockfile
        .save(&workspace)
        .with_context(|| format!("write lockfile to {}", workspace.display()))?;

    println!("{summary}");
    println!("updated {}", written.display());
    Ok(())
}

fn apply_command(
    cmd: AcceptCommand,
    section: &mut OtSection,
    air: &locus_air::AirWorkspace,
) -> Result<String> {
    match cmd {
        AcceptCommand::Canonical(a) => {
            let cid = accept_canonical(section, air, &a.symbol, a.concept.as_deref(), a.force)
                .with_context(|| format!("accept canonical `{}`", a.symbol))?;
            Ok(format!(
                "accepted `{}` as canonical for concept `{cid}`",
                a.symbol
            ))
        }
        AcceptCommand::Boundary(a) => {
            accept_boundary(section, air, &a.symbol, &a.concept, a.boundary.as_deref())
                .with_context(|| format!("accept boundary `{}`", a.symbol))?;
            Ok(format!(
                "accepted `{}` as boundary for concept `{}`{}",
                a.symbol,
                a.concept,
                a.boundary
                    .as_deref()
                    .map(|b| format!(" (label `{b}`)"))
                    .unwrap_or_default()
            ))
        }
        AcceptCommand::Converter(a) => {
            accept_converter(
                section,
                air,
                &a.symbol,
                &a.concept,
                a.from.as_deref(),
                a.to.as_deref(),
            )
            .with_context(|| format!("accept converter `{}`", a.symbol))?;
            Ok(format!(
                "accepted `{}` as converter for concept `{}`",
                a.symbol, a.concept
            ))
        }
    }
}
