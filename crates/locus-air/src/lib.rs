//! Architecture Intermediate Representation.
//!
//! Pure data + serde. No language-specific concerns, no inference logic.
//! Language adapters (e.g. `locus-rust`) build these structures; `locus-core`
//! consumes them. Schema is versioned via [`AirWorkspace::schema_version`] —
//! bump on any breaking field change.
//!
//! Self-application: every AIR type below is `// locus: ot canonical`. They are the
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
///     - `AirItem::PartialResultMatch` — captures `if let Ok(...) = expr { … }`
///       and `if let Err(...) = expr { … }` patterns with **no** `else`
///       branch. The unmatched arm is implicitly silent; FL005 flags this
///       when the file is outside `invariant_owner_paths`.
///
///   Closes the silent-error coverage gap that FL003 (which only sees
///   `.ok()` / `.err()` method-call shape) couldn't reach.
/// - **13**: language-agnostic naming pass. The architectural concepts
///   in AIR were sound but Rust-flavoured names (the previous
///   `EnumMatch`, `PartialIfLet`, `Visibility::Crate`, the
///   `From`/`TryFrom` conversion variants, `derives`/`attrs`,
///   `AirImpl`, `Macro`-as-discriminator) leaked into the schema.
///   v13 is a cosmetic + small-structural pass that removes the Rust
///   bias so a future TS / Python / Go / Swift adapter emits AIR JSON
///   that reads naturally:
///     - Renames (old → new): `ActionKind::EnumMatch` →
///       `DiscriminatedMatch`; `Visibility::Crate` → `Module`;
///       `CallKind::Macro` → `Meta`; `DiscardKind::Macro` → `Meta`;
///       `ArmBodyShape::Propagate` → `ErrorPropagation`;
///       `AirItem::PartialIfLet` → `PartialResultMatch` (with
///       `variant: Success|Failure` enum replacing the previous
///       `String "Ok"|"Err"`); the `ConversionMechanism` variants
///       move from Rust-trait-named `From` / `TryFrom` /
///       `InherentMethod` / `FreeFn` to architectural
///       `InfallibleAdapter` / `FallibleAdapter` / `InstanceMethod`
///       / `FreeFunction`, plus a new `FactoryFunction` variant.
///     - Replaces `AirType.derives` + `AirType.attrs` with a unified
///       `decorators: Vec<AirDecorator>` collection and adds the same
///       to `AirFunction`. Each decorator carries a `source` tag
///       (`Derive` / `Attribute` / `Decorator` / `Annotation`) so
///       per-language adapters can map their own syntax (`#[derive]`
///       vs. `@dataclass` vs. `@Override`) into one shape.
///     - Adds `path_segments: Vec<String>` to `AirImport` and
///       `symbol_segments: Vec<String>` to `AirType` / `AirFunction`
///       so paradigm matchers can operate on segments without
///       splitting `::` themselves (other adapters use `/`, `.`,
///       etc.).
///     - Renames `AirImpl` → `AirImplBlock` with
///       `trait_path → interface`, `self_ty → target_type`, and a
///       new `dispatch: ImplDispatch { Static, Structural, Dynamic }`
///       discriminator. Rust adapter emits `Static` for explicit
///       `impl Trait for Type` and `Dynamic` for `impl dyn Trait` /
///       trait-object boundaries; Go adapter would later emit
///       `Structural` for implicit interface satisfaction.
///     - Adds a `pattern: FallbackPattern { ValueOr, Or, DefaultOr }`
///       field on `AirFallbackCall` so non-Rust adapters can map
///       `??` / `||` / `getOr(...)` to the same architectural
///       shapes; the Rust `unwrap_or` / `or` / `unwrap_or_default`
///       method names are kept on `callee` as evidence.
///
///   `AirRetryLoop` and `AirClosureMethodCall` keep their current
///   shapes — their Rust bias is deep enough that speculative
///   generalisation would produce a worse abstraction than letting
///   future adapters emit parallel items.
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
///   for `// locus: fact <fact_kind>` source hints — the user marks a
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
pub const AIR_SCHEMA_VERSION: u32 = 13;

