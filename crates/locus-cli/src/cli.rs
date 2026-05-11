// locus: ot boundary cli.invocation cli
use clap::{Parser, Subcommand};

use crate::commands::{
    accept::AcceptCommand,
    check::CheckArgs,
    debt::DebtArgs,
    emit_air::EmitAirArgs,
    explain::ExplainArgs,
    init::InitArgs,
    paradigms::{
        ab::AbCommand, bo::BoCommand, cf::CfCommand, cr::CrCommand, cx::CxCommand, da::DaCommand,
        dc::DcCommand, dg::DgCommand, er::ErCommand, fl::FlCommand, fo::FoCommand, mo::MoCommand,
        ob::ObCommand, pa::PaCommand, rm::RmCommand, rw::RwCommand, ta::TaCommand, ut::UtCommand,
    },
    prune::PruneArgs,
};

#[derive(Parser, Debug)]
#[command(name = "locus", version, about = "Locus — architecture verifier")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

// locus: ot boundary cli.command cli
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Scan a Rust workspace and emit AIR JSON.
    EmitAir(EmitAirArgs),
    /// Build `locus.lock` from a fresh workspace scan.
    Init(InitArgs),
    /// Run all enabled paradigms against a workspace and report diagnostics.
    Check(CheckArgs),
    /// Record a symbol's accepted ownership in `locus.lock` (OT paradigm).
    #[command(subcommand)]
    Accept(AcceptCommand),
    /// Manage AB (Abstraction Discipline) declarations in `locus.lock`.
    #[command(subcommand)]
    Ab(AbCommand),
    /// Manage BO (Boundary Ownership) declarations in `locus.lock`.
    #[command(subcommand)]
    Bo(BoCommand),
    /// Manage CF (Config/Data Ownership) declarations in `locus.lock`.
    #[command(subcommand)]
    Cf(CfCommand),
    /// Manage CR (Composition Root Ownership) declarations in `locus.lock`.
    #[command(subcommand)]
    Cr(CrCommand),
    /// Manage CX (Complexity Budget) declarations in `locus.lock`.
    #[command(subcommand)]
    Cx(CxCommand),
    /// Manage DA (Demand-Driven Architecture) declarations in `locus.lock`.
    #[command(subcommand)]
    Da(DaCommand),
    /// Manage DC (Documentation / Comment Ownership) declarations in `locus.lock`.
    #[command(subcommand)]
    Dc(DcCommand),
    /// Manage DG (Dependency Graph) declarations in `locus.lock`.
    #[command(subcommand)]
    Dg(DgCommand),
    /// Manage ER (Error Taxonomy) declarations in `locus.lock`.
    #[command(subcommand)]
    Er(ErCommand),
    /// Manage FL (Failure Lineage) declarations in `locus.lock`.
    #[command(subcommand)]
    Fl(FlCommand),
    /// Manage FO (Feature Ownership) declarations in `locus.lock`.
    #[command(subcommand)]
    Fo(FoCommand),
    /// Manage MO (Module / File Ownership) declarations in `locus.lock`.
    #[command(subcommand)]
    Mo(MoCommand),
    /// Manage OB (Observability Ownership) declarations in `locus.lock`.
    #[command(subcommand)]
    Ob(ObCommand),
    /// Manage PA (Port/Adapter Ownership) declarations in `locus.lock`.
    #[command(subcommand)]
    Pa(PaCommand),
    /// Manage RM (Responsibility Mixing) declarations in `locus.lock`.
    #[command(subcommand)]
    Rm(RmCommand),
    /// Manage RW (Runtime Work Ownership) declarations in `locus.lock`.
    #[command(subcommand)]
    Rw(RwCommand),
    /// Manage TA (Test Architecture Ownership) declarations in `locus.lock`.
    #[command(subcommand)]
    Ta(TaCommand),
    /// Manage UT (Utility / Shared Module Discipline) declarations in `locus.lock`.
    #[command(subcommand)]
    Ut(UtCommand),
    /// List active and expired exceptions across `// locus: allow` hints and
    /// `Lockfile.exceptions`. Inventory of every suppression in the repo.
    Debt(DebtArgs),
    /// Print the rule-spec section for a given rule id (e.g. `OT004`).
    Explain(ExplainArgs),
    /// Remove expired lockfile exceptions from `locus.lock`.
    Prune(PruneArgs),
}
