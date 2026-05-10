mod diff;

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use locus_core::paradigms::abstraction_discipline::{
    AB_PREFIX, edit::add_accepted_single_impl as ab_add_accepted_single_impl,
    lockfile_schema::AbSection,
};
use locus_core::paradigms::boundary_ownership::{
    BO_PREFIX,
    edit::{
        add_domain_path as bo_add_domain_path, add_forbidden_import as bo_add_forbidden_import,
    },
    lockfile_schema::BoSection,
};
use locus_core::paradigms::complexity_budget::{
    CX_PREFIX,
    edit::{add_override as cx_add_override, set_default_max_lines as cx_set_default_max_lines},
    lockfile_schema::CxSection,
};
use locus_core::paradigms::composition_root::{
    CR_PREFIX, edit::add_composition_root as cr_add_composition_root, lockfile_schema::CrSection,
};
use locus_core::paradigms::config_data::{
    CF_PREFIX, edit::add_config_path as cf_add_config_path, lockfile_schema::CfSection,
};
use locus_core::paradigms::demand_driven::{
    DA_PREFIX,
    edit::{
        add_accepted_single_impl as da_add_accepted_single_impl, set_enabled as da_set_enabled,
    },
    lockfile_schema::DaSection,
};
use locus_core::paradigms::dependency_graph::{
    DG_PREFIX,
    edit::{add_shared_path, define_feature, forbid_edge},
    lockfile_schema::DgSection,
};
use locus_core::paradigms::documentation::{
    DC_PREFIX,
    edit::{add_exempt_path as dc_add_exempt_path, set_require_public_docs as dc_set_require},
    lockfile_schema::DcSection,
};
use locus_core::paradigms::failure_lineage::{
    FL_PREFIX,
    edit::{
        add_boundary_error_pattern as fl_add_boundary_error_pattern,
        add_domain_path as fl_add_domain_path,
    },
    lockfile_schema::FlSection,
};
use locus_core::paradigms::feature_ownership::{
    FO_PREFIX, edit::define_feature as fo_define_feature, lockfile_schema::FoSection,
};
use locus_core::paradigms::module_ownership::{
    MO_PREFIX,
    edit::{
        add_override as mo_add_override,
        set_default_max_public_types as mo_set_default_max_public_types,
    },
    lockfile_schema::MoSection,
};
use locus_core::paradigms::observability::{
    OB_PREFIX,
    edit::{
        add_forbidden_log_target as ob_add_forbidden_log_target,
        add_observer_path as ob_add_observer_path,
    },
    lockfile_schema::ObSection,
};
use locus_core::paradigms::one_truth::{
    OT_PREFIX,
    accept::{accept_boundary, accept_canonical, accept_converter},
    lockfile_schema::OtSection,
};
use locus_core::paradigms::port_adapter::{
    PA_PREFIX, edit::add_accepted_colocated as pa_add_accepted_colocated,
    lockfile_schema::PaSection,
};
use locus_core::paradigms::responsibility::{
    RM_PREFIX,
    edit::{
        add_exempt_path as rm_add_exempt_path,
        set_default_max_action_kinds as rm_set_default_max_action_kinds,
    },
    lockfile_schema::RmSection,
};
use locus_core::paradigms::test_architecture::{
    TA_PREFIX, edit::add_test_path as ta_add_test_path, lockfile_schema::TaSection,
};
use locus_core::paradigms::utility_discipline::{
    UT_PREFIX, edit::add_utility_path, lockfile_schema::UtSection,
};
use locus_core::{
    CheckMode, Diagnostic, Lockfile, Severity, apply_exceptions, registry, today_utc,
};

// locus: ot boundary cli.invocation cli
#[derive(Parser, Debug)]
#[command(name = "locus", version, about = "Locus — architecture verifier")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