// locus: ot canonical
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

// locus: ot canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirPackage {
    pub name: String,
    pub version: String,
    pub root_dir: String,
    pub files: Vec<AirFile>,
}

// locus: ot canonical
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

// locus: ot canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum AirItem {
    Type(AirType),
    Function(AirFunction),
    Conversion(AirConversion),
    Usage(AirUsage),
    TruthAction(AirTruthAction),
    Import(AirImport),
    Impl(AirImplBlock),
    CallSite(AirCallSite),
    SilentDiscard(AirSilentDiscard),
    PartialResultMatch(AirPartialResultMatch),
    MatchArm(AirMatchArm),
    ClosureMethodCall(AirClosureMethodCall),
    FallbackCall(AirFallbackCall),
    RetryLoop(AirRetryLoop),
    ScrutineeLiteral(AirScrutineeLiteral),
}

// locus: ot canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirType {
    // Renamed in JSON to avoid colliding with the AirItem external tag (also `kind`).
    #[serde(rename = "type_kind")]
    pub kind: TypeKind,
    pub name: String,
    /// Fully-qualified symbol as the language adapter rendered it
    /// (`pkg::module::Name` for Rust, `pkg/module/Name` for Go,
    /// `pkg.module.Name` for Python, etc.). Opaque text — paradigms
    /// that need segment-level matching should use [`Self::symbol_segments`].
    pub symbol: String,
    /// `symbol` split into segments. Lets paradigm matchers operate
    /// on path components without depending on the language adapter's
    /// delimiter convention. Rust: `["pkg", "module", "Name"]`;
    /// TypeScript: same shape after the adapter splits on `/`;
    /// Python: same shape split on `.`. Empty for adapters that
    /// haven't populated segments yet.
    #[serde(default)]
    pub symbol_segments: Vec<String>,
    pub visibility: Visibility,
    pub fields: Vec<AirField>,
    pub variants: Vec<AirVariant>,
    /// Decorators on this type — `#[derive(...)]` and `#[serde(...)]`
    /// in Rust, `@dataclass` / `@pytest.fixture` in Python, class
    /// decorators in TypeScript, annotations in Java. See
    /// [`AirDecorator`] for the per-source classification.
    #[serde(default)]
    pub decorators: Vec<AirDecorator>,
    pub span: AirSpan,
    /// Joined doc-comment text (`///` and `#[doc = "..."]`), one line per
    /// source comment with the rustdoc-convention single leading space
    /// stripped. `None` when the type has no doc comments. Consumed by the
    /// DC (Documentation) paradigm slice.
    pub doc: Option<String>,
}

// locus: ot canonical
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TypeKind {
    /// Record / product type. Rust `struct`, TS `class`/`interface`-as-shape,
    /// Python `class`/`@dataclass`, Go `struct`, Swift `struct`/`class`.
    Struct,
    /// Sum / discriminated-union type. Rust `enum`, TS discriminated
    /// union, Python `Enum`/`Literal`, Swift `enum`, Java `sealed`.
    Enum,
    /// Type alias. Rust `type X = Y`, TS `type X = Y`, Python type
    /// aliases (`X: TypeAlias = Y`).
    Alias,
    /// Untagged union. Rust `union` (FFI-only); rare in other
    /// languages — TS `A | B` is more like `Enum`. Adapters that
    /// don't have this concept skip it.
    Union,
    /// Method-bag / interface / abstract type. Rust `trait`, TS
    /// `interface`, Python `Protocol`/`abc.ABC`, Go `interface`,
    /// Swift `protocol`, Java `interface`.
    Trait,
}

// locus: ot canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirField {
    pub name: String,
    pub type_text: String,
    pub visibility: Visibility,
}

// locus: ot canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirVariant {
    pub name: String,
    pub fields: Vec<AirField>,
}

