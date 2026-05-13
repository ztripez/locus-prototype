# Rust adapter: semantic boundary

This document is the explicit contract for what the Rust adapter
(`crates/locus-rust`) understands and what it deliberately does not.
It exists because the adapter walks a real `syn` AST (not regex
parsing) but several later steps still operate on rendered strings
or shallow name-shape heuristics — that creates a false-confidence
risk for paradigm rules built on top.

If you are writing a new rule and the spec says "this is a converter"
or "this function spawns work," use this doc to check whether the
adapter can actually tell. Source of truth for the file/line references
below is the code itself; treat anything contradicted by source as a
doc bug and fix it.

Issue: [#110](https://github.com/ztripez/locus/issues/110).

## The four layers

Each layer has a different semantic guarantee. A fact's reliability is
capped by the weakest layer in its derivation chain.

### Layer 1 — raw source scan (no AST)

**File:** `crates/locus-rust/src/hints.rs` —
`scan_hints(source, file_path) -> Vec<AirHint>` (line 25).

`syn` strips comments, so `// locus:` annotations are read directly
from the source string before parsing. Each hint binds to the next
non-blank, non-comment, non-attribute line; that line number becomes
the hint's `target_span`. Multi-line raw strings (`r#"..."#`) are
detected so hints appearing inside string literals (e.g. inside
`indoc!` blocks in this crate's own tests) are not promoted.

Supported hint forms (kept in sync with `HintKind` in
`crates/locus-air/src/lib.rs`):

```
// locus: ot canonical
// locus: ot boundary <concept> <boundary>
// locus: ot converter
// locus: ot protocol-translation reason="..."
// locus: ot generated-boundary
// locus: allow <RULE> reason="..." expires="YYYY-MM-DD"
// locus: fact <fact_kind>
```

**What this layer does not do:** it has zero AST awareness. It cannot
tell whether the next line is actually a struct, a function, or a
random expression. It also does not track which item the hint refers
to beyond "the next visible line." If you put a hint inside a function
body or between two attributes, the binding may surprise you.

### Layer 2 — `syn` AST scan

**File:** `crates/locus-rust/src/visitor.rs` —
`collect_items(file, file_path, module) -> Vec<AirItem>` (line 31).

This is the real Rust parser. `syn::parse_file` produces a typed AST;
the visitor walks `syn::Item` / `syn::ImplItem` / `syn::Expr` and emits
`AirItem` variants. Per-variant emitters:

| AirItem variant     | Emitter (in `visitor.rs`)                            |
|---------------------|------------------------------------------------------|
| `Type` (struct/…)   | `emit_struct`, `emit_enum`, `emit_union`, `emit_alias` |
| `Function`          | `emit_fn` → `emit_fn_air_function`                   |
| `Conversion`        | `emit_fn_converter`, `emit_impl_trait_conversion`, `emit_impl_inherent_conversions` |
| `ImplBlock`         | `emit_impl_impl_block`                               |
| `Import`            | `emit_use`                                           |
| `CallSite`          | `scan_fn_body_for_truth_actions`                     |
| `SilentDiscard`     | let-binding scanner                                  |

This layer's guarantee is **syntactic**: an `AirItem::Type` with
`kind: TypeKind::Struct` really is a `struct` in the source. The
visitor is also where `parse_error` is set when `syn::parse_file`
fails; the scanner emits an `AirFile` with empty `items` and the
error message instead of aborting the whole workspace scan.

**What this layer does not do:** name resolution. The visitor cannot
tell that `Result` in `fn f() -> Result<T, E>` refers to
`std::result::Result` rather than a locally imported alias. Macros are
observed (their *invocation* shows up as an `AirCallSite` with
`CallKind::Meta`) but not **expanded** — you cannot reason about what
code a `tracing::instrument` or `tokio::main` actually emits.

### Layer 3 — rendered AIR text

**File:** `crates/locus-rust/src/type_render.rs` — `render_type` (line
20) and `render_path` (line 27).

Type paths and symbols are rendered to strings (`Vec<Result<User,
Error>>`, `pkg::module::Type`) for storage in AIR. Whitespace is
normalised so `Result < User , Error >` becomes `Result<User, Error>`,
keeping textual equality stable across formatting.

**What is lost in rendering:**

- **Lifetimes** are rendered literally (`&'a User`) but not separately
  tracked. Two distinct lifetime parameters collapse to the same
  string if the user named them identically.
- **Generics** are expanded inline; there is no parameter-vs-argument
  distinction at the AIR level. Generic bounds and `where` clauses are
  syntax-only.
- **Associated types** render as `Trait::Assoc` but the mapping is
  textual; no equality with the resolved concrete type is computed.
- **Hygiene** is not tracked; macro-generated identifiers appear as
  whatever spelling reached the renderer.

If a paradigm rule needs to compare two `AirType` symbols, it is
comparing strings, not types. That works for the cases AIR was
designed for (canonical-domain ownership, dependency graphs, file-
level budgets) and breaks if you stretch it (e.g. "is this `T` the
same type as the one declared in module X" — no, that question is
outside this adapter's scope).

### Layer 4 — heuristic inference

This is the most dangerous layer for new rules because the AIR fact
looks the same regardless of how it was produced. The heuristics in
the Rust adapter today:

**Converter detection by name shape** (`visitor.rs`):

- Free function: `fn to_*` / `fn from_*` / `fn into_*` / `fn map_*` /
  `fn convert_*` → `ConversionMechanism::FreeFunction` (line 548).
- Inherent method: `fn to_*` / `fn into_*` → `InstanceMethod` (line 561).
- Trait impl: text-equality on the last path segment of the impl's
  trait — `From` or `TryFrom` only (line 327).

The visitor does not check that a function's argument or return type
actually matches the "from / to" relationship implied by the name. For
free functions it requires exactly one parameter and a return type;
for inherent methods it requires `to_*` / `into_*` with a return type
distinct from `Self`. Within those guards, an inherent method like
`fn to_inert(&self) -> ()` whose return type is unrelated to any real
conversion is still emitted as an `AirConversion(from=Self, to=())`.

**`Result` / `Option` unwrapping** (`visitor.rs` line 580,
`strip_result_or_option`):

```rust
fn strip_result_or_option(ty: &str) -> Option<String> { /* … */ }
```

This operates on the *rendered string*. It returns the first comma-
separated type inside the outermost `Result<…>` or `Option<…>`. Nested
generics (`Result<Option<User>, Error>`) return the intermediate
wrapper (`Option<User>`), not the innermost type. There is no rustc-
backed type stripping; if the source uses a type alias
(`type MyResult<T> = Result<T, MyError>`), it is not seen as a
`Result`.

**Module-path inference** (`module_path.rs` lines 21–51,
`derive_module_path`):

The module path of a file is inferred from its **filesystem location**
relative to `src/`. `#[path = "..."]` overrides are not honoured;
`#[cfg(...)]`-gated modules are walked unconditionally. The result is
correct for idiomatic Rust layouts and wrong for crates that
deliberately rearrange their module tree.

**Import normalisation** (`visitor.rs` lines 444–452,
`normalize_use_path`):

`crate::X` is rewritten to `<lib_crate_name>::X` so AIR symbols are
comparable across files. `self::` and `super::` are left literal —
resolving them requires knowing the full module chain, which the
adapter does not reconstruct. Re-exports (`pub use a::b::C`) are
emitted as imports but not followed.

**Standard-library fact recognition** (loader, see below): produced
from path-segment string matching, not receiver-type checking. See
"loaders" section for the specific recognition patterns.

## Loaders

Loaders run after `collect_items` and translate call-site / hint
patterns into `AirFact` entries that paradigm rules consume.

### `StdRtLoader`

**File:** `crates/locus-rust/src/loaders/std_rt.rs`.

Maps callee path strings to `FactKind`:

| Pattern                                    | `FactKind`           |
|--------------------------------------------|----------------------|
| `*::spawn`                                 | `SpawnedWork`        |
| `*::env::var`, `*::env::var_os`            | `ConfigRead`         |
| `std::fs::write|create_dir|…`              | `PersistenceWrite`   |
| `std::thread::sleep|park`                  | `BlockingCall`       |
| `std::process::Command::*`                 | `ExternalIo` (+ `BlockingCall` on `output`/`status`) |
| `TcpStream::*`, `print!` / `println!`, …   | `ExternalIo` / `Logging` |

**Confidence is hardcoded at 0.9** — these are syntactic recognitions,
not verified facts. Method calls (`CallKind::Method`) are skipped
entirely (visitor.rs line 214) because the receiver type is unknown
without name resolution, so `my_executor.spawn(…)` does not match the
`*::spawn` pattern.

A function named `custom_spawn::do_work` *will* match `*::spawn`
because the matcher walks path segments. This is a known false
positive surface and is one of the things the issue #110 documentation
exists to make explicit.

### `MarkersLoader`

**File:** `crates/locus-rust/src/loaders/markers.rs`.

Promotes `// locus: fact <fact_kind>` Layer-1 hints into `AirFact`
entries by binding each hint's `target_span` to the enclosing function.
Hardcoded confidence 1.0 — user annotations are authoritative within
the markers loader's scope. Unknown fact kinds are silently dropped
(see `parse_fact_kind`).

## Rule-author checklist

Before adding a new rule under `crates/locus-core/src/paradigms/`,
walk through these four questions. Each `Yes` raises the bar for what
the adapter must do — and may mean the rule is out of scope for the
current adapter.

1. **Does this rule need only syntactic facts?**
   `AirItem::Type`, `AirItem::Function::line_count`, presence of an
   `// locus:` hint, raw call-site path text. ✅ Layer 1+2 deliver
   these directly; no further work needed.

2. **Does this rule need name resolution?**
   "Is this `Result` the stdlib `Result`?" — "Does this `User` refer
   to the canonical `domain::User`?" The adapter does **not** resolve
   names. Workarounds: require an explicit `// locus:` hint, or write
   the rule against the AIR-rendered symbol and accept the
   string-comparison limitations spelled out in Layer 3.

3. **Does this rule need type resolution?**
   "Is this function's return type a `Future`?" — "Does this argument
   implement `Send`?" The adapter does **not** run rustc. If your rule
   depends on this, either:
   - find a syntactic proxy (e.g. presence of `async` keyword in the
     function signature, which is captured),
   - require the user to mark the function with `// locus: fact …`,
   - or document the rule as advisory-only and tag the fact's
     confidence accordingly (future work — see "Future work" below).

4. **Does this rule need macro expansion?**
   "Does this `#[tokio::main]` spawn a runtime?" — "What does
   `tracing::instrument` actually log?" The adapter observes macro
   *invocations* (as `AirCallSite` with `CallKind::Meta`) but does
   not expand them. Reject the rule at design time or require markers.

If you answered Yes to (2), (3), or (4), call it out in the rule's
spec entry under `docs/PARADIGMS.md` so future readers know the rule
inherits a known semantic gap.

## What the boundary tests pin

`crates/locus-rust/tests/semantic_boundary.rs` is a test corpus that
exercises the four layers and locks in which cases are supported and
which are intentionally unresolved. If you change adapter behaviour
and one of those tests fails, update both the test and this document
together — the test's job is to keep the doc honest.

The tests cover, at minimum:

- Layer 1: hints survive comment-stripping, attach to the next line,
  ignore raw-string content.
- Layer 2: `syn::parse_file` errors are surfaced as `parse_error`
  rather than aborting the whole scan.
- Layer 3: type rendering normalises whitespace and is stable across
  reorderings the language treats as equivalent.
- Layer 4 (false-positive surface): a function named `to_string` that
  returns `()` is still classified as a converter; a custom
  `my_module::spawn` matches the std-rt `*::spawn` recognition.

These are the *known* false-positive shapes — they are tested to make
sure the behaviour stays bounded, not because the heuristics are
correct in those cases.

## Future work

These are out of scope for the document but recorded so they are not
forgotten:

- **Confidence / evidence tags on AIR facts.** Today loader-produced
  facts carry a hardcoded confidence (0.9 for std-rt, 1.0 for
  markers). A future schema bump could expose this on every
  `AirFact` and let rules consume it (e.g. demoting a Layer-4-only
  finding to Advisory). Tracked separately from #110.

- **Rustc-backed adapter mode.** A second adapter mode that runs
  inside a rustc plugin / driver would lift the Layer-4 ceiling for
  rules that need real type and macro information. Explicitly listed
  as a non-goal in #110 until the syntactic adapter boundary is
  documented and exhausted.

- **`#[path = "..."]` and `#[cfg]` module overrides.** Currently
  `derive_module_path` infers from the filesystem only. Real support
  would require either parsing the module attribute or asking cargo
  for the resolved module tree.
