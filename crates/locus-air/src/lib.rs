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
/// - **12**: closes the "tractable visitor work" gap from the audit.
///   Adds three new `AirItem` variants:
///     - `AirItem::FallbackCall` — `unwrap_or(literal)` /
///       `unwrap_or(call)` / `or(literal)` shapes where the
///       method's first argument is a default-producing
///       expression. Distinct from `ClosureMethodCall` (closure
///       arg) and `SilentDiscard` (`let _ = ...`); fills FL010's
///       "invalid input converted to valid default" detection.
///     - `AirItem::RetryLoop` — `loop` / `for` / `while`
///       expressions whose body contains both an `Expr::Try`
///       (a `?` propagating a Result) and an `Expr::Break`.
///       Catches FL012's "retry loops without an accepted retry
///       policy" shape.
///     - `AirItem::ScrutineeLiteral` — literal expressions
///       appearing as match-arm patterns or as the RHS of a
///       binary `==`/`!=` against a non-literal. Closes CF002
///       (magic decision constants) and CF003 (hardcoded
///       provider/model/topic IDs).
/// - **11**: user-declared fact markers. Adds `HintKind::MarksFact`
///   for `// ot: marks <fact_kind>` source hints — the user marks a
///   function as having a `FactKind` the loader tier can't infer
///   without framework knowledge (`hot_path`, `request_context`,
///   `boundary_entry`, `runtime_state_owner`, `background_worker`),
///   or annotates their own helper as a custom recogniser for an
///   already-produced kind (`external_io`, `persistence_write`,
///   `blocking_call`). The new `markers` loader translates these
///   hints into `AirFact` entries the consuming paradigms read the
///   same way they read std-rt's facts. Bridges the gap between the
///   architectural concepts already in `FactKind` and framework-
///   specific recognisers that haven't shipped yet.
/// - **10**: `match` arm bodies + closure-arg shape. Adds:
///     - `AirItem::MatchArm` — for every arm of every `match` expression
///       the visitor sees, records the pattern text, whether it contains
///       a wildcard binder (`_`), an `ArmBodyShape` heuristic for what the
///       arm's body does (`Empty` / `Literal` / `Call` / `Return` /
///       `Propagate` (uses `?`) / `Block` / `Other`), and the enclosing
///       function. Lets FL007 (catch-all `Err(_)`), FL011 (default-variant
///       failure sinks), and ER005 (catch-all error mapping) reason about
///       arm-level silence the previous AIR couldn't see.
///     - `AirItem::ClosureMethodCall` — for every method call whose first
///       argument is a closure (e.g. `result.map_err(|_| Default::default())`,
///       `result.unwrap_or_else(|e| log(e))`, `opt.or_else(|| ...)`),
///       records the callee, whether the closure pattern discards its
///       argument with `_`, and a body shape. Lets FL006 (`map_err`
///       losing source context) flag closures that throw the original
///       error away.
pub const AIR_SCHEMA_VERSION: u32 = 12;

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
    MatchArm(AirMatchArm),
    ClosureMethodCall(AirClosureMethodCall),
    FallbackCall(AirFallbackCall),
    RetryLoop(AirRetryLoop),
    ScrutineeLiteral(AirScrutineeLiteral),
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

/// One arm of a `match` expression. The visitor emits one
/// `AirItem::MatchArm` per arm so paradigm rules can reason about the
/// shape of each arm's body — specifically whether a `Result`-shape arm
/// silently swallows the unmatched case.
// ot: canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirMatchArm {
    /// The match expression's scrutinee, rendered as text.
    pub scrutinee: String,
    /// The arm's pattern, rendered as text (`"Ok(x)"`, `"Err(_)"`,
    /// `"_"`, `"Status::Active"`, …).
    pub pattern: String,
    /// `true` when the pattern contains at least one wildcard binder
    /// (`_`). Catches both bare `_` arms and tuple/struct patterns with
    /// `_` placeholders (`Err(_)`, `Foo(_, x)`).
    pub pattern_has_wildcard: bool,
    /// Heuristic shape of the arm's body. See [`ArmBodyShape`].
    pub body_shape: ArmBodyShape,
    /// Enclosing function's symbol, if known.
    pub function: Option<String>,
    pub span: AirSpan,
}

/// Coarse classification of a `match` arm body. Rules use this to tell
/// the difference between an arm that *handles* its case (returns,
/// propagates, computes something) and an arm that silently swallows
/// (unit body, literal default, `Default::default()` call).
// ot: canonical
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ArmBodyShape {
    /// Unit `()`, empty block `{}`. The arm matches and does nothing.
    Empty,
    /// A bare literal expression — `0`, `""`, `false`, `None`. The arm
    /// matches and returns a default value silently.
    Literal,
    /// A single function or method call (`Default::default()`,
    /// `Vec::new()`, `default()`). Often a silent default.
    Call,
    /// A `return` expression (with or without value). Definitely
    /// non-silent — control flow leaves the function.
    Return,
    /// The arm uses the `?` operator somewhere. The error is
    /// propagated to the caller — the opposite of silent.
    Propagate,
    /// A multi-statement block. Could be doing real work; the rule
    /// shouldn't pre-judge.
    Block,
    /// Anything else (constructor, method chain, macro call, …).
    Other,
}