/// Visibility of a type / function / field. Renamed in v13 from the
/// Rust-specific `Crate` to a least-common-denominator `Module`:
/// most languages have a "wider than private, narrower than public"
/// tier that maps here (Rust `pub(crate)`, Java package-private,
/// Go uppercase-but-crate-internal-by-convention, TS
/// non-exported-but-module-visible).
// locus: ot canonical
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Visibility {
    /// Visible to all consumers across package/module boundaries.
    Public,
    /// Visible within the current package/crate/module but not
    /// exported. Rust `pub(crate)`, Java package-private,
    /// TypeScript non-`export`'d module locals.
    Module,
    /// Visible to a specific scope narrower than the whole module.
    /// Rust `pub(in path::to)`, Swift `fileprivate`, Java protected.
    Restricted,
    /// Visible only inside the defining type / file. Rust `pub(self)`
    /// or no `pub`, TS `private`, Python `_name` convention, Java `private`.
    Private,
}

// locus: ot canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirFunction {
    pub name: String,
    /// Fully-qualified symbol as the language adapter rendered it.
    /// See [`AirType::symbol`] for delimiter conventions; use
    /// [`Self::symbol_segments`] for portable segment matching.
    pub symbol: String,
    /// `symbol` split into segments — see [`AirType::symbol_segments`].
    #[serde(default)]
    pub symbol_segments: Vec<String>,
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
    /// Decorators on this function — `#[inline]` / `#[test]` in Rust,
    /// `@staticmethod` / `@property` in Python, decorators in TS,
    /// annotations in Java/Kotlin. See [`AirDecorator`].
    #[serde(default)]
    pub decorators: Vec<AirDecorator>,
}

/// A decorator / derive / annotation attached to a type or function.
/// Unifies Rust `#[derive(Foo)]` + `#[serde(rename = "x")]`,
/// TypeScript class decorators, Python `@dataclass` / `@pytest.fixture`,
/// Java/Kotlin annotations, Swift property wrappers — every
/// "metadata attached to a definition" syntax. The `source` tag lets
/// rules that care about a specific syntactic surface (BO004 cares
/// about Rust derives specifically) match against it; rules that
/// just want "any decorator named X" can ignore source.
// locus: ot canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirDecorator {
    pub source: DecoratorSource,
    /// Decorator / derive / annotation name. `Serialize` for
    /// `#[derive(Serialize)]`, `dataclass` for `@dataclass`,
    /// `Override` for `@Override`.
    pub name: String,
    /// Rendered argument text, one entry per top-level argument.
    /// Empty for argument-less decorators. Adapters keep the
    /// rendering consistent with the rest of the AIR's `type_text`
    /// conventions (no extra spaces, `::` for Rust paths).
    #[serde(default)]
    pub args: Vec<String>,
}

// locus: ot canonical
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DecoratorSource {
    /// Rust `#[derive(Foo)]` — implementation generated for the type.
    Derive,
    /// Rust `#[attr(...)]`, Java/Kotlin annotations — metadata that
    /// configures a separate processor (serde, JSON-Schema, etc.).
    Attribute,
    /// TypeScript / JavaScript class & method decorators —
    /// `@Component`, `@Injectable`, `@Get('/path')`.
    Decorator,
    /// Python `@dataclass` / `@cached_property` / `@pytest.fixture` —
    /// callable wrapping the decorated definition.
    Annotation,
}

/// Block of methods declared on a type, optionally implementing an
/// interface. Renamed in v13 from `AirImplBlock` to lift the Rust-only
/// `impl Trait for Type` shape into a language-agnostic
/// "implements interface" / "method bag on type" concept.
// locus: ot canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirImplBlock {
    /// `Some("path::to::Interface")` when this block implements an
    /// interface (Rust `impl Trait for Type`, TS `class X implements I`,
    /// Java `class X implements I`, Python `class X(Protocol)`,
    /// Swift `extension X: I`). `None` for inherent / non-conforming
    /// method bags.
    #[serde(default)]
    pub interface: Option<String>,
    /// The type this block adds methods to. Same convention as
    /// [`AirType::symbol`] for delimiter handling.
    pub target_type: String,
    /// Names of methods declared inside the block, in declaration order.
    /// Empty for empty blocks.
    pub method_names: Vec<String>,
    /// How the implementation is *bound* to the interface. Rust
    /// `impl Trait for Type` is `Static`; Rust trait objects
    /// (`Box<dyn Trait>`) and Java reflection-bound impls are
    /// `Dynamic`; Go's implicit interface satisfaction (a struct
    /// "implements" an interface by having the method set without
    /// declaring the relationship) is `Structural`. Rust adapter
    /// always emits `Static` today.
    #[serde(default = "default_impl_dispatch")]
    pub dispatch: ImplDispatch,
    pub span: AirSpan,
}

