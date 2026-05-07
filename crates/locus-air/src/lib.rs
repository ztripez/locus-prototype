//! Architecture Intermediate Representation.
//!
//! Pure data + serde. No language-specific concerns, no inference logic.
//! Language adapters (e.g. `locus-rust`) build these structures; `locus-core`
//! consumes them. Schema is versioned via [`AirWorkspace::schema_version`] —
//! bump on any breaking field change.
//!
//! Self-application: every AIR type below is `// ot: canonical`. They are the
//! one accepted representation of "source facts in a workspace." No shadow
//! variants of these types should exist anywhere in the Locus codebase.

use serde::{Deserialize, Serialize};

/// AIR schema version. Bumped on breaking changes to how facts are emitted.
///
/// History:
/// - **1**: initial Phase 1 emission.
/// - **2**: type-text strings (`AirField.type_text`, `AirFunction.params`/
///   `return_type`, `AirConversion.from`/`to`, `AirConversion.symbol`) are
///   rendered cleanly — no extra spaces inside generics or around `&` / `::`.
/// - **3**: symbols are package-prefixed (`sample_crate::identity::User`)
///   instead of using the literal `crate` prefix. This makes symbols globally
///   unique across a Cargo workspace; without it, two crates can both emit
///   `crate::user::User` and collide in the lockfile.
/// - **4**: adds `AirItem::Import` for every `use` statement. Paths are
///   normalized so leading `crate` is rewritten to the package's lib name —
///   keeps import paths consistent with [`AirType::symbol`] for cross-paradigm
///   pattern matching (DG, future paradigms).
/// - **5**: paradigm-slice scaffolding for CX, DC, AB, PA. Adds
///   `AirFile.line_count` and `AirFunction.line_count` (CX),
///   `AirType.doc` and `AirFunction.doc` joined doc-comment text (DC),
///   `TypeKind::Trait` for trait declarations and a new `AirItem::Impl`
///   variant carrying every `impl` block — inherent or trait-implementing —
///   with its method names (AB, PA). All additions append to the end of
///   their owning structs so existing AIR JSON stays mostly stable.
/// - **6**: loader-tier `ActionKind` variants for CF, RW, OB. Adds `Spawn`
///   (detected from `*::spawn` calls — tokio, std::thread, rayon),
///   `EnvRead` (detected from `*::env::var` calls), and `Log` (detected
///   from logging macros: `println!`, `dbg!`, `eprintln!`, and any macro
///   path ending in a recognised log level like `tracing::info!`,
///   `log::warn!`). These join the existing `Construct`/`EnvMatch`/
///   `StringCompare` action signals for paradigms that need to reason
///   about runtime/observability concerns.
/// - **7**: introduces the loader tier. Adds `AirItem::CallSite` for every
///   call/method/macro invocation the visitor sees (framework-neutral —
///   path text and `CallKind` only) and `AirWorkspace.facts: Vec<AirFact>`
///   populated by loaders post-scan. Removes `ActionKind::Spawn`,
///   `ActionKind::EnvRead`, and `ActionKind::Log` — the visitor no longer
///   interprets framework-specific patterns; loaders translate AIR
///   call-sites into normalized `FactKind` (`SpawnsWork`, `ReadsEnv`,
///   `LogsRaw`, `LogsStructured`, `NetworkCall`, `DbWrite`) facts that
///   paradigms (CF, RW, OB) consume.
/// - **8**: realigns `FactKind` with the spec's normalized-fact vocabulary
///   (`docs/PARADIGMS.md` §"Framework Knowledge and Sub-Paradigm Loaders").
///   Renames: `SpawnsWork` → `SpawnedWork`, `ReadsEnv` → `ConfigRead`,
///   `NetworkCall` → `ExternalIo`, `DbWrite` → `PersistenceWrite`.
///   Collapses the `LogsRaw` / `LogsStructured` distinction into a single
///   `Logging` kind — raw-vs-structured is policy (OB's lockfile
///   `forbidden_log_targets` patterns), not a fact taxonomy.
///   Adds spec-mandated future-fact variants the loader tier will produce
///   over time: `BlockingCall`, `HotPath`, `RequestContext`, `BoundaryEntry`,
///   `RuntimeStateOwner`, `BackgroundWorker`. Adds `AirFact.evidence:
///   Option<String>` so consumers can match against the original callee
///   path (e.g. OB001 filtering `Logging` facts by their `println` /
///   `tracing::info` evidence).
/// - **9**: silent-error coverage. Adds two new `AirItem` variants the
///   visitor emits when scanning function bodies, both feeding the FL
///   paradigm (Failure Lineage):
///     - `AirItem::SilentDiscard` — captures `let _ = expr;` statements where
///       the discarded expression is a call (`Method` / `Function` / `Macro`).
///       Carries the rendered callee text and a `DiscardKind` so FL004 can
///       decide whether the discard is legitimate (e.g. `lock` / `send` /
///       `drop` patterns) or an agent-introduced silent failure swallow.
///     - `AirItem::PartialIfLet` — captures `if let Ok(...) = expr { … }`
///       and `if let Err(...) = expr { … }` patterns with **no** `else`
///       branch. The unmatched arm is implicitly silent; FL005 flags this
///       when the file is outside `invariant_owner_paths`.
///
///   Closes the silent-error coverage gap that FL003 (which only sees
///   `.ok()` / `.err()` method-call shape) couldn't reach.
pub const AIR_SCHEMA_VERSION: u32 = 9;