// locus: ot boundary cli.command cli
#[derive(Subcommand, Debug)]
enum Command {
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

// locus: ot boundary cli.ut cli
#[derive(Subcommand, Debug)]
enum UtCommand {
    /// Mark a module pattern as a utility module (UT001).
    AddUtilityPath(UtAddUtilityPathArgs),
}

// locus: ot boundary cli.ut-add-utility-path cli
#[derive(clap::Args, Debug)]
struct UtAddUtilityPathArgs {
    /// Module pattern matching utility modules.
    pattern: String,
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}

// locus: ot boundary cli.ab cli
#[derive(Subcommand, Debug)]
enum AbCommand {
    /// Mark a trait pattern as an accepted single-impl trait (AB001).
    AcceptSingleImpl(AbAcceptSingleImplArgs),
}

// locus: ot boundary cli.ab-accept-single-impl cli
#[derive(clap::Args, Debug)]
struct AbAcceptSingleImplArgs {
    /// Trait symbol pattern (full path or short name).
    pattern: String,
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}

// locus: ot boundary cli.bo cli
#[derive(Subcommand, Debug)]
enum BoCommand {
    /// Mark a module pattern as domain/application code (BO001).
    AddDomainPath(BoAddDomainPathArgs),
    /// Mark an import-path pattern as forbidden inside the domain layer (BO001).
    AddForbiddenImport(BoAddForbiddenImportArgs),
}

// locus: ot boundary cli.bo-add-domain-path cli
#[derive(clap::Args, Debug)]
struct BoAddDomainPathArgs {
    /// Module pattern matching domain/application files.
    pattern: String,
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}

// locus: ot boundary cli.bo-add-forbidden-import cli
#[derive(clap::Args, Debug)]
struct BoAddForbiddenImportArgs {
    /// Import-path pattern that domain code must not reach.
    pattern: String,
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}

// locus: ot boundary cli.cf cli
#[derive(Subcommand, Debug)]
enum CfCommand {
    /// Mark a module pattern as part of the config layer (CF001).
    AddConfigPath(CfAddConfigPathArgs),
}

// locus: ot boundary cli.cf-add-config-path cli
#[derive(clap::Args, Debug)]
struct CfAddConfigPathArgs {
    /// Module pattern matching config-owning files.
    pattern: String,
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}

// locus: ot boundary cli.cr cli
#[derive(Subcommand, Debug)]
enum CrCommand {
    /// Declare a module pattern as a composition root (CR001).
    AddCompositionRoot(CrAddCompositionRootArgs),
}

// locus: ot boundary cli.cr-add-composition-root cli
#[derive(clap::Args, Debug)]
struct CrAddCompositionRootArgs {
    /// Module pattern matching composition-root files.
    pattern: String,
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}

// locus: ot boundary cli.cx cli
#[derive(Subcommand, Debug)]
enum CxCommand {
    /// Set the workspace-wide function-line budget (CX001).
    SetDefault(CxSetDefaultArgs),
    /// Add a per-module function-line override (CX001).
    AddOverride(CxAddOverrideArgs),
}

// locus: ot boundary cli.cx-set-default cli
#[derive(clap::Args, Debug)]
struct CxSetDefaultArgs {
    /// Maximum number of lines a single function may span.
    #[arg(long)]
    max_lines: u32,
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}

// locus: ot boundary cli.cx-add-override cli
#[derive(clap::Args, Debug)]
struct CxAddOverrideArgs {
    /// Module pattern this override applies to.
    #[arg(long)]
    module: String,
    /// Override budget in lines.
    #[arg(long)]
    max_lines: u32,
    /// Update the budget on an existing override instead of erroring.
    #[arg(long)]
    force: bool,
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}

// locus: ot boundary cli.da cli
#[derive(Subcommand, Debug)]
enum DaCommand {
    /// Enable DA paradigm checks.
    Enable(DaToggleArgs),
    /// Disable DA paradigm checks.
    Disable(DaToggleArgs),
    /// Mark a trait pattern as an accepted single-impl abstraction (DA001).
    AcceptSingleImpl(DaAcceptSingleImplArgs),
}

// locus: ot boundary cli.da-toggle cli
#[derive(clap::Args, Debug)]
struct DaToggleArgs {
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}

// locus: ot boundary cli.da-accept-single-impl cli
#[derive(clap::Args, Debug)]
struct DaAcceptSingleImplArgs {
    /// Trait symbol pattern (full path or short name).
    pattern: String,
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}

// locus: ot boundary cli.dc cli
#[derive(Subcommand, Debug)]
enum DcCommand {
    /// Turn DC001's "public API must be documented" check on.
    Enable(DcToggleArgs),
    /// Turn DC001's "public API must be documented" check off.
    Disable(DcToggleArgs),
    /// Add a module pattern exempt from the public-doc requirement (DC001).
    AddExemptPath(DcAddExemptPathArgs),
}

// locus: ot boundary cli.dc-toggle cli
#[derive(clap::Args, Debug)]
struct DcToggleArgs {
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}

// locus: ot boundary cli.dc-add-exempt-path cli
#[derive(clap::Args, Debug)]
struct DcAddExemptPathArgs {
    /// Module pattern exempt from DC001.
    pattern: String,
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}

// locus: ot boundary cli.dg cli
#[derive(Subcommand, Debug)]
enum DgCommand {
    /// Forbid imports matching `from` -> `to` patterns.
    ForbidEdge(DgForbidEdgeArgs),
    /// Define a named feature with optional public-API patterns.
    DefineFeature(DgDefineFeatureArgs),
    /// Mark a module pattern as shared infrastructure (DG004).
    AddSharedPath(DgAddSharedPathArgs),
}

// locus: ot boundary cli.dg-define-feature cli
#[derive(clap::Args, Debug)]
struct DgDefineFeatureArgs {
    /// Feature name (`billing`, `identity`, …).
    #[arg(long)]
    name: String,
    /// Module pattern matching everything that belongs to this feature.
    #[arg(long)]
    module: String,
    /// Public-API pattern. Repeat to add more than one.
    #[arg(long)]
    public_api: Vec<String>,
    /// Overwrite an existing feature with this name.
    #[arg(long)]
    force: bool,
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}

// locus: ot boundary cli.dg-add-shared-path cli
#[derive(clap::Args, Debug)]
struct DgAddSharedPathArgs {
    /// Module pattern matching shared infrastructure.
    pattern: String,
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}

// locus: ot boundary cli.dg-forbid-edge cli
#[derive(clap::Args, Debug)]
struct DgForbidEdgeArgs {
    /// Module pattern of the importer, e.g. `lore::domain::*`.
    #[arg(long)]
    from: String,
    /// Pattern of the import path the importer must not reach.
    #[arg(long)]
    to: String,
    /// Optional reason — surfaced in DG001 diagnostics.
    #[arg(long)]
    reason: Option<String>,
    /// Update the reason on an existing edge instead of erroring.
    #[arg(long)]
    force: bool,
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}

// locus: ot boundary cli.accept cli
#[derive(Subcommand, Debug)]
enum AcceptCommand {
    /// Accept a symbol as canonical for a concept.
    Canonical(AcceptCanonicalArgs),
    /// Accept a symbol as a boundary adapter for an existing concept.
    Boundary(AcceptBoundaryArgs),
    /// Accept a converter symbol for an existing concept.
    Converter(AcceptConverterArgs),
}

// locus: ot boundary cli.accept-canonical cli
#[derive(clap::Args, Debug)]
struct AcceptCanonicalArgs {
    /// Fully-qualified symbol of the canonical type, e.g. `crate::domain::User`.
    symbol: String,
    /// Concept id to bind to. Defaults to the symbol's name stem.
    #[arg(long)]
    concept: Option<String>,
    /// Replace an existing canonical for the concept.
    #[arg(long)]
    force: bool,
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}

// locus: ot boundary cli.accept-boundary cli
#[derive(clap::Args, Debug)]
struct AcceptBoundaryArgs {
    /// Fully-qualified symbol of the boundary type, e.g. `crate::api::UserDto`.
    symbol: String,
    /// Concept id this boundary belongs to. Required.
    #[arg(long)]
    concept: String,
    /// Boundary label, e.g. `api.v1`, `persistence`, `proto`.
    #[arg(long)]
    boundary: Option<String>,
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}

// locus: ot boundary cli.accept-converter cli
#[derive(clap::Args, Debug)]
struct AcceptConverterArgs {
    /// The converter symbol — e.g. `"impl TryFrom<UserDto> for User"` or a free fn path.
    symbol: String,
    /// Concept id the converter belongs to.
    #[arg(long)]
    concept: String,
    /// Optional source-side symbol hint.
    #[arg(long)]
    from: Option<String>,
    /// Optional target-side symbol hint.
    #[arg(long)]
    to: Option<String>,
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}

// locus: ot boundary cli.er cli
#[derive(Subcommand, Debug)]
enum ErCommand {
    /// Mark a module pattern as part of the domain layer (ER003).
    AddDomainPath(ErAddDomainPathArgs),
}

// locus: ot boundary cli.er-add-domain-path cli
#[derive(clap::Args, Debug)]
struct ErAddDomainPathArgs {
    /// Module path glob, e.g. `"crate::domain::*"`.
    pattern: String,
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}

// locus: ot boundary cli.fl cli
#[derive(Subcommand, Debug)]
enum FlCommand {
    /// Mark a module pattern as domain code (FL001).
    AddDomainPath(FlAddDomainPathArgs),
    /// Mark an error-type pattern as a boundary error that must not escape the domain (FL001).
    AddBoundaryError(FlAddBoundaryErrorArgs),
}

// locus: ot boundary cli.fl-add-domain-path cli
#[derive(clap::Args, Debug)]
struct FlAddDomainPathArgs {
    /// Module pattern matching domain files.
    pattern: String,
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}

// locus: ot boundary cli.fl-add-boundary-error cli
#[derive(clap::Args, Debug)]
struct FlAddBoundaryErrorArgs {
    /// Pattern matching the error type that must not appear in domain signatures.
    pattern: String,
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}

// locus: ot boundary cli.fo cli
#[derive(Subcommand, Debug)]
enum FoCommand {
    /// Define a named feature region (FO001).
    DefineFeature(FoDefineFeatureArgs),
}

// locus: ot boundary cli.fo-define-feature cli
#[derive(clap::Args, Debug)]
struct FoDefineFeatureArgs {
    /// Feature name.
    #[arg(long)]
    name: String,
    /// Module pattern matching everything that belongs to this feature.
    #[arg(long)]
    module: String,
    /// Overwrite an existing feature with this name.
    #[arg(long)]
    force: bool,
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}

// locus: ot boundary cli.mo cli
#[derive(Subcommand, Debug)]
enum MoCommand {
    /// Set the workspace-wide public-types-per-file budget (MO001).
    SetDefault(MoSetDefaultArgs),
    /// Add a per-module public-types budget override (MO001).
    AddOverride(MoAddOverrideArgs),
}

// locus: ot boundary cli.mo-set-default cli
#[derive(clap::Args, Debug)]
struct MoSetDefaultArgs {
    /// Maximum number of `pub` top-level types per file.
    #[arg(long)]
    max_types: u32,
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}

// locus: ot boundary cli.mo-add-override cli
#[derive(clap::Args, Debug)]
struct MoAddOverrideArgs {
    /// Module pattern this override applies to.
    #[arg(long)]
    module: String,
    /// Override budget in number of public types.
    #[arg(long)]
    max_types: u32,
    /// Update the budget on an existing override instead of erroring.
    #[arg(long)]
    force: bool,
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}

// locus: ot boundary cli.ob cli
#[derive(Subcommand, Debug)]
enum ObCommand {
    /// Declare a module pattern as a legitimate observer (OB001).
    AddObserverPath(ObAddObserverPathArgs),
    /// Add a macro pattern to the forbidden log targets list (OB001).
    AddForbiddenLogTarget(ObAddForbiddenLogTargetArgs),
}

// locus: ot boundary cli.ob-add-observer-path cli
#[derive(clap::Args, Debug)]
struct ObAddObserverPathArgs {
    /// Module pattern matching observer files.
    pattern: String,
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}

// locus: ot boundary cli.ob-add-forbidden-log-target cli
#[derive(clap::Args, Debug)]
struct ObAddForbiddenLogTargetArgs {
    /// Macro path pattern considered raw/inappropriate.
    pattern: String,
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}

// locus: ot boundary cli.pa cli
#[derive(Subcommand, Debug)]
enum PaCommand {
    /// Mark a trait pattern as an accepted co-located trait (PA001).
    AcceptColocated(PaAcceptColocatedArgs),
    /// Add a module pattern declaring the application layer (PA002).
    AddApplicationPath(PaAddApplicationPathArgs),
}

// locus: ot boundary cli.pa-accept-colocated cli
#[derive(clap::Args, Debug)]
struct PaAcceptColocatedArgs {
    /// Trait symbol pattern (full path or short name).
    pattern: String,
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}

// locus: ot boundary cli.pa-add-application-path cli
#[derive(clap::Args, Debug)]
struct PaAddApplicationPathArgs {
    /// Module path glob, e.g. `"crate::application::*"`.
    pattern: String,
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}

// locus: ot boundary cli.rm cli
#[derive(Subcommand, Debug)]
enum RmCommand {
    /// Set the workspace-wide per-function action-kind cap (RM001).
    SetDefault(RmSetDefaultArgs),
    /// Add a module pattern exempt from RM checks.
    AddExemptPath(RmAddExemptPathArgs),
    /// Add a module pattern declaring the domain layer (RM006).
    AddDomainPath(RmAddDomainPathArgs),
}

// locus: ot boundary cli.rm-set-default cli
#[derive(clap::Args, Debug)]
struct RmSetDefaultArgs {
    /// Maximum number of distinct action kinds a single function may produce.
    #[arg(long)]
    max_kinds: u32,
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}

// locus: ot boundary cli.rm-add-exempt-path cli
#[derive(clap::Args, Debug)]
struct RmAddExemptPathArgs {
    /// Module pattern exempt from RM checks.
    pattern: String,
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}

// locus: ot boundary cli.rm-add-domain-path cli
#[derive(clap::Args, Debug)]
struct RmAddDomainPathArgs {
    /// Module path glob, e.g. `"crate::domain::*"`.
    pattern: String,
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}

// locus: ot boundary cli.rw cli
#[derive(Subcommand, Debug)]
enum RwCommand {
    /// Mark a module pattern as a runtime owner (RW001).
    AcceptRuntimeOwner(RwAcceptRuntimeOwnerArgs),
}

// locus: ot boundary cli.rw-accept-runtime-owner cli
#[derive(clap::Args, Debug)]
struct RwAcceptRuntimeOwnerArgs {
    /// Module path glob.
    pattern: String,
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}

// locus: ot boundary cli.ta cli
#[derive(Subcommand, Debug)]
enum TaCommand {
    /// Mark a module pattern as test code (TA001).
    AddTestPath(TaAddTestPathArgs),
}

// locus: ot boundary cli.ta-add-test-path cli
#[derive(clap::Args, Debug)]
struct TaAddTestPathArgs {
    /// Module pattern matching test files.
    pattern: String,
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}

// locus: ot boundary cli.init cli
#[derive(clap::Args, Debug)]
struct InitArgs {
    /// Workspace root (containing Cargo.toml).
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
    /// Refuse to overwrite an existing locus.lock.
    #[arg(long)]
    no_overwrite: bool,
    /// Comma-separated paradigm prefixes the user explicitly acknowledges
    /// as empty. Each prefix is appended to `Lockfile.acknowledged_empty`
    /// (silencing LOCUS002 for that paradigm). Already-present prefixes
    /// are silently deduped. Example: `--acknowledge-empty RW,DA`.
    #[arg(long, value_name = "PREFIXES")]
    acknowledge_empty: Option<String>,
}

// locus: ot boundary cli.emit-air cli
#[derive(clap::Args, Debug)]
struct EmitAirArgs {
    /// Workspace root (containing Cargo.toml).
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
    /// Output file. Defaults to stdout.
    #[arg(long)]
    output: Option<PathBuf>,
    /// Pretty-print JSON.
    #[arg(long)]
    pretty: bool,
}

// locus: ot boundary cli.debt cli
#[derive(clap::Args, Debug)]
struct DebtArgs {
    /// Workspace root (containing Cargo.toml).
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
    /// Emit one JSON object per line instead of human-readable text.
    #[arg(long)]
    json: bool,
    /// Group output by rule id so hotspot rules are obvious.
    #[arg(long)]
    by_rule: bool,
}

// locus: ot boundary cli.explain cli
#[derive(clap::Args, Debug)]
struct ExplainArgs {
    /// Rule id to explain, e.g. `OT004`.
    rule_id: String,
    /// Workspace root (containing docs/PARADIGMS.md).
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}

// locus: ot boundary cli.prune cli
#[derive(clap::Args, Debug)]
struct PruneArgs {
    /// Workspace root (containing Cargo.toml).
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}

// locus: ot boundary cli.check cli
#[derive(clap::Args, Debug)]
struct CheckArgs {
    /// Workspace root (containing Cargo.toml).
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
    /// Treat warnings as fatal. Use this for LLM-generated patches.
    #[arg(long)]
    agent_strict: bool,
    /// Emit diagnostics as JSON instead of human-readable text.
    #[arg(long)]
    json: bool,
    /// Filter diagnostics to files modified since the baseline ref.
    /// Combines tracked changes between baseline and HEAD, working-tree
    /// changes, and untracked-but-not-ignored files. Useful in CI to
    /// fail only on PR-introduced violations, not legacy noise.
    #[arg(long)]
    changed: bool,
    /// Baseline ref for `--changed`. Defaults to the first ref that
    /// resolves from `origin/main`, `origin/master`, `main`, `master`,
    /// `HEAD~1`. Pass an explicit ref (e.g. `--baseline origin/develop`)
    /// to override. Also used by Policy Guard (`PG001`-`PG004`) to read
    /// the baseline `locus.lock`.
    #[arg(long)]
    baseline: Option<String>,
    /// Acknowledge that this run is calibrating policy (raising budgets,
    /// adding overrides, expanding `acknowledged_empty`, or widening
    /// `OT.converter_paths`). Without this flag, Policy Guard fails
    /// `--agent-strict` on any policy widening vs the baseline lockfile.
    /// With it, PG001/PG002/PG003/PG004/PG008 fire as Advisory and a
    /// structured calibration report is printed alongside the normal
    /// output. PG006 (missing debt metadata) is **not** affected by
    /// calibration — calibration legitimizes the addition itself, but
    /// does not waive the requirement to record `reason` / `expires` /
    /// `owner`. See issue #44.
    #[arg(long)]
    allow_policy_calibration: bool,
    /// Acknowledge that no baseline lockfile is available for the
    /// Policy Guard audit (e.g. shallow CI clone, first commit before
    /// `locus.lock` existed). Without this flag, PG000 fires Fatal
    /// under `--agent-strict` so that a missing audit can't silently
    /// disable the gate. See issue #44.
    #[arg(long)]
    allow_missing_policy_baseline: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::EmitAir(args) => emit_air(args),
        Command::Init(args) => init(args),
        Command::Check(args) => check(args),
        Command::Accept(cmd) => accept(cmd),
        Command::Ab(cmd) => ab(cmd),
        Command::Bo(cmd) => bo(cmd),
        Command::Cf(cmd) => cf(cmd),
        Command::Cr(cmd) => cr(cmd),
        Command::Cx(cmd) => cx(cmd),
        Command::Da(cmd) => da(cmd),
        Command::Dc(cmd) => dc(cmd),
        Command::Dg(cmd) => dg(cmd),
        Command::Er(cmd) => er(cmd),
        Command::Fl(cmd) => fl(cmd),
        Command::Fo(cmd) => fo(cmd),
        Command::Mo(cmd) => mo(cmd),
        Command::Ob(cmd) => ob(cmd),
        Command::Pa(cmd) => pa(cmd),
        Command::Rm(cmd) => rm(cmd),
        Command::Rw(cmd) => rw(cmd),
        Command::Ta(cmd) => ta(cmd),
        Command::Ut(cmd) => ut(cmd),
        Command::Debt(args) => debt(args),
        Command::Explain(args) => explain(args),
        Command::Prune(args) => prune(args),
    }
}