fn default_impl_dispatch() -> ImplDispatch {
    ImplDispatch::Static
}

// locus: ot canonical
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ImplDispatch {
    /// Static / explicit conformance. Rust `impl Trait for Type`,
    /// TS `class X implements I`, Java `implements`, Swift
    /// `extension X: Protocol`.
    Static,
    /// Implicit / structural conformance — the type satisfies the
    /// interface by having the right method set, without declaring
    /// the relationship. Go interfaces, TypeScript structural
    /// typing, Python duck-typing.
    Structural,
    /// Late / runtime-bound dispatch — Rust `dyn Trait`, Java
    /// `Class.cast`, Python ABC virtual subclassing.
    Dynamic,
}

// locus: ot canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirConversion {
    pub from: String,
    pub to: String,
    pub mechanism: ConversionMechanism,
    pub symbol: String,
    pub span: AirSpan,
}

/// How a type-to-type conversion is wired. Renamed in v13 from the
/// Rust-trait-name-shaped variants (`From`/`TryFrom`) to language-
/// agnostic categories. Adapters map their idioms here:
///
/// - **InfallibleAdapter**: Rust `impl From<A> for B`, TS `(a: A): B`,
///   Python `def __init__(self, a: A)` for total construction.
/// - **FallibleAdapter**: Rust `impl TryFrom<A>`, TS `(a: A): B | null`,
///   Python `@classmethod def try_from(cls, a)` returning `Optional`.
/// - **InstanceMethod**: Rust inherent `impl B { fn from_a(...) }`,
///   TS class method, Python instance method.
/// - **FreeFunction**: Rust free `fn map_a_to_b(a: A) -> B`, TS
///   module-level function, Go package-level function.
/// - **FactoryFunction**: a free / static factory whose name is a
///   convention (`X::new`, `X.create`, `make_x`). Adapters that
///   want to distinguish factory functions from arbitrary free
///   functions emit this variant; otherwise they fall back to
///   `FreeFunction`.
// locus: ot canonical
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConversionMechanism {
    InfallibleAdapter,
    FallibleAdapter,
    InstanceMethod,
    FreeFunction,
    FactoryFunction,
}

// locus: ot canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirUsage {
    pub from_symbol: String,
    pub to_symbol: String,
    #[serde(rename = "usage_kind")]
    pub kind: UsageKind,
    pub span: AirSpan,
}

// locus: ot canonical
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum UsageKind {
    FunctionParam,
    FunctionReturn,
    FieldType,
}

// locus: ot canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirTruthAction {
    pub action: ActionKind,
    pub target: String,
    pub function: Option<String>,
    pub span: AirSpan,
    pub confidence: f32,
    pub reasons: Vec<String>,
}

/// Architectural shape of a "decision-like" action inside a function
/// body. Renamed in v13 from the Rust-syntax-shaped `EnumMatch` to
/// `DiscriminatedMatch` so other adapters (TS discriminated-union
/// `switch`, Python `match` on dataclasses, Go type-switch, Swift
/// `enum` match) emit naturally.
// locus: ot canonical
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    /// Constructing a new value of a domain-shaped type. Rust
    /// `Type { ... }` literal, TS `new Type(...)`, Python `Type(...)`.
    Construct,
    /// Dispatching on a discriminator/tag — Rust `match`, TS
    /// `switch (x.kind)`, Python `match x:`, Go type-switch,
    /// Swift `switch` on an enum.
    DiscriminatedMatch,
    /// Comparing a value against a string literal — `if role ==
    /// "admin"` and similar.
    StringCompare,
    /// Validation-like operation — `if !is_valid(x) { return ... }`,
    /// `assert!(x.starts_with("..."))`, `raise ValueError`.
    Validate,
    /// Normalisation-like operation — `x.trim().to_lowercase()`,
    /// `x.replace(...)`, canonicalising input.
    Normalize,
}