// ot: canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirWorkspace {
    pub schema_version: u32,
    pub packages: Vec<AirPackage>,
    /// Normalized facts produced by loaders after the visitor has finished.
    /// Loaders inspect `packages` (specifically `AirItem::CallSite` and
    /// `AirItem::Import`) and emit `AirFact` entries that paradigms consume
    /// in place of framework-specific reasoning. Empty when scan is run
    /// without loaders (e.g. via `scan_raw`).
    #[serde(default)]
    pub facts: Vec<AirFact>,
}

impl AirWorkspace {
    pub fn new(packages: Vec<AirPackage>) -> Self {
        Self {
            schema_version: AIR_SCHEMA_VERSION,
            packages,
            facts: Vec::new(),
        }
    }
}

// ot: canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirPackage {
    pub name: String,
    pub version: String,
    pub root_dir: String,
    pub files: Vec<AirFile>,
}

// ot: canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirFile {
    pub path: String,
    pub module_path: Option<String>,
    pub items: Vec<AirItem>,
    pub hints: Vec<AirHint>,
    pub parse_error: Option<String>,
    /// Total number of source lines in the file. Used by the CX (Complexity
    /// Budget) paradigm slice; counted by the language adapter from the raw
    /// source string so it isn't affected by the syn parse outcome.
    pub line_count: u32,
}

// ot: canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum AirItem {
    Type(AirType),
    Function(AirFunction),
    Conversion(AirConversion),
    Usage(AirUsage),
    TruthAction(AirTruthAction),
    Import(AirImport),
    Impl(AirImpl),
    CallSite(AirCallSite),
    SilentDiscard(AirSilentDiscard),
    PartialIfLet(AirPartialIfLet),
}

// ot: canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirType {
    // Renamed in JSON to avoid colliding with the AirItem external tag (also `kind`).
    #[serde(rename = "type_kind")]
    pub kind: TypeKind,
    pub name: String,
    pub symbol: String,
    pub visibility: Visibility,
    pub fields: Vec<AirField>,
    pub variants: Vec<AirVariant>,
    pub derives: Vec<String>,
    pub attrs: Vec<String>,
    pub span: AirSpan,
    /// Joined doc-comment text (`///` and `#[doc = "..."]`), one line per
    /// source comment with the rustdoc-convention single leading space
    /// stripped. `None` when the type has no doc comments. Consumed by the
    /// DC (Documentation) paradigm slice.
    pub doc: Option<String>,
}

// ot: canonical
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TypeKind {
    Struct,
    Enum,
    Alias,
    Union,
    Trait,
}

// ot: canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirField {
    pub name: String,
    pub type_text: String,
    pub visibility: Visibility,
}

// ot: canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirVariant {
    pub name: String,
    pub fields: Vec<AirField>,
}

// ot: canonical
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Visibility {
    Public,
    Crate,
    Restricted,
    Private,
}

// ot: canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirFunction {
    pub name: String,
    pub symbol: String,
    pub visibility: Visibility,
    pub params: Vec<(String, String)>,
    pub return_type: Option<String>,
    pub span: AirSpan,
    /// Lines spanned by the function (inclusive: `end_line - start_line + 1`).
    /// Drives the CX (Complexity Budget) paradigm slice.
    pub line_count: u32,
    /// Joined doc-comment text (`///` and `#[doc = "..."]`), one line per
    /// source comment with the rustdoc-convention single leading space
    /// stripped. `None` when the function has no doc comments. Consumed by
    /// the DC (Documentation) paradigm slice.
    pub doc: Option<String>,
}