fn ut(cmd: UtCommand) -> Result<()> {
    match cmd {
        UtCommand::AddUtilityPath(args) => ut_add_utility_path(args),
    }
}

fn ut_add_utility_path(args: UtAddUtilityPathArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: UtSection = lockfile
        .paradigm_section(UT_PREFIX)
        .context("UT lockfile section is malformed")?;

    add_utility_path(&mut section, &args.pattern)
        .with_context(|| format!("add utility path `{}`", args.pattern))?;

    let value = serde_json::to_value(&section).context("serialize UT section")?;
    lockfile.paradigms.insert(UT_PREFIX.to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("added utility path pattern `{}`", args.pattern);
    println!("updated {}", written.display());
    Ok(())
}

fn ab(cmd: AbCommand) -> Result<()> {
    match cmd {
        AbCommand::AcceptSingleImpl(args) => ab_accept_single_impl(args),
    }
}

fn ab_accept_single_impl(args: AbAcceptSingleImplArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: AbSection = lockfile
        .paradigm_section(AB_PREFIX)
        .context("AB lockfile section is malformed")?;

    ab_add_accepted_single_impl(&mut section, &args.pattern)
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

fn bo(cmd: BoCommand) -> Result<()> {
    match cmd {
        BoCommand::AddDomainPath(args) => bo_add_domain_path_cli(args),
        BoCommand::AddForbiddenImport(args) => bo_add_forbidden_import_cli(args),
    }
}

fn bo_add_domain_path_cli(args: BoAddDomainPathArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: BoSection = lockfile
        .paradigm_section(BO_PREFIX)
        .context("BO lockfile section is malformed")?;

    bo_add_domain_path(&mut section, &args.pattern)
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

fn bo_add_forbidden_import_cli(args: BoAddForbiddenImportArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: BoSection = lockfile
        .paradigm_section(BO_PREFIX)
        .context("BO lockfile section is malformed")?;

    bo_add_forbidden_import(&mut section, &args.pattern)
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

fn cf(cmd: CfCommand) -> Result<()> {
    match cmd {
        CfCommand::AddConfigPath(args) => cf_add_config_path_cli(args),
    }
}

fn cf_add_config_path_cli(args: CfAddConfigPathArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: CfSection = lockfile
        .paradigm_section(CF_PREFIX)
        .context("CF lockfile section is malformed")?;

    cf_add_config_path(&mut section, &args.pattern)
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

fn cr(cmd: CrCommand) -> Result<()> {
    match cmd {
        CrCommand::AddCompositionRoot(args) => cr_add_composition_root_cli(args),
    }
}

fn cr_add_composition_root_cli(args: CrAddCompositionRootArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: CrSection = lockfile
        .paradigm_section(CR_PREFIX)
        .context("CR lockfile section is malformed")?;

    cr_add_composition_root(&mut section, &args.pattern)
        .with_context(|| format!("add composition root `{}`", args.pattern))?;

    let value = serde_json::to_value(&section).context("serialize CR section")?;
    lockfile.paradigms.insert(CR_PREFIX.to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("added composition root pattern `{}`", args.pattern);
    println!("updated {}", written.display());
    Ok(())
}

fn cx(cmd: CxCommand) -> Result<()> {
    match cmd {
        CxCommand::SetDefault(args) => cx_set_default_cli(args),
        CxCommand::AddOverride(args) => cx_add_override_cli(args),
    }
}

fn cx_set_default_cli(args: CxSetDefaultArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: CxSection = lockfile
        .paradigm_section(CX_PREFIX)
        .context("CX lockfile section is malformed")?;

    cx_set_default_max_lines(&mut section, args.max_lines);

    let value = serde_json::to_value(&section).context("serialize CX section")?;
    lockfile.paradigms.insert(CX_PREFIX.to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("set CX default function-line budget to {}", args.max_lines);
    println!("updated {}", written.display());
    Ok(())
}

fn cx_add_override_cli(args: CxAddOverrideArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: CxSection = lockfile
        .paradigm_section(CX_PREFIX)
        .context("CX lockfile section is malformed")?;

    cx_add_override(&mut section, &args.module, args.max_lines, args.force)
        .with_context(|| format!("add CX override for `{}`", args.module))?;

    let value = serde_json::to_value(&section).context("serialize CX section")?;
    lockfile.paradigms.insert(CX_PREFIX.to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!(
        "added CX override `{}` -> {} lines",
        args.module, args.max_lines
    );
    println!("updated {}", written.display());
    Ok(())
}

fn da(cmd: DaCommand) -> Result<()> {
    match cmd {
        DaCommand::Enable(args) => da_set_enabled_cli(args, true),
        DaCommand::Disable(args) => da_set_enabled_cli(args, false),
        DaCommand::AcceptSingleImpl(args) => da_accept_single_impl_cli(args),
    }
}

fn da_set_enabled_cli(args: DaToggleArgs, enabled: bool) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: DaSection = lockfile
        .paradigm_section(DA_PREFIX)
        .context("DA lockfile section is malformed")?;

    da_set_enabled(&mut section, enabled);

    let value = serde_json::to_value(&section).context("serialize DA section")?;
    lockfile.paradigms.insert(DA_PREFIX.to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!(
        "DA paradigm {}",
        if enabled { "enabled" } else { "disabled" }
    );
    println!("updated {}", written.display());
    Ok(())
}

fn da_accept_single_impl_cli(args: DaAcceptSingleImplArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: DaSection = lockfile
        .paradigm_section(DA_PREFIX)
        .context("DA lockfile section is malformed")?;

    da_add_accepted_single_impl(&mut section, &args.pattern)
        .with_context(|| format!("accept single-impl trait `{}`", args.pattern))?;

    let value = serde_json::to_value(&section).context("serialize DA section")?;
    lockfile.paradigms.insert(DA_PREFIX.to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("accepted single-impl trait pattern `{}`", args.pattern);
    println!("updated {}", written.display());
    Ok(())
}

fn dc(cmd: DcCommand) -> Result<()> {
    match cmd {
        DcCommand::Enable(args) => dc_set_require_cli(args, true),
        DcCommand::Disable(args) => dc_set_require_cli(args, false),
        DcCommand::AddExemptPath(args) => dc_add_exempt_path_cli(args),
    }
}

fn dc_set_require_cli(args: DcToggleArgs, value: bool) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: DcSection = lockfile
        .paradigm_section(DC_PREFIX)
        .context("DC lockfile section is malformed")?;

    dc_set_require(&mut section, value);

    let serialized = serde_json::to_value(&section).context("serialize DC section")?;
    lockfile.paradigms.insert(DC_PREFIX.to_string(), serialized);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!(
        "DC require_public_docs {}",
        if value { "enabled" } else { "disabled" }
    );
    println!("updated {}", written.display());
    Ok(())
}

fn dc_add_exempt_path_cli(args: DcAddExemptPathArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: DcSection = lockfile
        .paradigm_section(DC_PREFIX)
        .context("DC lockfile section is malformed")?;

    dc_add_exempt_path(&mut section, &args.pattern)
        .with_context(|| format!("add exempt path `{}`", args.pattern))?;

    let value = serde_json::to_value(&section).context("serialize DC section")?;
    lockfile.paradigms.insert(DC_PREFIX.to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("added exempt path pattern `{}`", args.pattern);
    println!("updated {}", written.display());
    Ok(())
}

fn fl(cmd: FlCommand) -> Result<()> {
    match cmd {
        FlCommand::AddDomainPath(args) => fl_add_domain_path_cli(args),
        FlCommand::AddBoundaryError(args) => fl_add_boundary_error_cli(args),
    }
}

fn fl_add_domain_path_cli(args: FlAddDomainPathArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: FlSection = lockfile
        .paradigm_section(FL_PREFIX)
        .context("FL lockfile section is malformed")?;

    fl_add_domain_path(&mut section, &args.pattern)
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

fn fl_add_boundary_error_cli(args: FlAddBoundaryErrorArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: FlSection = lockfile
        .paradigm_section(FL_PREFIX)
        .context("FL lockfile section is malformed")?;

    fl_add_boundary_error_pattern(&mut section, &args.pattern)
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

fn er(cmd: ErCommand) -> Result<()> {
    match cmd {
        ErCommand::AddDomainPath(args) => er_add_domain_path_cli(args),
    }
}

fn er_add_domain_path_cli(args: ErAddDomainPathArgs) -> Result<()> {
    use locus_core::paradigms::error_taxonomy::edit::add_domain_path;
    use locus_core::paradigms::error_taxonomy::lockfile_schema::ErSection;

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

fn fo(cmd: FoCommand) -> Result<()> {
    match cmd {
        FoCommand::DefineFeature(args) => fo_define_feature_cli(args),
    }
}

fn fo_define_feature_cli(args: FoDefineFeatureArgs) -> Result<()> {
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

fn mo(cmd: MoCommand) -> Result<()> {
    match cmd {
        MoCommand::SetDefault(args) => mo_set_default_cli(args),
        MoCommand::AddOverride(args) => mo_add_override_cli(args),
    }
}

fn mo_set_default_cli(args: MoSetDefaultArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: MoSection = lockfile
        .paradigm_section(MO_PREFIX)
        .context("MO lockfile section is malformed")?;

    mo_set_default_max_public_types(&mut section, args.max_types);

    let value = serde_json::to_value(&section).context("serialize MO section")?;
    lockfile.paradigms.insert(MO_PREFIX.to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("set MO default public-types budget to {}", args.max_types);
    println!("updated {}", written.display());
    Ok(())
}

fn mo_add_override_cli(args: MoAddOverrideArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: MoSection = lockfile
        .paradigm_section(MO_PREFIX)
        .context("MO lockfile section is malformed")?;

    mo_add_override(&mut section, &args.module, args.max_types, args.force)
        .with_context(|| format!("add MO override for `{}`", args.module))?;

    let value = serde_json::to_value(&section).context("serialize MO section")?;
    lockfile.paradigms.insert(MO_PREFIX.to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!(
        "added MO override `{}` -> {} types",
        args.module, args.max_types
    );
    println!("updated {}", written.display());
    Ok(())
}

fn ob(cmd: ObCommand) -> Result<()> {
    match cmd {
        ObCommand::AddObserverPath(args) => ob_add_observer_path_cli(args),
        ObCommand::AddForbiddenLogTarget(args) => ob_add_forbidden_log_target_cli(args),
    }
}

fn ob_add_observer_path_cli(args: ObAddObserverPathArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: ObSection = lockfile
        .paradigm_section(OB_PREFIX)
        .context("OB lockfile section is malformed")?;

    ob_add_observer_path(&mut section, &args.pattern)
        .with_context(|| format!("add observer path `{}`", args.pattern))?;

    let value = serde_json::to_value(&section).context("serialize OB section")?;
    lockfile.paradigms.insert(OB_PREFIX.to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("added observer path pattern `{}`", args.pattern);
    println!("updated {}", written.display());
    Ok(())
}

fn ob_add_forbidden_log_target_cli(args: ObAddForbiddenLogTargetArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: ObSection = lockfile
        .paradigm_section(OB_PREFIX)
        .context("OB lockfile section is malformed")?;

    ob_add_forbidden_log_target(&mut section, &args.pattern)
        .with_context(|| format!("add forbidden log target `{}`", args.pattern))?;

    let value = serde_json::to_value(&section).context("serialize OB section")?;
    lockfile.paradigms.insert(OB_PREFIX.to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("added forbidden log target pattern `{}`", args.pattern);
    println!("updated {}", written.display());
    Ok(())
}

fn pa(cmd: PaCommand) -> Result<()> {
    match cmd {
        PaCommand::AcceptColocated(args) => pa_accept_colocated_cli(args),
        PaCommand::AddApplicationPath(args) => pa_add_application_path_cli(args),
    }
}

fn pa_accept_colocated_cli(args: PaAcceptColocatedArgs) -> Result<()> {
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

fn pa_add_application_path_cli(args: PaAddApplicationPathArgs) -> Result<()> {
    use locus_core::paradigms::port_adapter::edit::add_application_path;
    use locus_core::paradigms::port_adapter::lockfile_schema::PaSection;

    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: PaSection = lockfile
        .paradigm_section("PA")
        .context("PA lockfile section is malformed")?;

    add_application_path(&mut section, &args.pattern)
        .with_context(|| format!("add PA application path `{}`", args.pattern))?;

    let value = serde_json::to_value(&section).context("serialize PA section")?;
    lockfile.paradigms.insert("PA".to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("added PA application path pattern `{}`", args.pattern);
    println!("updated {}", written.display());
    Ok(())
}

fn rm(cmd: RmCommand) -> Result<()> {
    match cmd {
        RmCommand::SetDefault(args) => rm_set_default_cli(args),
        RmCommand::AddExemptPath(args) => rm_add_exempt_path_cli(args),
        RmCommand::AddDomainPath(args) => rm_add_domain_path_cli(args),
    }
}

fn rm_set_default_cli(args: RmSetDefaultArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: RmSection = lockfile
        .paradigm_section(RM_PREFIX)
        .context("RM lockfile section is malformed")?;

    rm_set_default_max_action_kinds(&mut section, args.max_kinds);

    let value = serde_json::to_value(&section).context("serialize RM section")?;
    lockfile.paradigms.insert(RM_PREFIX.to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("set RM default action-kind cap to {}", args.max_kinds);
    println!("updated {}", written.display());
    Ok(())
}

fn rm_add_exempt_path_cli(args: RmAddExemptPathArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: RmSection = lockfile
        .paradigm_section(RM_PREFIX)
        .context("RM lockfile section is malformed")?;

    rm_add_exempt_path(&mut section, &args.pattern)
        .with_context(|| format!("add exempt path `{}`", args.pattern))?;

    let value = serde_json::to_value(&section).context("serialize RM section")?;
    lockfile.paradigms.insert(RM_PREFIX.to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("added exempt path pattern `{}`", args.pattern);
    println!("updated {}", written.display());
    Ok(())
}

fn rm_add_domain_path_cli(args: RmAddDomainPathArgs) -> Result<()> {
    use locus_core::paradigms::responsibility::edit::add_domain_path;
    use locus_core::paradigms::responsibility::lockfile_schema::RmSection;

    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: RmSection = lockfile
        .paradigm_section("RM")
        .context("RM lockfile section is malformed")?;

    add_domain_path(&mut section, &args.pattern)
        .with_context(|| format!("add RM domain path `{}`", args.pattern))?;

    let value = serde_json::to_value(&section).context("serialize RM section")?;
    lockfile.paradigms.insert("RM".to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("added RM domain path pattern `{}`", args.pattern);
    println!("updated {}", written.display());
    Ok(())
}

fn rw(cmd: RwCommand) -> Result<()> {
    match cmd {
        RwCommand::AcceptRuntimeOwner(args) => rw_accept_runtime_owner_cli(args),
    }
}

fn rw_accept_runtime_owner_cli(args: RwAcceptRuntimeOwnerArgs) -> Result<()> {
    use locus_core::paradigms::runtime_work::edit::add_runtime_owner_path;
    use locus_core::paradigms::runtime_work::lockfile_schema::RwSection;

    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: RwSection = lockfile
        .paradigm_section("RW")
        .context("RW lockfile section is malformed")?;

    add_runtime_owner_path(&mut section, &args.pattern)
        .with_context(|| format!("add RW runtime owner path `{}`", args.pattern))?;

    let value = serde_json::to_value(&section).context("serialize RW section")?;
    lockfile.paradigms.insert("RW".to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("added RW runtime owner pattern `{}`", args.pattern);
    println!("updated {}", written.display());
    Ok(())
}

fn ta(cmd: TaCommand) -> Result<()> {
    match cmd {
        TaCommand::AddTestPath(args) => ta_add_test_path_cli(args),
    }
}

fn ta_add_test_path_cli(args: TaAddTestPathArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mut section: TaSection = lockfile
        .paradigm_section(TA_PREFIX)
        .context("TA lockfile section is malformed")?;

    ta_add_test_path(&mut section, &args.pattern)
        .with_context(|| format!("add test path `{}`", args.pattern))?;

    let value = serde_json::to_value(&section).context("serialize TA section")?;
    lockfile.paradigms.insert(TA_PREFIX.to_string(), value);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("added test path pattern `{}`", args.pattern);
    println!("updated {}", written.display());
    Ok(())
}

fn dg(cmd: DgCommand) -> Result<()> {
    match cmd {
        DgCommand::ForbidEdge(args) => dg_forbid_edge(args),
        DgCommand::DefineFeature(args) => dg_define_feature(args),
        DgCommand::AddSharedPath(args) => dg_add_shared_path(args),
    }
}

fn dg_define_feature(args: DgDefineFeatureArgs) -> Result<()> {
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

fn dg_add_shared_path(args: DgAddSharedPathArgs) -> Result<()> {
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

fn dg_forbid_edge(args: DgForbidEdgeArgs) -> Result<()> {
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

fn accept(cmd: AcceptCommand) -> Result<()> {
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

    let summary = apply_accept_command(cmd, &mut section, &air)?;

    let value = serde_json::to_value(&section).context("serialize OT section")?;
    lockfile.paradigms.insert(OT_PREFIX.to_string(), value);
    let written = lockfile
        .save(&workspace)
        .with_context(|| format!("write lockfile to {}", workspace.display()))?;

    println!("{summary}");
    println!("updated {}", written.display());
    Ok(())
}

fn apply_accept_command(
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

fn init(args: InitArgs) -> Result<()> {
    use locus_core::lockfile::LOCKFILE_NAME;

    let lockfile_path = args.workspace.join(LOCKFILE_NAME);
    if args.no_overwrite && lockfile_path.exists() {
        anyhow::bail!(
            "{} already exists; rerun without --no-overwrite to replace it",
            lockfile_path.display()
        );
    }

    let air = locus_rust::scan(&args.workspace)
        .with_context(|| format!("scan failed: {}", args.workspace.display()))?;
    let registry = registry();

    // Load existing lockfile so previously-acknowledged prefixes and accepted
    // decisions survive a re-run, then refresh paradigm sections.
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    populate_lockfile_sections(
        &mut lockfile,
        &registry,
        &air,
        args.acknowledge_empty.as_deref(),
    );

    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("wrote {}", written.display());
    print_init_sections_summary(&registry, &lockfile);

    let suggestions = collect_init_suggestions(&registry, &air, &lockfile);
    let hints_promoted = count_hint_promotions(&lockfile);
    print!("{}", render_checklist(&suggestions, hints_promoted));

    if !suggestions.is_empty() {
        // Flush before exit; process::exit skips destructors, so a buffered
        // stdout under pipe/redirect would otherwise drop the checklist.
        let _ = io::stdout().lock().flush();
        std::process::exit(1);
    }
    Ok(())
}

fn populate_lockfile_sections(
    lockfile: &mut Lockfile,
    registry: &[Box<dyn locus_core::Paradigm>],
    air: &locus_air::AirWorkspace,
    acknowledge_empty: Option<&str>,
) {
    // Re-run paradigm init to refresh sections from a fresh scan
    // (today only OT writes a non-empty section).
    for paradigm in registry {
        let section = paradigm.init(air);
        if !section_is_empty(&section) {
            lockfile
                .paradigms
                .insert(paradigm.rule_prefix().to_string(), section);
        }
    }
    if let Some(raw) = acknowledge_empty {
        for prefix in parse_prefix_list(raw) {
            if !lockfile.acknowledged_empty.iter().any(|p| p == &prefix) {
                lockfile.acknowledged_empty.push(prefix);
            }
        }
    }
}

fn print_init_sections_summary(registry: &[Box<dyn locus_core::Paradigm>], lockfile: &Lockfile) {
    for paradigm in registry {
        let count = lockfile
            .paradigms
            .get(paradigm.rule_prefix())
            .map(summarize_section)
            .unwrap_or_else(|| "(empty)".to_string());
        println!(
            "  {} {}: {}",
            paradigm.rule_prefix(),
            paradigm.name(),
            count
        );
    }
}

fn collect_init_suggestions(
    registry: &[Box<dyn locus_core::Paradigm>],
    air: &locus_air::AirWorkspace,
    lockfile: &Lockfile,
) -> Vec<locus_core::init::Suggestion> {
    let mut suggestions: Vec<locus_core::init::Suggestion> = Vec::new();
    for paradigm in registry {
        suggestions.extend(paradigm.suggest(air, lockfile));
    }
    suggestions.extend(locus_core::init::cross_paradigm_suggestions(air, lockfile));
    let seeds = locus_core::init::default_vacancy_seeds();
    suggestions.extend(locus_core::init::vacancy_seeds(
        air,
        lockfile,
        seeds,
        &suggestions,
    ));
    locus_core::init::aggregate(suggestions)
}

fn count_hint_promotions(lockfile: &Lockfile) -> usize {
    use locus_core::paradigms::one_truth::lockfile_schema::{OtSection, Source};

    let section: OtSection = match lockfile.paradigm_section("OT") {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let mut count = 0usize;
    for entry in section.concepts.values() {
        if entry.canonical.source == Source::Hint {
            count += 1;
        }
        for b in &entry.boundaries {
            if b.source == Source::Hint {
                count += 1;
            }
        }
    }
    count
}

fn section_is_empty(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Null => true,
        serde_json::Value::Object(m) => m.is_empty() || m.values().all(section_is_empty),
        serde_json::Value::Array(a) => a.is_empty(),
        _ => false,
    }
}

fn summarize_section(v: &serde_json::Value) -> String {
    // Best-effort summary; specific paradigms can override later by exposing
    // their own renderer when there's enough variety to justify it.
    if let Some(concepts) = v.get("concepts").and_then(|c| c.as_object()) {
        let canonicals = concepts.len();
        let boundaries: usize = concepts
            .values()
            .filter_map(|c| c.get("boundaries").and_then(|b| b.as_array()))
            .map(|a| a.len())
            .sum();
        let converters: usize = concepts
            .values()
            .filter_map(|c| c.get("converters").and_then(|b| b.as_array()))
            .map(|a| a.len())
            .sum();
        return format!(
            "{canonicals} concept(s), {boundaries} boundary(ies), {converters} converter(s)"
        );
    }
    "section recorded".to_string()
}

fn emit_air(args: EmitAirArgs) -> Result<()> {
    let air = locus_rust::scan(&args.workspace)
        .with_context(|| format!("scan failed: {}", args.workspace.display()))?;

    let mut writer: Box<dyn Write> = match args.output {
        Some(path) => Box::new(BufWriter::new(
            File::create(&path).with_context(|| format!("create {}", path.display()))?,
        )),
        None => Box::new(BufWriter::new(io::stdout().lock())),
    };

    if args.pretty {
        serde_json::to_writer_pretty(&mut writer, &air)?;
    } else {
        serde_json::to_writer(&mut writer, &air)?;
    }
    writer.write_all(b"\n")?;
    Ok(())
}

fn debt(args: DebtArgs) -> Result<()> {
    use locus_core::exceptions::{collect_exceptions, today_utc};

    let air = locus_rust::scan(&args.workspace)
        .with_context(|| format!("scan failed: {}", args.workspace.display()))?;
    let lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let today = today_utc();
    let entries = collect_exceptions(&air, &lockfile, Some(&today));

    if args.json {
        return print_debt_json(&entries, args.by_rule);
    }

    if args.by_rule {
        print_debt_by_rule_text(&entries);
    } else {
        print_debt_text(&entries);
    }
    Ok(())
}

fn print_debt_json(
    entries: &[locus_core::exceptions::ExceptionEntry],
    by_rule: bool,
) -> Result<()> {
    use locus_core::exceptions::{ExceptionSource, ExceptionStatus};

    if by_rule {
        let grouped = group_debt_by_rule(entries);
        let stdout = io::stdout();
        let mut w = BufWriter::new(stdout.lock());
        for row in grouped {
            serde_json::to_writer(&mut w, &row)?;
            w.write_all(b"\n")?;
        }
        return Ok(());
    }
    let stdout = io::stdout();
    let mut w = BufWriter::new(stdout.lock());
    for e in entries {
        let row = serde_json::json!({
            "source": match e.source {
                ExceptionSource::Hint => "hint",
                ExceptionSource::Lockfile => "lockfile",
                ExceptionSource::CxExemptPath => "cx_exempt_path",
            },
            "rule": e.rule,
            "target": e.target,
            "reason": e.reason,
            "expires": e.expires,
            "status": match e.status {
                ExceptionStatus::Active => "active",
                ExceptionStatus::Expired => "expired",
                ExceptionStatus::Unbounded => "unbounded",
                ExceptionStatus::LegacyNoMetadata => "legacy_no_metadata",
            },
        });
        serde_json::to_writer(&mut w, &row)?;
        w.write_all(b"\n")?;
    }
    Ok(())
}

fn prune(args: PruneArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let today = today_utc();
    let removed = prune_expired_lockfile_exceptions(&mut lockfile, &today);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;
    println!("removed {removed} expired lockfile exception(s)");
    println!("updated {}", written.display());
    Ok(())
}

fn prune_expired_lockfile_exceptions(lockfile: &mut Lockfile, today: &str) -> usize {
    let before = lockfile.exceptions.len();
    lockfile
        .exceptions
        .retain(|ex| ex.expires.as_str() >= today);
    before.saturating_sub(lockfile.exceptions.len())
}

fn explain(args: ExplainArgs) -> Result<()> {
    let docs_path = args.workspace.join("docs").join("PARADIGMS.md");
    let body = std::fs::read_to_string(&docs_path)
        .with_context(|| format!("read {}", docs_path.display()))?;
    let Some(section) = extract_rule_section(&body, &args.rule_id) else {
        anyhow::bail!(
            "rule `{}` not found in {}",
            args.rule_id,
            docs_path.display()
        );
    };
    println!("{section}");
    Ok(())
}

fn extract_rule_section(markdown: &str, rule_id: &str) -> Option<String> {
    let needle = format!("#### {rule_id} ");
    let lines: Vec<&str> = markdown.lines().collect();
    let start = lines
        .iter()
        .position(|line| line.trim_start().starts_with(&needle))?;
    let end = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find_map(|(idx, line)| line.trim_start().starts_with("#### ").then_some(idx))
        .unwrap_or(lines.len());
    Some(lines[start..end].join("\n").trim().to_string())
}

fn group_debt_by_rule(
    entries: &[locus_core::exceptions::ExceptionEntry],
) -> Vec<serde_json::Value> {
    use locus_core::exceptions::ExceptionStatus;
    use std::collections::BTreeMap;

    // (total, active, expired, unbounded, legacy_no_metadata)
    let mut rows: BTreeMap<String, (usize, usize, usize, usize, usize)> = BTreeMap::new();
    for e in entries {
        let slot = rows.entry(e.rule.clone()).or_insert((0, 0, 0, 0, 0));
        slot.0 += 1;
        match e.status {
            ExceptionStatus::Active => slot.1 += 1,
            ExceptionStatus::Expired => slot.2 += 1,
            ExceptionStatus::Unbounded => slot.3 += 1,
            ExceptionStatus::LegacyNoMetadata => slot.4 += 1,
        }
    }

    rows.into_iter()
        .map(
            |(rule, (total, active, expired, unbounded, legacy_no_metadata))| {
                serde_json::json!({
                    "rule": rule,
                    "total": total,
                    "active": active,
                    "expired": expired,
                    "unbounded": unbounded,
                    "legacy_no_metadata": legacy_no_metadata,
                })
            },
        )
        .collect()
}

fn print_debt_by_rule_text(entries: &[locus_core::exceptions::ExceptionEntry]) {
    let grouped = group_debt_by_rule(entries);
    println!("debt by rule ({} rules with suppressions)", grouped.len());
    for row in grouped {
        println!(
            "  {:<6} total {:<4} active {:<4} expired {:<4} unbounded {:<4} legacy-no-metadata {:<4}",
            row["rule"].as_str().unwrap_or(""),
            row["total"].as_u64().unwrap_or(0),
            row["active"].as_u64().unwrap_or(0),
            row["expired"].as_u64().unwrap_or(0),
            row["unbounded"].as_u64().unwrap_or(0),
            row["legacy_no_metadata"].as_u64().unwrap_or(0)
        );
    }
}

fn format_debt_entry(e: &locus_core::exceptions::ExceptionEntry) -> String {
    use locus_core::exceptions::ExceptionSource;
    let source = match e.source {
        ExceptionSource::Hint => "hint",
        ExceptionSource::Lockfile => "lock",
        ExceptionSource::CxExemptPath => "cx-exempt",
    };
    let expires = e.expires.as_deref().unwrap_or("—");
    let reason = e.reason.as_deref().unwrap_or("");
    format!(
        "  {:<8} {:<40} expires {:<12} ({}) {}",
        e.rule, e.target, expires, source, reason
    )
}

fn print_debt_status_section(
    entries: &[locus_core::exceptions::ExceptionEntry],
    status: locus_core::exceptions::ExceptionStatus,
    header: &str,
) {
    let rows: Vec<_> = entries.iter().filter(|e| e.status == status).collect();
    if !rows.is_empty() {
        println!();
        println!("{header}");
        for e in rows {
            println!("{}", format_debt_entry(e));
        }
    }
}

fn print_debt_text(entries: &[locus_core::exceptions::ExceptionEntry]) {
    use locus_core::exceptions::ExceptionStatus;

    let (mut active, mut expired, mut unbounded, mut legacy_no_metadata) =
        (0usize, 0usize, 0usize, 0usize);
    for e in entries {
        match e.status {
            ExceptionStatus::Active => active += 1,
            ExceptionStatus::Expired => expired += 1,
            ExceptionStatus::Unbounded => unbounded += 1,
            ExceptionStatus::LegacyNoMetadata => legacy_no_metadata += 1,
        }
    }
    println!(
        "debt: {active} active, {expired} expired, {unbounded} unbounded, \
         {legacy_no_metadata} legacy-no-metadata ({} total)",
        entries.len()
    );

    print_debt_status_section(
        entries,
        ExceptionStatus::Expired,
        "EXPIRED  (re-run `locus check` for LOCUS001 advisories)",
    );
    print_debt_status_section(
        entries,
        ExceptionStatus::LegacyNoMetadata,
        "LEGACY-NO-METADATA  (pre-schema entries — add reason/expires/owner \
         or migrate to struct form)",
    );
    print_debt_status_section(
        entries,
        ExceptionStatus::Unbounded,
        "UNBOUNDED  (no expiry — review or add one)",
    );
    print_debt_status_section(entries, ExceptionStatus::Active, "ACTIVE");
}

fn check(args: CheckArgs) -> Result<()> {
    let air = locus_rust::scan(&args.workspace)
        .with_context(|| format!("scan failed: {}", args.workspace.display()))?;
    let lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let mode = if args.agent_strict {
        CheckMode::AgentStrict
    } else {
        CheckMode::Human
    };

    let mut all = Vec::new();
    for paradigm in registry() {
        all.extend(paradigm.check(&air, &lockfile, mode));
    }

    // Apply exceptions BEFORE Policy Guard — PG must not be suppressible by
    // the same lockfile it audits. See #44.
    let today = today_utc();
    let all = apply_exceptions(all, &air, &lockfile, Some(&today));

    // --changed filter is applied before PG so PG diagnostics bypass it
    // (PG is global; it must not be hidden by a PR-scoped diff filter).
    let mut all = apply_changed_filter(all, &args)?;

    // Policy Guard appended last: after apply_exceptions and --changed.
    append_policy_guard(&mut all, &lockfile, &args, mode)?;

    emit_check_output(&all, args.json)?;

    let any_fatal = all.iter().any(|d| d.severity.is_fatal());
    if any_fatal {
        std::process::exit(1);
    }
    Ok(())
}

fn emit_check_output(all: &[Diagnostic], json: bool) -> Result<()> {
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    if json {
        serde_json::to_writer_pretty(&mut out, all)?;
        writeln!(out)?;
    } else {
        report_text(&mut out, all)?;
    }
    out.flush()?;
    Ok(())
}

fn apply_changed_filter(all: Vec<Diagnostic>, args: &CheckArgs) -> Result<Vec<Diagnostic>> {
    if !args.changed {
        return Ok(all);
    }
    let workspace_abs = args
        .workspace
        .canonicalize()
        .unwrap_or_else(|_| args.workspace.clone());
    let changed =
        diff::changed_files(&workspace_abs, args.baseline.as_deref()).with_context(|| {
            format!(
                "computing changed files in {} (--changed)",
                workspace_abs.display()
            )
        })?;
    Ok(all
        .into_iter()
        .filter(|d| {
            changed
                .iter()
                .any(|rel| diff::paths_match(&d.span.file, rel, &workspace_abs))
        })
        .collect())
}

fn append_policy_guard(
    all: &mut Vec<Diagnostic>,
    lockfile: &Lockfile,
    args: &CheckArgs,
    mode: CheckMode,
) -> Result<()> {
    let baseline_lockfile = diff::read_baseline_lockfile(&args.workspace, args.baseline.as_deref());
    let pg = locus_core::check_policy_mutation(
        lockfile,
        baseline_lockfile.as_ref(),
        mode,
        args.allow_policy_calibration,
        args.allow_missing_policy_baseline,
    );
    if args.allow_policy_calibration && !pg.is_empty() {
        report_policy_calibration(&pg)?;
    }
    all.extend(pg);
    Ok(())
}

/// Print a structured before/after report for Policy Guard diagnostics
/// when `--allow-policy-calibration` is set. The report is informational
/// — the diagnostics themselves are also rendered in normal output, but
/// at Advisory severity. Per #44 §"Calibration mode".
fn report_policy_calibration(pg: &[Diagnostic]) -> Result<()> {
    use std::io::Write;
    let stderr = io::stderr();
    let mut w = stderr.lock();
    writeln!(w, "Policy calibration report ({} mutation(s)):", pg.len())?;
    for d in pg {
        writeln!(w, "  [{}] {}", d.rule_id, d.message)?;
        for line in &d.why {
            writeln!(w, "    why: {line}")?;
        }
    }
    writeln!(
        w,
        "(invoked with --allow-policy-calibration; PG001-PG004/PG008 fire as \
         Advisory. PG000 (missing baseline) and PG006 (missing debt \
         metadata) remain strict — calibration legitimizes intentional \
         widening, not a missing audit or missing justification.)"
    )?;
    Ok(())
}

fn report_text<W: Write>(out: &mut W, diags: &[Diagnostic]) -> io::Result<()> {
    if diags.is_empty() {
        writeln!(out, "no diagnostics — workspace is clean.")?;
        return Ok(());
    }
    let mut fatal = 0usize;
    let mut warning = 0usize;
    let mut advisory = 0usize;
    for d in diags {
        let label = match d.severity {
            Severity::Fatal => {
                fatal += 1;
                "error"
            }
            Severity::Warning => {
                warning += 1;
                "warning"
            }
            Severity::Advisory => {
                advisory += 1;
                "info"
            }
        };
        writeln!(
            out,
            "{label}[{}]: {}\n  --> {}:{}",
            d.rule_id, d.message, d.span.file, d.span.line_start
        )?;
        if let Some(c) = &d.concept {
            writeln!(out, "  concept: {c}")?;
        }
        for reason in &d.why {
            writeln!(out, "  - {reason}")?;
        }
        if let Some(fix) = &d.suggested_fix {
            writeln!(out, "  fix: {fix}")?;
        }
        writeln!(out)?;
    }
    writeln!(
        out,
        "summary: {fatal} error(s), {warning} warning(s), {advisory} advisory."
    )?;
    Ok(())
}

fn parse_prefix_list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_uppercase())
        .collect()
}

#[cfg(test)]
mod parse_prefix_list_tests {
    use super::*;

    #[test]
    fn splits_and_uppercases() {
        assert_eq!(parse_prefix_list("rw,da"), vec!["RW", "DA"]);
    }

    #[test]
    fn trims_whitespace_and_drops_empties() {
        assert_eq!(parse_prefix_list("  RW , , FO  "), vec!["RW", "FO"]);
    }

    #[test]
    fn empty_input_returns_empty() {
        assert!(parse_prefix_list("").is_empty());
        assert!(parse_prefix_list(" , ").is_empty());
    }
}

#[cfg(test)]
mod init_acknowledge_empty_tests {
    use super::*;
    use locus_core::lockfile::LOCKFILE_NAME;

    #[test]
    fn acknowledge_empty_persists_into_lockfile() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        // Minimal cargo workspace so `locus_rust::scan` succeeds.
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"x\"\nversion = \"0.0.1\"\nedition = \"2024\"\n[lib]\npath = \"src/lib.rs\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/lib.rs"), "").unwrap();

        // Ack every paradigm that emits a vacancy seed in `init`; otherwise
        // `init` calls `process::exit(1)` and aborts the test runner.
        // Derive the list from `default_vacancy_seeds()` so adding a new
        // seed doesn't silently break this test.
        let seed_prefixes: Vec<String> = locus_core::init::default_vacancy_seeds()
            .iter()
            .map(|(p, _, _)| (*p).to_string())
            .collect();
        let ack_input = seed_prefixes
            .iter()
            .map(|p| p.to_lowercase())
            .collect::<Vec<_>>()
            .join(",");

        let args = InitArgs {
            workspace: dir.to_path_buf(),
            no_overwrite: false,
            acknowledge_empty: Some(ack_input),
        };
        init(args).unwrap();

        let lockfile_bytes = std::fs::read(dir.join(LOCKFILE_NAME)).unwrap();
        let lf: Lockfile = serde_json::from_slice(&lockfile_bytes).unwrap();
        assert_eq!(lf.acknowledged_empty, seed_prefixes);
    }
}

fn render_checklist(suggestions: &[locus_core::init::Suggestion], hints_promoted: usize) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    let _ = writeln!(out, "auto-applied: {hints_promoted} source hints promoted");
    let _ = writeln!(out, "unresolved: {}", suggestions.len());
    if suggestions.is_empty() {
        return out;
    }
    for s in suggestions {
        out.push('\n');
        out.push_str(&s.render());
        out.push('\n');
    }
    out.push('\n');
    out.push_str("re-run `locus init` after applying changes.\n");
    out
}

#[cfg(test)]
mod render_checklist_tests {
    use super::*;
    use locus_core::init::Suggestion;

    #[test]
    fn render_empty_checklist_says_zero_unresolved() {
        let suggestions: Vec<Suggestion> = Vec::new();
        let out = render_checklist(&suggestions, /*hints_promoted=*/ 4);
        assert!(out.contains("auto-applied: 4 source hints promoted"));
        assert!(out.contains("unresolved: 0"));
        assert!(!out.contains("re-run"));
    }
}

#[cfg(test)]
mod debt_grouping_tests {
    use super::*;
    use locus_core::exceptions::{ExceptionEntry, ExceptionSource, ExceptionStatus};

    fn entry(rule: &str, status: ExceptionStatus) -> ExceptionEntry {
        ExceptionEntry {
            source: ExceptionSource::Hint,
            rule: rule.to_string(),
            target: "src/lib.rs:1".to_string(),
            reason: None,
            expires: None,
            status,
        }
    }

    #[test]
    fn groups_counts_by_rule_and_status() {
        let entries = vec![
            entry("DG003", ExceptionStatus::Active),
            entry("DG003", ExceptionStatus::Expired),
            entry("DG003", ExceptionStatus::Unbounded),
            entry("OT004", ExceptionStatus::Active),
            entry("OT004", ExceptionStatus::Active),
        ];

        let rows = group_debt_by_rule(&entries);
        assert_eq!(rows.len(), 2);

        let dg = rows
            .iter()
            .find(|r| r["rule"] == "DG003")
            .expect("DG003 row");
        assert_eq!(dg["total"], 3);
        assert_eq!(dg["active"], 1);
        assert_eq!(dg["expired"], 1);
        assert_eq!(dg["unbounded"], 1);

        let ot = rows
            .iter()
            .find(|r| r["rule"] == "OT004")
            .expect("OT004 row");
        assert_eq!(ot["total"], 2);
        assert_eq!(ot["active"], 2);
        assert_eq!(ot["expired"], 0);
        assert_eq!(ot["unbounded"], 0);
    }
}

#[cfg(test)]
mod prune_tests {
    use super::*;
    use locus_core::lockfile::Exception;

    #[test]
    fn prune_removes_only_expired_lockfile_exceptions() {
        let mut lockfile = Lockfile::empty();
        lockfile.exceptions = vec![
            Exception {
                rule: "OT004".to_string(),
                target: "src/lib.rs:1".to_string(),
                reason: "temporary".to_string(),
                expires: "2026-01-01".to_string(),
            },
            Exception {
                rule: "DG003".to_string(),
                target: "src/lib.rs:1".to_string(),
                reason: "temporary".to_string(),
                expires: "2026-12-31".to_string(),
            },
        ];

        let removed = prune_expired_lockfile_exceptions(&mut lockfile, "2026-05-09");
        assert_eq!(removed, 1);
        assert_eq!(lockfile.exceptions.len(), 1);
        assert_eq!(lockfile.exceptions[0].rule, "DG003");
    }
}

#[cfg(test)]
mod explain_tests {
    use super::*;

    #[test]
    fn extract_rule_section_returns_exact_heading_block() {
        let md = r#"
## X
#### OT004 — Name
line a
line b

#### DG001 — Next
line c
"#;
        let got = extract_rule_section(md, "OT004").expect("section exists");
        assert!(got.starts_with("#### OT004 — Name"));
        assert!(got.contains("line a"));
        assert!(got.contains("line b"));
        assert!(!got.contains("DG001"));
    }
}