// locus: ot canonical
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

// locus: ot canonical
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum CallKind {
    /// Free / standalone function call (`foo(args)` where `foo` is
    /// a path or named function reference).
    Function,
    /// Method call on a receiver (`x.foo(args)`). Receiver-type
    /// resolution is out of AIR's scope.
    Method,
    /// Meta / syntactic / definition-time call surface. Rust macros
    /// (`println!`, `vec![]`), TypeScript template-tag invocations
    /// (``html`<div/>` ``), Python decorator calls evaluated at
    /// definition time, Java reflective method invocations.
    /// Renamed in v13 from `Macro` to lift the Rust-specific shape.
    Meta,
}

/// `let _ = expr;` — a discarded binding. Captured only when `expr` is a
/// call shape (`Method` / `Function` / `Macro`); arbitrary discarded
/// expressions (`let _ = some_field;` / `let _ = Block { ... };`) are
/// recorded with `kind = Other` and a `None` callee so FL004 can choose
/// to ignore them by default.
// locus: ot canonical
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

// locus: ot canonical
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DiscardKind {
    /// `let _ = receiver.method(...);` — method call on a receiver.
    Method,
    /// `let _ = function(...);` — free function call.
    Function,
    /// `let _ = macro!(...);` — syntactic / meta call surface.
    /// Renamed in v13 from `Macro` (consistent with [`CallKind::Meta`]).
    Meta,
    /// `let _ = some_other_expr;` — block, field access, literal, etc.
    /// Recorded for completeness; FL004 defaults to ignoring this kind
    /// because the false-positive surface is too large.
    Other,
}

/// A partial match against the `Result`-shape: only the success or
/// only the failure branch is handled, with no `else` / no companion
/// arm. The unmatched side falls through silently. FL005 fires on
/// this shape outside `invariant_owner_paths`.
///
/// Each language adapter emits this for its own `Result`-equivalent
/// pattern: Rust `if let Ok/Err(...) = expr { ... }` (no else),
/// TypeScript `if (result.ok) { ... }` (no else), Python `if
/// result.is_ok(): ...` (no else), Go `if err == nil { ... }` (no
/// else handling the err). Renamed in v13 from `AirPartialResultMatch` to
/// lift the Rust-only `if let` shape into a language-agnostic
/// "partial result match" concept.
// locus: ot canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirPartialResultMatch {
    /// Which branch of the `Result` shape *was* handled. The
    /// unmatched complement is the silent path. Renamed in v13 from
    /// the previous `String "Ok"|"Err"` to a typed enum so consumers
    /// don't have to do string compares.
    pub variant: ResultMatchVariant,
    /// Enclosing function's symbol, if known.
    pub function: Option<String>,
    pub span: AirSpan,
}

// locus: ot canonical
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ResultMatchVariant {
    /// The success branch was handled — Rust `Ok(x)`, TS
    /// `result.ok`, Python `result.is_ok()`, Go `err == nil`.
    /// The implicit failure branch is silent.
    Success,
    /// The failure branch was handled — Rust `Err(e)`, TS
    /// `!result.ok`, Python `result.is_err()`, Go `err != nil`.
    /// The implicit success branch is silent.
    Failure,
}