// ot: canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirImpl {
    /// `Some("path::to::Trait")` for `impl Trait for Type`; `None` for
    /// inherent `impl Type`. Rendered with the same clean type-text
    /// formatting as [`AirType`] symbols.
    pub trait_path: Option<String>,
    /// The `Type` in `impl ... for Type`.
    pub self_ty: String,
    /// Names of methods declared inside the impl, in declaration order.
    /// Empty for empty impls.
    pub method_names: Vec<String>,
    pub span: AirSpan,
}

// ot: canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirConversion {
    pub from: String,
    pub to: String,
    pub mechanism: ConversionMechanism,
    pub symbol: String,
    pub span: AirSpan,
}

// ot: canonical
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ConversionMechanism {
    From,
    TryFrom,
    InherentMethod,
    FreeFn,
}

// ot: canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirUsage {
    pub from_symbol: String,
    pub to_symbol: String,
    #[serde(rename = "usage_kind")]
    pub kind: UsageKind,
    pub span: AirSpan,
}

// ot: canonical
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum UsageKind {
    FunctionParam,
    FunctionReturn,
    FieldType,
}

// ot: canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirTruthAction {
    pub action: ActionKind,
    pub target: String,
    pub function: Option<String>,
    pub span: AirSpan,
    pub confidence: f32,
    pub reasons: Vec<String>,
}

// ot: canonical
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ActionKind {
    Construct,
    EnumMatch,
    StringCompare,
    Validate,
    Normalize,
}

// ot: canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirCallSite {
    /// Rendered path text of the callee. For `tokio::spawn(f)` this is
    /// `"tokio::spawn"`; for `x.lock()` it's `"lock"` (just the method
    /// name — receiver-type resolution is out of scope); for the macro
    /// `tracing::info!(…)` it's `"tracing::info"`.
    pub callee: String,
    // Renamed in JSON to avoid colliding with the AirItem external tag (also `kind`).
    #[serde(rename = "call_kind")]
    pub kind: CallKind,
    /// Enclosing function's symbol, if known.
    pub function: Option<String>,
    pub span: AirSpan,
}

// ot: canonical
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum CallKind {
    Function,
    Method,
    Macro,
}

/// `let _ = expr;` — a discarded binding. Captured only when `expr` is a
/// call shape (`Method` / `Function` / `Macro`); arbitrary discarded
/// expressions (`let _ = some_field;` / `let _ = Block { ... };`) are
/// recorded with `kind = Other` and a `None` callee so FL004 can choose
/// to ignore them by default.
// ot: canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirSilentDiscard {
    /// Rendered callee text — same convention as [`AirCallSite::callee`]
    /// (last `::` segment for path-qualified macros; bare method name for
    /// method calls). `None` when the discarded expression isn't a call.
    pub callee: Option<String>,
    // Renamed in JSON to avoid colliding with the AirItem external tag (also `kind`).
    #[serde(rename = "discard_kind")]
    pub kind: DiscardKind,
    /// Enclosing function's symbol, if known.
    pub function: Option<String>,
    pub span: AirSpan,
}

// ot: canonical
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum DiscardKind {
    /// `let _ = receiver.method(...);`
    Method,
    /// `let _ = function(...);`
    Function,
    /// `let _ = macro!(...);`
    Macro,
    /// `let _ = some_other_expr;` — block, field access, literal, etc.
    /// Recorded for completeness; FL004 defaults to ignoring this kind
    /// because the false-positive surface is too large.
    Other,
}

/// `if let Ok(...) = expr { ... }` or `if let Err(...) = expr { ... }`
/// **without** an `else` branch. The unmatched arm is implicitly silent —
/// the failure (or success) just falls through. FL005 fires on this
/// shape outside `invariant_owner_paths`. Patterns matching anything
/// other than the `Ok` / `Err` `Result` constructors are not recorded.
// ot: canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirPartialIfLet {
    /// `"Ok"` or `"Err"` — the variant the surface `if let` matches on.
    /// We record this so FL005 can phrase the diagnostic precisely
    /// (a missing `Err` branch reads differently from a missing `Ok` one).
    pub variant: String,
    /// Enclosing function's symbol, if known.
    pub function: Option<String>,
    pub span: AirSpan,
}