/// A method call whose first argument is a closure. The visitor emits
/// these for shapes like `result.map_err(|_| ...)`,
/// `result.unwrap_or_else(|e| ...)`, `option.or_else(|| ...)`, etc.
/// FL006 uses [`Self::closure_discards_arg`] to flag `map_err(|_|)`-shape
/// closures that throw the original error away.
// ot: canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirClosureMethodCall {
    /// Bare method name — same convention as [`AirCallSite::callee`] for
    /// method calls. `"map_err"`, `"unwrap_or_else"`, `"or_else"`, …
    pub callee: String,
    /// `true` when the closure's first parameter pattern is `_` or the
    /// closure has no parameters (`|_| ...`, `|| ...`, `|_, x| ...`).
    /// `map_err(|_| ...)` is the canonical "lose source context"
    /// pattern.
    pub closure_discards_arg: bool,
    /// Heuristic shape of the closure body — same vocabulary as
    /// [`ArmBodyShape`] so paradigm rules can share matching logic.
    pub body_shape: ArmBodyShape,
    /// Enclosing function's symbol, if known.
    pub function: Option<String>,
    pub span: AirSpan,
}

/// A method call whose first argument is a *default-producing*
/// expression — `result.unwrap_or(0)`, `option.or(Default::default())`,
/// `result.or_else_with(some_fn)`. Distinct from
/// [`AirClosureMethodCall`] (closure arg, captured separately) and
/// [`AirSilentDiscard`] (no binding at all).
///
/// Used by FL010 to flag the "invalid input converted to a valid
/// default state" pattern: `result.unwrap_or(literal)` outside a
/// declared invariant-owner module is an agent shortcut that hides
/// the failure path. The shape captured in [`Self::default_shape`]
/// lets rules distinguish a literal default (most likely silent) from
/// a multi-statement fallback block (might be doing real work).
// ot: canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirFallbackCall {
    /// Bare method name (`unwrap_or`, `or`, `unwrap_or_default`).
    pub callee: String,
    /// Heuristic shape of the first-argument default expression.
    /// `unwrap_or_default` — which takes no argument — is recorded
    /// with `default_shape = Empty`.
    pub default_shape: ArmBodyShape,
    /// Enclosing function's symbol, if known.
    pub function: Option<String>,
    pub span: AirSpan,
}

/// A loop construct (`loop {}`, `for ... {}`, `while ... {}`) whose
/// body contains both error propagation (`?`) and an explicit `break`.
/// FL012 fires on this shape — it's the visitor's structural signal
/// for "retry without accepted policy": something fallible is being
/// repeated until it succeeds, with no declared retry policy or
/// backoff strategy.
// ot: canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirRetryLoop {
    /// What kind of loop this is. Useful for diagnostic phrasing
    /// (`for _ in 0..N` reads differently from `loop`).
    pub loop_kind: LoopKind,
    /// `true` when the loop body uses `?` somewhere (transitively).
    /// FL012 requires this — otherwise the loop is just a plain
    /// counter / iterator, not a retry.
    pub propagates: bool,
    /// `true` when the loop body has at least one `break` expression.
    /// FL012 requires this — without `break`, `loop {}` doesn't
    /// have a success-exit path.
    pub has_break: bool,
    /// Enclosing function's symbol, if known.
    pub function: Option<String>,
    pub span: AirSpan,
}

// ot: canonical
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum LoopKind {
    /// `loop { ... }`
    Loop,
    /// `for x in iter { ... }`
    For,
    /// `while cond { ... }`
    While,
}

/// A literal expression appearing as a *scrutinee* — a match-arm
/// pattern (`match x { "active" => ... }`) or the right-hand side of
/// a binary `==`/`!=` comparison against a non-literal expression
/// (`if role == "admin" { ... }`).
///
/// CF002 (magic decision constants) and CF003 (hardcoded
/// provider/model/topic IDs) consume these to flag string/int
/// literals used as decision keys outside a declared config layer.
/// The visitor records the literal value verbatim so the rule can
/// pattern-match it against a forbidden-value list.
// ot: canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirScrutineeLiteral {
    /// The literal value, rendered as text (`"active"`, `42`,
    /// `3.14`, `true`). String literals retain their surrounding
    /// quotes so callers can distinguish `"42"` from `42`.
    pub value: String,
    /// Type of the literal — string, int, float, or bool.
    pub kind: LiteralKind,
    /// Where the literal appeared. `MatchArm` means it was a pattern
    /// in a match arm; `BinaryCompare` means it was the RHS of a
    /// `==`/`!=` against a path / field / method-call expression.
    pub context: LiteralContext,
    /// Enclosing function's symbol, if known.
    pub function: Option<String>,
    pub span: AirSpan,
}

// ot: canonical
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum LiteralKind {
    Str,
    Int,
    Float,
    Bool,
}

// ot: canonical
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum LiteralContext {
    /// `match x { "active" => ..., }`
    MatchArm,
    /// `if role == "admin" { ... }`, `flag != 0`, …
    BinaryCompare,
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
    /// User-declared fact marker — `// ot: marks <fact_kind>` above a
    /// function tells Locus "treat this function as having `<fact_kind>`."
    /// The `markers` loader translates each `MarksFact` hint into an
    /// `AirFact` targeting the function the hint binds to. Used for
    /// fact kinds the loader tier can't auto-recognise without
    /// framework knowledge (`hot_path`, `request_context`,
    /// `boundary_entry`, `runtime_state_owner`, `background_worker`)
    /// and for letting users annotate their own helpers as carrying
    /// the kinds std-rt only recognises in stdlib (`external_io`,
    /// `persistence_write`, `blocking_call`).
    ///
    /// `fact_kind` carries the snake_case spec name (`"hot_path"`,
    /// `"request_context"`, …) — the loader does the mapping to
    /// [`FactKind`] so unknown markers degrade gracefully (logged but
    /// not promoted to a fact).
    MarksFact {
        fact_kind: String,
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