/// One arm of a `match` expression. The visitor emits one
/// `AirItem::MatchArm` per arm so paradigm rules can reason about the
/// shape of each arm's body — specifically whether a `Result`-shape arm
/// silently swallows the unmatched case.
// locus: ot canonical
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
// locus: ot canonical
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
    /// The arm propagates an error to the caller — Rust `?`,
    /// TypeScript `try { … }` rethrow, Python `raise`, Go's
    /// `if err != nil { return err }` early-exit shape. The
    /// opposite of silent. Renamed in v13 from `Propagate` to make
    /// the cross-language meaning explicit.
    ErrorPropagation,
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
// locus: ot canonical
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
// locus: ot canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirFallbackCall {
    /// Architectural classification of the fallback shape — added in
    /// v13 so non-Rust adapters can map their idioms here without
    /// inventing fake Rust method names. TS `??` / `||`, Go's
    /// two-value `if !ok { default }`, Python `value or default`
    /// all map to one of [`FallbackPattern::ValueOr`] /
    /// [`FallbackPattern::Or`] / [`FallbackPattern::DefaultOr`].
    pub pattern: FallbackPattern,
    /// Original callee text, kept as evidence the rule can quote.
    /// Rust adapter populates with `unwrap_or` / `or` /
    /// `unwrap_or_default`; TypeScript adapter would populate with
    /// `??` / `||` / a project-specific `getOr`.
    pub callee: String,
    /// Heuristic shape of the first-argument default expression.
    /// `unwrap_or_default` (no argument) — and equivalents — are
    /// recorded with `default_shape = Empty`.
    pub default_shape: ArmBodyShape,
    /// Enclosing function's symbol, if known.
    pub function: Option<String>,
    pub span: AirSpan,
}

/// Architectural shape of a fallback / value-or-default operation.
// locus: ot canonical
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum FallbackPattern {
    /// Value-or-default with an explicit default expression. Rust
    /// `result.unwrap_or(0)` / `option.unwrap_or(...)`, TS
    /// `result ?? default`, Python `value if value is not None
    /// else default`.
    ValueOr,
    /// Either-or: try the first; if it fails, fall through to the
    /// second. Rust `option.or(other)` / `result.or(...)`, TS
    /// `result || alternate`, Python `value or alternate`.
    Or,
    /// Default-of-type fallback — no explicit default, the type's
    /// default value is used. Rust `unwrap_or_default()`, Python
    /// `dict.setdefault(key)` patterns, TS spread-default
    /// `{...defaults, ...x}`.
    DefaultOr,
}

/// A loop construct (`loop {}`, `for ... {}`, `while ... {}`) whose
/// body contains both error propagation (`?`) and an explicit `break`.
/// FL012 fires on this shape — it's the visitor's structural signal
/// for "retry without accepted policy": something fallible is being
/// repeated until it succeeds, with no declared retry policy or
/// backoff strategy.
// locus: ot canonical
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

// locus: ot canonical
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
// locus: ot canonical
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

// locus: ot canonical
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum LiteralKind {
    Str,
    Int,
    Float,
    Bool,
}

// locus: ot canonical
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum LiteralContext {
    /// `match x { "active" => ..., }`
    MatchArm,
    /// `if role == "admin" { ... }`, `flag != 0`, …
    BinaryCompare,
}

// locus: ot canonical
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
// locus: ot canonical
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

// locus: ot canonical
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

// locus: ot canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirImport {
    /// Fully-rendered import path as the language adapter wrote it.
    /// Rust `use foo::bar::Baz` → `"foo::bar::Baz"`. TypeScript
    /// `import { Baz } from "./foo/bar"` → `"./foo/bar/Baz"` or
    /// equivalent adapter convention. Python `from foo.bar import
    /// Baz` → `"foo.bar.Baz"`. `use a::{b, c}` is flattened: each
    /// leaf becomes its own AirImport. Leading `crate::` (Rust) is
    /// normalised to the package's lib name so paths are consistent
    /// with [`AirType::symbol`].
    ///
    /// Opaque text — paradigm matchers that need delimiter-agnostic
    /// segment matching should use [`Self::path_segments`].
    pub path: String,
    /// `path` split into segments. Lets paradigm matchers operate
    /// on path components (`["foo", "bar", "Baz"]`) without
    /// depending on the language adapter's delimiter convention.
    /// Empty for adapters that haven't populated segments yet.
    #[serde(default)]
    pub path_segments: Vec<String>,
    pub visibility: Visibility,
    pub span: AirSpan,
}

// locus: ot canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirHint {
    pub kind: HintKind,
    pub raw: String,
    pub span: AirSpan,
    pub target_span: Option<AirSpan>,
}

// locus: ot canonical
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
    /// User-declared fact marker — `// locus: fact <fact_kind>` above a
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

// locus: ot canonical
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