// ot: canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirFact {
    pub kind: FactKind,
    pub target: FactTarget,
    /// Loader name that produced this fact, e.g. `"std-rt"`.
    pub source: String,
    pub confidence: f32,
    pub reasons: Vec<String>,
    /// Original callee / matched-identifier text, when the fact derived
    /// from a single call site. Lets paradigms filter facts by evidence
    /// without re-walking AIR (e.g. OB001 keeping its `forbidden_log_targets`
    /// pattern list against `Logging`-fact evidence). `None` for facts
    /// derived by aggregation or whole-file inference.
    #[serde(default)]
    pub evidence: Option<String>,
}

/// Normalized architectural facts loaders produce.
///
/// Vocabulary aligned with `docs/PARADIGMS.md` §"Framework Knowledge
/// and Sub-Paradigm Loaders" — these names are *architectural concepts*,
/// not framework categories. A persistence write is a persistence write
/// whether it came from `sqlx` or `redis` or filesystem code; an
/// external-io call is the same fact whether it's `reqwest`, `tonic`,
/// or `surf`. Loaders bridge specific frameworks → these normalized
/// kinds; paradigms only ever see the normalized kinds.
// ot: canonical
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum FactKind {
    /// A function or call site that spawns concurrent work
    /// (`spawned_work` in the spec). RW001's signal.
    SpawnedWork,
    /// A function reads behavior-shaping configuration data
    /// (`config_read` in the spec). CF001's signal. Today this is just
    /// env-var reads; future loaders will widen the source set.
    ConfigRead,
    /// Any logging primitive (`println!`, `dbg!`, `tracing::info!`,
    /// `log::warn!`, …). The raw-vs-structured policy decision belongs
    /// to OB001's `forbidden_log_targets` lockfile patterns, matched
    /// against [`AirFact::evidence`].
    Logging,
    /// Outbound external IO — HTTP/gRPC/queue calls reaching outside
    /// the process (`external_io` in the spec).
    ExternalIo,
    /// Persistence write — DB query, filesystem write, cache mutation
    /// (`persistence_write` in the spec).
    PersistenceWrite,
    /// A blocking call inside a non-blocking / async context
    /// (`blocking_call` in the spec). Reserved for a future loader; no
    /// loader produces this yet.
    BlockingCall,
    /// A hot-path function — registered in a hot loop, frame system,
    /// or per-request handler (`hot_path` in the spec). Reserved.
    HotPath,
    /// Function executing inside a request context (`request_context`).
    /// Reserved for future framework loaders (axum/actix/rocket).
    RequestContext,
    /// Function or impl marked as a boundary entry — the public surface
    /// where data crosses into the system (`boundary_entry`). Reserved.
    BoundaryEntry,
    /// Function/state owns a runtime resource (lock, channel, task
    /// supervisor) (`runtime_state_owner`). Reserved.
    RuntimeStateOwner,
    /// Function declared as a background worker / job processor
    /// (`background_worker`). Reserved.
    BackgroundWorker,
}

// ot: canonical
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "scope")]
pub enum FactTarget {
    /// Fact applies to a function symbol (most common case).
    Function { symbol: String },
    /// Fact applies to a whole file by path.
    File { path: String },
    /// Fact applies to a specific call site / span.
    Span(AirSpan),
}

// ot: canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirImport {
    /// Fully-rendered import path. `use foo::bar::Baz` → `"foo::bar::Baz"`.
    /// `use a::{b, c}` is flattened: each leaf becomes its own AirImport.
    /// Leading `crate::` is normalized to the package's lib name so paths
    /// are consistent with [`AirType::symbol`].
    pub path: String,
    pub visibility: Visibility,
    pub span: AirSpan,
}

// ot: canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirHint {
    pub kind: HintKind,
    pub raw: String,
    pub span: AirSpan,
    pub target_span: Option<AirSpan>,
}

// ot: canonical
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "category", rename_all = "kebab-case")]
pub enum HintKind {
    Canonical,
    Boundary {
        concept: Option<String>,
        boundary: Option<String>,
    },
    Converter,
    ProtocolTranslation {
        reason: Option<String>,
    },
    GeneratedBoundary,
    Allow {
        rule: String,
        reason: Option<String>,
        expires: Option<String>,
    },
    Unknown,
}

// ot: canonical
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AirSpan {
    pub file: String,
    pub line_start: u32,
    pub line_end: u32,
}

impl AirSpan {
    pub fn new(file: impl Into<String>, line_start: u32, line_end: u32) -> Self {
        Self {
            file: file.into(),
            line_start,
            line_end,
        }
    }
}
