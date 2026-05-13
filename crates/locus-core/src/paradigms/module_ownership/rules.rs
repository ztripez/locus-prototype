//! MO rule implementations.
//!
//! Implemented:
//! - [`mo001`]: too many public top-level types in a single file.
//! - [`mo002`]: responsibility entropy in a single file (canonical/boundary/
//!   converter hints, handler-named functions, persistence imports, io call
//!   sites — too many distinct architectural roles co-existing).
//! - [`mo003`]: canonical hint co-located with a boundary hint in the same file.
//! - [`mo004`]: canonical hint co-located with a handler-named function in the
//!   same file.
//! - [`mo005`]: entrypoint modules (`main.rs`, `mod.rs`, `lib.rs`) contain
//!   type declarations, impl blocks, or substantial functions — forbidden
//!   because entrypoint modules are composition surfaces, not ownership
//!   sites. `lib.rs` is classified by either an explicit
//!   `paradigms.MO.lib_rs_kinds` lockfile entry or a built-in heuristic
//!   (see [`mo005`] doc comment).

use locus_air::{
    AirFile, AirHint, AirImport, AirItem, AirSpan, AirWorkspace, HintKind, Visibility,
};

use super::lockfile_schema::{LibRsKind, MoSection, matches_name_glob, matches_pattern};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

fn mo001_why(
    module_path: &str,
    count: u32,
    budget: u32,
    default_budget: u32,
    matched_override: Option<&super::lockfile_schema::MoOverride>,
    section: &MoSection,
) -> Vec<String> {
    let mut why = vec![
        format!("file `{module_path}` defines {count} public top-level type(s)"),
        if let Some(o) = matched_override {
            format!("budget {budget} from override `module = {}`", o.module)
        } else {
            format!("budget {budget} (workspace default)")
        },
    ];
    if matched_override.is_none() && section.default_max_public_types.is_none() {
        why.push(format!(
            "no `default_max_public_types` configured; using built-in fallback {default_budget}",
        ));
    }
    why
}

#[allow(clippy::too_many_arguments)]
fn mo001_diagnostic(
    module_path: &str,
    count: u32,
    budget: u32,
    default_budget: u32,
    span: AirSpan,
    matched_override: Option<&super::lockfile_schema::MoOverride>,
    section: &MoSection,
    mode: CheckMode,
) -> Diagnostic {
    Diagnostic {
        rule_id: "MO001".to_string(),
        severity: mode.elevate(Severity::Warning),
        span,
        concept: None,
        message: format!(
            "module `{module_path}` has {count} public top-level types (budget {budget})"
        ),
        why: mo001_why(
            module_path,
            count,
            budget,
            default_budget,
            matched_override,
            section,
        ),
        suggested_fix: Some(
            "split the module into submodules each owning one architectural role, \
             or — if this density is intended (e.g. an API surface) — raise the \
             budget by adding an override to `paradigms.MO.overrides` in \
             `.locus/lock.json`"
                .into(),
        ),
    }
}

/// MO001 — module file has too many public top-level types.
///
/// For each `AirFile` with a `module_path`, count `AirItem::Type` items
/// whose visibility is `Public`. Compare against the file's effective
/// budget (override wins, then default, then built-in fallback).
/// Fires when `count > budget`. One diagnostic per file.
///
/// Severity: Warning by default; `--agent-strict` elevates to Fatal.
/// Fires by default on un-onboarded code using built-in fallback budgets.
pub fn mo001(air: &AirWorkspace, section: &MoSection, mode: CheckMode) -> Vec<Diagnostic> {
    let default_budget = section.effective_default();
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            let count = file
                .items
                .iter()
                .filter(
                    |item| matches!(item, AirItem::Type(t) if t.visibility == Visibility::Public),
                )
                .count() as u32;
            let matched_override = section.matching_override(module_path);
            let budget = matched_override
                .map(|o| o.max_public_types)
                .unwrap_or(default_budget);
            if count <= budget {
                continue;
            }
            // Anchor at the first public type, or line 1 of the file.
            let span = file
                .items
                .iter()
                .find_map(|item| match item {
                    AirItem::Type(t) if t.visibility == Visibility::Public => Some(t.span.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| locus_air::AirSpan::new(file.path.clone(), 1, 1));
            out.push(mo001_diagnostic(
                module_path,
                count,
                budget,
                default_budget,
                span,
                matched_override,
                section,
                mode,
            ));
        }
    }
    out
}

/// In-code default callee patterns flagged as "io" by MO002. Kept as a
/// constant rather than a lockfile field because MO002's spec only enumerates
/// `entropy_threshold`, `handler_name_patterns`, and `persistence_import_patterns`
/// as configurable surface — the io contributor is a built-in heuristic, not
/// user policy. If a project legitimately makes io calls in a non-blob file,
/// the noise is absorbed by the entropy threshold (count must reach 3+).
const IO_CALLEE_PATTERNS: &[&str] = &[
    "*::fs::*",
    "*::net::*",
    "*::TcpStream::*",
    "*::TcpListener::*",
    "*::UdpSocket::*",
];

/// Anchor a per-file diagnostic at a useful span: the first item in the
/// file when present, otherwise line 1 of the file.
fn file_anchor_span(file: &AirFile) -> AirSpan {
    file.items
        .iter()
        .map(|item| match item {
            AirItem::Type(t) => t.span.clone(),
            AirItem::Function(f) => f.span.clone(),
            AirItem::Conversion(c) => c.span.clone(),
            AirItem::Import(i) => i.span.clone(),
            AirItem::Impl(i) => i.span.clone(),
            AirItem::TruthAction(a) => a.span.clone(),
            AirItem::CallSite(c) => c.span.clone(),
            AirItem::Usage(u) => u.span.clone(),
            AirItem::SilentDiscard(d) => d.span.clone(),
            AirItem::PartialResultMatch(p) => p.span.clone(),
            AirItem::MatchArm(a) => a.span.clone(),
            AirItem::ClosureMethodCall(c) => c.span.clone(),
            AirItem::FallbackCall(c) => c.span.clone(),
            AirItem::RetryLoop(l) => l.span.clone(),
            AirItem::ScrutineeLiteral(l) => l.span.clone(),
        })
        .next()
        .unwrap_or_else(|| AirSpan::new(file.path.clone(), 1, 1))
}

fn has_canonical_hint(file: &AirFile) -> bool {
    file.hints
        .iter()
        .any(|h| matches!(h.kind, HintKind::Canonical))
}

fn has_boundary_hint(file: &AirFile) -> bool {
    file.hints
        .iter()
        .any(|h| matches!(h.kind, HintKind::Boundary { .. }))
}

fn has_converter_hint(file: &AirFile) -> bool {
    file.hints
        .iter()
        .any(|h| matches!(h.kind, HintKind::Converter))
}

fn has_handler_named_function(file: &AirFile, patterns: &[&str]) -> bool {
    file.items.iter().any(|item| {
        let AirItem::Function(f) = item else {
            return false;
        };
        patterns.iter().any(|p| matches_name_glob(p, &f.name))
    })
}

fn has_persistence_import(file: &AirFile, patterns: &[&str]) -> bool {
    file.items.iter().any(|item| {
        let AirItem::Import(AirImport { path, .. }) = item else {
            return false;
        };
        patterns.iter().any(|p| matches_pattern(p, path))
    })
}

fn has_io_call_site(file: &AirFile) -> bool {
    file.items.iter().any(|item| {
        let AirItem::CallSite(c) = item else {
            return false;
        };
        IO_CALLEE_PATTERNS
            .iter()
            .any(|p| matches_pattern(p, &c.callee))
    })
}

fn mo002_diagnostic(
    module_path: &str,
    count: u32,
    threshold: u32,
    role_list: &str,
    span: AirSpan,
    mode: CheckMode,
) -> Diagnostic {
    Diagnostic {
        rule_id: "MO002".to_string(),
        severity: mode.elevate(Severity::Warning),
        span,
        concept: None,
        message: format!(
            "module `{module_path}` carries {count} distinct architectural roles \
             ({role_list}); threshold is {threshold}"
        ),
        why: vec![
            format!("file `{module_path}` exhibits roles: {role_list}"),
            format!(
                "MO002 entropy threshold is {threshold} (configured via \
                 `paradigms.MO.entropy_threshold` in `.locus/lock.json`)"
            ),
            "a single file mixing canonical/boundary/converter/handler/persistence/io \
             roles is a responsibility blob — split each role into its own module"
                .into(),
        ],
        suggested_fix: Some(
            "split this file along role boundaries: canonical types into \
             `domain/`, boundary DTOs into `dto/`, conversions into a \
             `convert.rs`, handlers into a `handlers/` module, and \
             persistence/io into an adapter layer. If the density is \
             intentional, raise `paradigms.MO.entropy_threshold` in \
             `.locus/lock.json`."
                .into(),
        ),
    }
}

/// MO002 — responsibility entropy in a single file.
///
/// Counts how many of the six architectural roles a file carries:
/// canonical hint, boundary hint, converter hint, handler-named function,
/// persistence import, or IO call site. Fires when `count >= threshold`
/// (default 3).
///
/// Severity: Warning by default; `--agent-strict` elevates to Fatal.
/// Fires by default with built-in fallback threshold and pattern lists.
pub fn mo002(air: &AirWorkspace, section: &MoSection, mode: CheckMode) -> Vec<Diagnostic> {
    let threshold = section.effective_entropy_threshold();
    let handler_patterns = section.effective_handler_name_patterns();
    let persistence_patterns = section.effective_persistence_import_patterns();
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            let mut roles: Vec<&'static str> = Vec::new();
            if has_canonical_hint(file) {
                roles.push("canonical");
            }
            if has_boundary_hint(file) {
                roles.push("boundary");
            }
            if has_converter_hint(file) {
                roles.push("converter");
            }
            if has_handler_named_function(file, &handler_patterns) {
                roles.push("handler");
            }
            if has_persistence_import(file, &persistence_patterns) {
                roles.push("persistence");
            }
            if has_io_call_site(file) {
                roles.push("io");
            }
            let count = roles.len() as u32;
            if count < threshold {
                continue;
            }
            let span = file_anchor_span(file);
            let role_list = roles.join(", ");
            out.push(mo002_diagnostic(
                module_path,
                count,
                threshold,
                &role_list,
                span,
                mode,
            ));
        }
    }
    out
}

fn mo003_diagnostic(module_path: &str, span: AirSpan, mode: CheckMode) -> Diagnostic {
    Diagnostic {
        rule_id: "MO003".to_string(),
        severity: mode.elevate(Severity::Warning),
        span,
        concept: None,
        message: format!("module `{module_path}` mixes canonical and boundary types"),
        why: vec![
            format!(
                "file `{module_path}` has both a `// locus: ot canonical` \
                 and a `// locus: ot boundary` hint"
            ),
            "canonical types are the domain truth; boundary types are the \
             wire/protocol shadow of that truth — keeping them in one file \
             blurs ownership and makes the converter direction ambiguous"
                .into(),
        ],
        suggested_fix: Some(
            "split the file: move canonical types into a `domain/` module \
             and boundary types into a `dto/` module, with explicit \
             `From`/`TryFrom` converters between them"
                .into(),
        ),
    }
}

/// MO003 — canonical type co-located with a boundary type in the same file.
///
/// Fires for any `AirFile` containing both an `AirHint::Canonical` and an
/// `AirHint::Boundary`. The two hints describe opposing roles — canonical
/// types are the domain truth; boundary types are the wire/protocol shadow
/// of that truth — so co-locating them in one file blurs ownership.
///
/// Severity: Warning by default; `--agent-strict` elevates to Fatal. No
/// new lockfile fields — the rule is a pure structural check on hints.
pub fn mo003(air: &AirWorkspace, mode: CheckMode) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            if !(has_canonical_hint(file) && has_boundary_hint(file)) {
                continue;
            }
            let span = file
                .hints
                .iter()
                .find(|h| matches!(h.kind, HintKind::Canonical))
                .map(|h: &AirHint| h.span.clone())
                .unwrap_or_else(|| file_anchor_span(file));
            out.push(mo003_diagnostic(module_path, span, mode));
        }
    }
    out
}

fn mo004_diagnostic(
    module_path: &str,
    handler: &locus_air::AirFunction,
    handler_patterns: &[&str],
    mode: CheckMode,
) -> Diagnostic {
    Diagnostic {
        rule_id: "MO004".to_string(),
        severity: mode.elevate(Severity::Warning),
        span: handler.span.clone(),
        concept: None,
        message: format!(
            "module `{module_path}` co-locates handler `{}` with a canonical concept",
            handler.name
        ),
        why: vec![
            format!("file `{module_path}` has a `// locus: ot canonical` hint"),
            format!(
                "function `{}` matches handler name pattern (one of {handler_patterns:?})",
                handler.name,
            ),
            "handlers belong to an application/transport layer; canonical \
             types belong to the domain layer — co-locating them couples \
             the two and makes the canonical reusable from non-handler \
             callers harder"
                .into(),
        ],
        suggested_fix: Some(format!(
            "move `{}` into a `handlers/` module that depends on the \
             canonical, instead of defining both in the same file. If the \
             name match is a false positive, narrow \
             `paradigms.MO.handler_name_patterns` in `.locus/lock.json`.",
            handler.name
        )),
    }
}

/// MO004 — handler co-located with a canonical concept in the same file.
///
/// Fires for any `AirFile` containing both an `AirHint::Canonical` *and* a
/// function whose name matches `handler_name_patterns` (reuses the same
/// patterns as MO002, with the same default fallback `*_handler`/`handle_*`).
///
/// Handlers belong to an application/transport layer; canonical types belong
/// to the domain layer. Co-locating them couples the two.
///
/// Severity: Warning by default; `--agent-strict` elevates to Fatal.
pub fn mo004(air: &AirWorkspace, section: &MoSection, mode: CheckMode) -> Vec<Diagnostic> {
    let handler_patterns = section.effective_handler_name_patterns();
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            if !has_canonical_hint(file) {
                continue;
            }
            let handler = file.items.iter().find_map(|item| {
                let AirItem::Function(f) = item else {
                    return None;
                };
                if handler_patterns
                    .iter()
                    .any(|p| matches_name_glob(p, &f.name))
                {
                    Some(f)
                } else {
                    None
                }
            });
            let Some(handler) = handler else {
                continue;
            };
            out.push(mo004_diagnostic(
                module_path,
                handler,
                &handler_patterns,
                mode,
            ));
        }
    }
    out
}

/// Line-count budget for "thin" functions that MO005 permits in entrypoint
/// modules. A `main`, `run`, or `init` function up to this many lines is
/// accepted as composition glue. A function exceeding this limit is
/// substantial enough that it belongs in a dedicated module.
///
/// 25 lines is deliberate: it accommodates `fn main() -> Result<()>` that
/// parses args and dispatches a single `run()` call, plus small helpers like
/// `fn run(cli: Cli) -> Result<()> { commands::run(cli) }`, while still
/// flagging multi-branch dispatch bodies that belong in a `commands/` module.
pub const MO005_THIN_FN_MAX_LINES: u32 = 25;

/// Function names that MO005 treats as composition glue and therefore
/// exempts from the line-count check **in addition to** the thin-function
/// budget. An entrypoint module may contain any number of these functions
/// provided each is individually below `MO005_THIN_FN_MAX_LINES` lines.
///
/// `main` / `run` / `init` are the conventional Rust entrypoint names.
/// `setup` / `start` appear in test harnesses and integration crates.
const ENTRYPOINT_FN_NAMES: &[&str] = &["main", "run", "init", "setup", "start"];

/// Identifies whether an entrypoint module is a binary root (`main.rs`),
/// a directory module root (`mod.rs`), or a library crate root (`lib.rs`).
///
/// The distinction matters for two reasons:
///
/// 1. The composition-host exception (unit struct + thin `impl Paradigm`)
///    applies only to `mod.rs` files, where it is the deliberate
///    architectural convention. `main.rs` (binary entrypoints) must have
///    zero impl blocks regardless of trait, target, or method size.
///
/// 2. `lib.rs` is the crate's public API surface — fundamentally different
///    from `main.rs`. A `lib.rs` is classified into one of three canonical
///    shapes (thin re-export / canonical-data / composition root) by an
///    explicit `paradigms.MO.lib_rs_kinds` lockfile entry, or by a built-in
///    heuristic when no entry matches. See [`mo005`] for the full table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EntrypointKind {
    /// Binary-crate entrypoint (`main.rs` or module path ending in `::main`).
    Main,
    /// Directory-module root (`mod.rs` or module path ending in `::mod`).
    Mod,
    /// Library-crate root (`lib.rs`). Cargo gives `lib.rs` a flat
    /// `module_path` equal to the crate's lib name (e.g. `locus_air`)
    /// with no `::lib` suffix, so this kind is detected from the file
    /// basename only.
    LibRs,
}

/// Check whether a file is an entrypoint module based on its module path
/// and file path, and return the `EntrypointKind` when it is.
///
/// Returns `None` when the file is not an entrypoint module.
///
/// Detection uses two complementary signals:
///
/// 1. **Module-path suffix** — the last segment of `module_path` is compared
///    against `ENTRYPOINT_SUFFIXES`. Catches `my_crate::commands::mod` as a
///    directory-module entrypoint and `my_crate::main` as a binary entrypoint.
///
/// 2. **File-path basename** — the file's OS path (when provided) is also
///    checked. This catches the common case in Rust where binary-crate
///    `main.rs` produces a flat `module_path` equal to just the crate name
///    (e.g. `locus_cli` rather than `locus_cli::main`). Likewise `mod.rs`
///    files inside subdirectories have module paths like `pkg::commands`,
///    not `pkg::commands::mod`.
///
/// Both checks recognise `main` / `mod` / `lib` so the semantics are
/// symmetric — either the logical name or the filesystem name can trigger
/// the rule. `lib.rs` is detected from the file basename only (Cargo emits
/// a flat `module_path` for the lib root with no `::lib` suffix).
fn entrypoint_kind(module_path: &str, file_path: &str) -> Option<EntrypointKind> {
    // Check 1: last module_path segment ("main" / "mod" only; "lib" is
    // never a module-path suffix in Rust).
    let last_segment = module_path.rsplit("::").next().unwrap_or(module_path);
    if let Some(kind) = segment_to_entrypoint_kind(last_segment) {
        return Some(kind);
    }
    // Check 2: file basename stem (e.g. "main" from ".../src/main.rs",
    // "lib" from ".../src/lib.rs").
    let stem = std::path::Path::new(file_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    segment_to_entrypoint_kind(stem)
}

fn segment_to_entrypoint_kind(segment: &str) -> Option<EntrypointKind> {
    match segment {
        "main" => Some(EntrypointKind::Main),
        "mod" => Some(EntrypointKind::Mod),
        "lib" => Some(EntrypointKind::LibRs),
        _ => None,
    }
}

/// Total impl-block line budget for the composition-host exception in
/// entrypoint `mod.rs` files.
///
/// `AirImplBlock` carries the names of methods but not per-method line counts.
/// The impl block's span (line_end − line_start + 1) is used as a proxy: if
/// the entire block fits within this budget, every method inside it is
/// necessarily thin (≤30 lines each, even with 4–5 methods and boilerplate).
///
/// 120 lines is chosen to comfortably accommodate the largest real composition-
/// host impl blocks observed during dogfooding (~60 lines) while still flagging
/// impls that contain substantial business logic. The observed maximum across
/// all Locus paradigm host impls is ~60 lines; 120 gives 2× headroom.
pub const MO005_COMPOSITION_HOST_IMPL_MAX_LINES: u32 = 120;

/// Public-declaration budget for the lib.rs composition-root heuristic.
///
/// A `lib.rs` with both re-export weight (`pub use` imports) AND public
/// substantial declarations is treated as a composition root. The
/// heuristic stays silent when the public-declaration count is at or below
/// this budget — locus-rust's lib.rs (1 `pub enum` + 3 `pub fn`) sits at
/// 4, so 5 gives a one-step headroom for natural growth before the rule
/// fires.
///
/// Above the budget, the file is treated as an accidental god module and
/// every public declaration is flagged. The user can either refactor or
/// declare an explicit `lib_rs_kinds` entry in `.locus/lock.json` with
/// the `composition-root` kind to silence the rule with debt metadata.
///
/// Aligned with `DEFAULT_MAX_PUBLIC_TYPES` (5) from MO001's section so the
/// two budgets evolve together — a crate hitting the lib.rs composition-
/// root budget is also at the per-module public-type budget.
pub const LIB_RS_COMPOSITION_ROOT_DECL_BUDGET: u32 = 5;

/// Heuristic counts for a `lib.rs` file: `(reexport_weight, public_decls,
/// any_decl)`. See [`resolve_lib_rs_kind`] for how each count is used.
struct LibRsShapeStats {
    /// Count of public `AirImport` items (Rust `pub use`). Drives the
    /// `R == 0` canonical-data signal.
    reexport_weight: u32,
    /// Count of substantial public declarations: public types, public
    /// functions, public conversions, and impl blocks (impls don't carry
    /// their own visibility, so any impl contributes).
    public_decls: u32,
    /// `true` when the file contains any substantial declaration at all
    /// (regardless of visibility) — the thin-reexport signal flips on
    /// `any_decl == false`.
    any_decl: bool,
}

fn lib_rs_shape_stats(file: &AirFile) -> LibRsShapeStats {
    let mut stats = LibRsShapeStats {
        reexport_weight: 0,
        public_decls: 0,
        any_decl: false,
    };
    for item in &file.items {
        match item {
            AirItem::Import(imp) if imp.visibility == Visibility::Public => {
                stats.reexport_weight += 1;
            }
            AirItem::Type(t) => {
                stats.any_decl = true;
                if t.visibility == Visibility::Public {
                    stats.public_decls += 1;
                }
            }
            // Impls don't carry their own visibility — every impl
            // contributes toward `public_decls` so an impl-heavy file
            // still trips the composition-root budget.
            AirItem::Impl(_) | AirItem::Conversion(_) => {
                stats.any_decl = true;
                stats.public_decls += 1;
            }
            AirItem::Function(f) => {
                stats.any_decl = true;
                if f.visibility == Visibility::Public {
                    // Thin or not, public fns are part of the crate's
                    // exposed surface and count toward the budget.
                    stats.public_decls += 1;
                }
            }
            _ => {}
        }
    }
    stats
}

/// Resolve the effective `LibRsKind` for a lib.rs file.
///
/// Precedence:
/// 1. Explicit lockfile entry in `paradigms.MO.lib_rs_kinds` matching the
///    file's `module_path` — returned verbatim.
/// 2. Heuristic — returns `Some(CanonicalData)` when the file has no
///    re-export weight (zero `pub use` imports) and at least one
///    substantial declaration; `Some(CompositionRoot)` when there is
///    re-export weight AND the public-declaration count is at or below
///    [`LIB_RS_COMPOSITION_ROOT_DECL_BUDGET`]; `None` when neither shape
///    applies (treated as `ThinReexport` — main.rs scoping enforced).
///
/// `None` from this function means "apply MO005 with full main.rs-style
/// declaration prohibitions." Returning a `Some` variant means MO005
/// should skip declaration checks on this file.
fn resolve_lib_rs_kind(file: &AirFile, section: &MoSection) -> Option<LibRsKind> {
    let module_path = file.module_path.as_deref().unwrap_or("");
    if let Some(entry) = section.lib_rs_kind_for(module_path) {
        return Some(entry.kind);
    }
    let stats = lib_rs_shape_stats(file);
    if !stats.any_decl {
        // Thin re-export shape — no declarations at all.
        return None;
    }
    if stats.reexport_weight == 0 {
        // Canonical-data shape — declarations without any `pub use` wiring.
        return Some(LibRsKind::CanonicalData);
    }
    if stats.public_decls <= LIB_RS_COMPOSITION_ROOT_DECL_BUDGET {
        // Small composition root — declarations + wiring below the
        // god-module threshold.
        return Some(LibRsKind::CompositionRoot);
    }
    // Above the budget with re-export weight present — accidental god
    // module accumulating in lib.rs.
    None
}

/// Trait name suffix that identifies the known composition trait.
///
/// Matched by suffix so both `Paradigm` and `crate::paradigms::Paradigm` (or
/// any module-qualified form) resolve to the same check. If another trait named
/// `Paradigm` exists in the workspace, that is an acceptable false-negative
/// risk for this first pass.
const COMPOSITION_TRAIT_NAME: &str = "Paradigm";

/// Returns `true` when `impl_block` matches the composition-host pattern in a
/// `mod.rs` entrypoint:
///
/// 1. The impl block implements the known composition trait (`Paradigm`).
/// 2. The target type is a local unit struct (zero fields) declared in
///    `same_file_items`.
/// 3. The total impl block span is ≤ `MO005_COMPOSITION_HOST_IMPL_MAX_LINES`.
///
/// All three conditions must hold; if any fails the impl block is still
/// flagged by MO005.
fn is_composition_host_impl(
    impl_block: &locus_air::AirImplBlock,
    same_file_items: &[AirItem],
) -> bool {
    // Condition 1: implements the Paradigm trait.
    let Some(interface) = &impl_block.interface else {
        return false;
    };
    let implements_paradigm = interface == COMPOSITION_TRAIT_NAME
        || interface.ends_with(&format!("::{COMPOSITION_TRAIT_NAME}"));
    if !implements_paradigm {
        return false;
    }

    // Condition 2: target type is a local unit struct in the same module.
    let target = &impl_block.target_type;
    let is_local_unit_struct = same_file_items.iter().any(|item| {
        let AirItem::Type(t) = item else {
            return false;
        };
        t.kind == locus_air::TypeKind::Struct
            && t.fields.is_empty()
            && (t.name == *target
                || t.symbol == *target
                || t.symbol.ends_with(&format!("::{target}")))
    });
    if !is_local_unit_struct {
        return false;
    }

    // Condition 3: total impl block is within the thin-methods budget.
    let impl_lines = impl_block
        .span
        .line_end
        .saturating_sub(impl_block.span.line_start)
        + 1;
    impl_lines <= MO005_COMPOSITION_HOST_IMPL_MAX_LINES
}

/// Returns `true` when `type_item` is the host unit struct paired with a
/// composition-host impl in the same file.
///
/// The host struct is allowed (not flagged) when its matching `impl Paradigm`
/// is allowed. This prevents the struct itself from triggering MO005 even
/// though it is a type declaration in an entrypoint module.
fn is_composition_host_struct(type_item: &locus_air::AirType, same_file_items: &[AirItem]) -> bool {
    // Must be a unit struct (no fields).
    if type_item.kind != locus_air::TypeKind::Struct || !type_item.fields.is_empty() {
        return false;
    }
    // There must be a composition-host impl targeting this struct in the same file.
    same_file_items.iter().any(|item| {
        let AirItem::Impl(impl_block) = item else {
            return false;
        };
        let target = &impl_block.target_type;
        let names_match = *target == type_item.name
            || *target == type_item.symbol
            || target.ends_with(&format!("::{}", type_item.name));
        names_match && is_composition_host_impl(impl_block, same_file_items)
    })
}

/// Classify an `AirItem` as allowed or forbidden in an entrypoint module.
///
/// `same_file_items` is the full item list from the file being checked; it is
/// needed to evaluate the composition-host exception (the host struct and its
/// `impl Paradigm` are validated in terms of each other).
///
/// `kind` identifies whether the entrypoint is `main.rs` or `mod.rs`. The
/// composition-host exception (unit struct + thin `impl Paradigm`) is only
/// valid in `mod.rs` files — `main.rs` is the binary entrypoint and must have
/// zero impl blocks regardless of trait, target, or method size.
///
/// Returns `None` when the item is permitted; returns `Some(reason)` when
/// it is a forbidden declaration that MO005 should flag.
/// Classify a `Type` item in an entrypoint module.
fn classify_type_item(
    t: &locus_air::AirType,
    same_file_items: &[AirItem],
    kind: EntrypointKind,
) -> Option<String> {
    // The composition-host exception applies only in mod.rs files.
    if kind == EntrypointKind::Mod && is_composition_host_struct(t, same_file_items) {
        return None;
    }
    let kind_str = match t.kind {
        locus_air::TypeKind::Struct => "struct",
        locus_air::TypeKind::Enum => "enum",
        locus_air::TypeKind::Trait => "trait",
        locus_air::TypeKind::Alias => "type alias",
        locus_air::TypeKind::Union => "union",
    };
    Some(format!(
        "{kind_str} `{}` declared in entrypoint module — move to a sibling module",
        t.name
    ))
}

/// Classify an `Impl` item in an entrypoint module.
fn classify_impl_item(
    i: &locus_air::AirImplBlock,
    same_file_items: &[AirItem],
    kind: EntrypointKind,
) -> Option<String> {
    // Allow the composition-host impl in mod.rs only.
    if kind == EntrypointKind::Mod && is_composition_host_impl(i, same_file_items) {
        return None;
    }
    let target = &i.target_type;
    Some(format!(
        "impl block for `{target}` in entrypoint module — move to the module that owns `{target}`"
    ))
}

/// Classify a `Function` item in an entrypoint module.
fn classify_function_item(f: &locus_air::AirFunction) -> Option<String> {
    let is_permitted_name = ENTRYPOINT_FN_NAMES.contains(&f.name.as_str());
    let within_budget = f.line_count <= MO005_THIN_FN_MAX_LINES;
    if is_permitted_name && within_budget {
        return None; // thin composition-glue function: allowed
    }
    if is_permitted_name {
        Some(format!(
            "function `{}` in entrypoint module spans {} lines (budget {}); \
             move its body into a dedicated module",
            f.name, f.line_count, MO005_THIN_FN_MAX_LINES
        ))
    } else {
        Some(format!(
            "function `{}` in entrypoint module is not composition glue; \
             move it to a sibling module (e.g. `commands/`, `routes/`, \
             `handlers/`)",
            f.name
        ))
    }
}

fn mo005_classify_item(
    item: &AirItem,
    same_file_items: &[AirItem],
    kind: EntrypointKind,
) -> Option<String> {
    match item {
        AirItem::Type(t) => classify_type_item(t, same_file_items, kind),
        AirItem::Impl(i) => classify_impl_item(i, same_file_items, kind),
        AirItem::Function(f) => classify_function_item(f),
        AirItem::Conversion(c) => Some(format!(
            "converter `{}` declared in entrypoint module — move to a `convert.rs` \
             or the owning domain module",
            c.symbol
        )),
        // All other item kinds — imports, hints, facts, call-sites, etc.
        // — are passive observations, not declarations. Permitted.
        _ => None,
    }
}

/// MO005 — entrypoint modules must be composition surfaces, not ownership
/// sites.
///
/// Fires for every `AirItem` in a file whose `module_path` ends in `::main`
/// or `::mod`, or whose file's basename is `main.rs`/`mod.rs`/`lib.rs`,
/// that is a type declaration, impl block, converter, or a function that
/// is not a thin composition-glue wrapper (≤25 lines named `main`/`run`/
/// `init`/`setup`/`start`).
///
/// **Allowed in entrypoint modules:**
/// - `mod` declarations (not captured in AIR at the item level)
/// - imports (`AirItem::Import`)
/// - crate-level doc attrs / hints
/// - thin `fn main` / `fn run` / `fn init` (≤ `MO005_THIN_FN_MAX_LINES` lines)
/// - **composition-host pair** in `mod.rs`: a unit struct + `impl Paradigm`
///   where all methods are thin (total impl block ≤ `MO005_COMPOSITION_HOST_IMPL_MAX_LINES`
///   lines). This encodes the deliberate architectural convention that every
///   Locus paradigm module has exactly one host struct + `impl Paradigm` in its
///   `mod.rs`. The exception is structural: it fires only when the impl
///   implements the known `Paradigm` trait, the target is a local unit struct,
///   and the whole block is thin.
///
/// **Forbidden:**
/// - struct / enum / trait / alias / union declarations (except the host unit struct)
/// - impl blocks (except the composition-host `impl Paradigm` in `mod.rs`)
/// - converter declarations
/// - functions not named `main`/`run`/`init`/`setup`/`start`
/// - functions whose line count exceeds the budget
///
/// ## `lib.rs` classification
///
/// `lib.rs` covers four distinct architectural shapes and the rule
/// distinguishes them. The effective shape is taken from
/// `paradigms.MO.lib_rs_kinds` when an entry matches the file's
/// `module_path`; otherwise a heuristic infers it from the AIR items:
///
/// | Shape              | Heuristic signal                                 | MO005 behavior                              |
/// |--------------------|--------------------------------------------------|---------------------------------------------|
/// | thin re-export     | zero substantial declarations                    | passes silently (no items to flag)          |
/// | canonical-data     | substantial declarations AND zero `pub use`      | skips MO005 (file IS the data contract)     |
/// | composition root   | re-export weight AND public decls ≤ budget       | skips MO005 (small wiring + glue allowed)   |
/// | accidental god mod | re-export weight AND public decls > budget       | flags each substantial declaration          |
///
/// An explicit `paradigms.MO.lib_rs_kinds` entry takes precedence over the
/// heuristic. The supported kinds are `thin-reexport` (enforce main.rs
/// scoping), `canonical-data` (skip the file entirely), and
/// `composition-root` (skip the file entirely; rely on MO001/MO002 for
/// god-module signals). The budget is
/// [`LIB_RS_COMPOSITION_ROOT_DECL_BUDGET`] (5).
///
/// **Rationale:** entrypoint modules are the last place agents look when
/// adding new behavior, so they accumulate it. Enforcing that they contain
/// only composition glue keeps the dependency tree legible and prevents
/// god-module accumulation at the crate root.
///
/// Severity: Warning by default; `--agent-strict` elevates to Fatal.
///
/// Exemption hierarchy: explicit `paradigms.MO.lib_rs_kinds` lockfile entry
/// (lib.rs only), then `// locus: allow MO005` source-hint, then the
/// built-in lib.rs heuristic.
fn mo005_item_span(item: &AirItem, file_path: &str) -> AirSpan {
    match item {
        AirItem::Type(t) => t.span.clone(),
        AirItem::Function(f) => f.span.clone(),
        AirItem::Impl(i) => i.span.clone(),
        AirItem::Conversion(c) => c.span.clone(),
        _ => AirSpan::new(file_path.to_string(), 1, 1),
    }
}

fn mo005_lib_rs_suggested_fix(module_path: &str) -> String {
    format!(
        "move this declaration into a dedicated sibling module, or — \
         if this density is intentional (e.g. a canonical-data crate \
         surface like locus-air, or an integration crate's composition \
         root) — declare it in `.locus/lock.json` with \
         `paradigms.MO.lib_rs_kinds = [{{ module: \"{module_path}\", \
         kind: \"canonical-data\" | \"composition-root\", reason, \
         expires, owner }}]`"
    )
}

const MO005_ENTRYPOINT_PRINCIPLE: &str = "entrypoint modules are composition surfaces — they wire \
     modules together via `mod` declarations, imports, and thin \
     `main`/`run`/`init` functions. Substantial declarations \
     belong in dedicated sibling modules.";

const MO005_LIB_RS_GOD_MODULE_NOTE: &str = "treated as `thin-reexport`: re-export weight is present \
     AND the public-declaration count exceeds the lib.rs \
     composition-root budget. Either refactor or declare an \
     explicit `paradigms.MO.lib_rs_kinds` entry.";

const MO005_DEFAULT_SUGGESTED_FIX: &str = "move this declaration into a dedicated sibling module \
     (e.g. `cli.rs` for the root arg struct, `commands/` for \
     command implementations). Entrypoint should contain only \
     `mod` decls, imports, and a thin `main` or `run` function.";

/// Emit MO005 diagnostics for forbidden items in a single entrypoint file.
fn mo005_check_file(
    file: &locus_air::AirFile,
    module_path: &str,
    ep_kind: EntrypointKind,
    file_label: &str,
    mode: CheckMode,
    out: &mut Vec<Diagnostic>,
) {
    for item in &file.items {
        let Some(reason) = mo005_classify_item(item, &file.items, ep_kind) else {
            continue;
        };
        let span = mo005_item_span(item, &file.path);
        let mut why = vec![
            format!(
                "`{file_label}` (module `{module_path}`) is an entrypoint \
                 module — it must be a composition surface, not an ownership site"
            ),
            MO005_ENTRYPOINT_PRINCIPLE.into(),
        ];
        let suggested_fix = if ep_kind == EntrypointKind::LibRs {
            why.push(MO005_LIB_RS_GOD_MODULE_NOTE.into());
            Some(mo005_lib_rs_suggested_fix(module_path))
        } else {
            Some(MO005_DEFAULT_SUGGESTED_FIX.into())
        };
        out.push(Diagnostic {
            rule_id: "MO005".to_string(),
            severity: mode.elevate(Severity::Warning),
            span,
            concept: None,
            message: format!("MO005: {reason}"),
            why,
            suggested_fix,
        });
    }
}

pub fn mo005(air: &AirWorkspace, section: &MoSection, mode: CheckMode) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            let Some(ep_kind) = entrypoint_kind(module_path, &file.path) else {
                continue;
            };
            // lib.rs gets classified by lockfile + heuristic; the
            // `CanonicalData` and `CompositionRoot` shapes skip the rule
            // entirely. Returning `None` from `resolve_lib_rs_kind` means
            // "treat as ThinReexport" and apply main.rs-style scoping.
            if ep_kind == EntrypointKind::LibRs
                && let Some(kind) = resolve_lib_rs_kind(file, section)
                && matches!(kind, LibRsKind::CanonicalData | LibRsKind::CompositionRoot)
            {
                continue;
            }
            // Prefer the file basename (e.g. "main.rs") for the diagnostic.
            let file_label = std::path::Path::new(&file.path)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(&file.path);
            mo005_check_file(file, module_path, ep_kind, file_label, mode, &mut out);
        }
    }
    out
}

// ── RuleDefinition impls (governance spine migration, epic #71) ──────────────

use crate::governance::finding::{FindingSource, RuleFinding};
use crate::governance::ids::{ParadigmId, RuleId};
use crate::governance::rule::{RuleContext, RuleDefinition};

const MO_PARADIGM: ParadigmId = ParadigmId::new("MO");
const MO001_ID: RuleId = RuleId::new("MO001");
const MO002_ID: RuleId = RuleId::new("MO002");
const MO003_ID: RuleId = RuleId::new("MO003");
const MO004_ID: RuleId = RuleId::new("MO004");
const MO005_ID: RuleId = RuleId::new("MO005");

pub struct Mo001Rule;
pub static MO001_RULE: Mo001Rule = Mo001Rule;

impl RuleDefinition for Mo001Rule {
    fn id(&self) -> RuleId {
        MO001_ID
    }
    fn paradigm(&self) -> ParadigmId {
        MO_PARADIGM
    }
    fn title(&self) -> &'static str {
        "module has too many public types"
    }
    fn default_severity(&self) -> crate::diagnostics::Severity {
        crate::diagnostics::Severity::Warning
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        use super::lockfile_schema::MoSection;
        let section: MoSection = ctx.lockfile.paradigm_section("MO").unwrap_or_default();
        mo001(ctx.air, &section, ctx.mode)
            .into_iter()
            .map(|d| RuleFinding {
                id: ctx.finding_ids.next(),
                source: FindingSource::RegisteredRule(MO001_ID),
                rule_id: Some(MO001_ID),
                paradigm_id: Some(MO_PARADIGM),
                default_severity: d.severity,
                span: Some(d.span),
                concept: d.concept,
                message: d.message,
                evidence: vec![],
                why: d.why,
                suggested_fix: d.suggested_fix,
                diagnostic_code: None,
            })
            .collect()
    }
}

pub struct Mo002Rule;
pub static MO002_RULE: Mo002Rule = Mo002Rule;

impl RuleDefinition for Mo002Rule {
    fn id(&self) -> RuleId {
        MO002_ID
    }
    fn paradigm(&self) -> ParadigmId {
        MO_PARADIGM
    }
    fn title(&self) -> &'static str {
        "module carries too many architectural roles"
    }
    fn default_severity(&self) -> crate::diagnostics::Severity {
        crate::diagnostics::Severity::Warning
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        use super::lockfile_schema::MoSection;
        let section: MoSection = ctx.lockfile.paradigm_section("MO").unwrap_or_default();
        mo002(ctx.air, &section, ctx.mode)
            .into_iter()
            .map(|d| RuleFinding {
                id: ctx.finding_ids.next(),
                source: FindingSource::RegisteredRule(MO002_ID),
                rule_id: Some(MO002_ID),
                paradigm_id: Some(MO_PARADIGM),
                default_severity: d.severity,
                span: Some(d.span),
                concept: d.concept,
                message: d.message,
                evidence: vec![],
                why: d.why,
                suggested_fix: d.suggested_fix,
                diagnostic_code: None,
            })
            .collect()
    }
}

pub struct Mo003Rule;
pub static MO003_RULE: Mo003Rule = Mo003Rule;

impl RuleDefinition for Mo003Rule {
    fn id(&self) -> RuleId {
        MO003_ID
    }
    fn paradigm(&self) -> ParadigmId {
        MO_PARADIGM
    }
    fn title(&self) -> &'static str {
        "canonical and boundary hints co-located in same module"
    }
    fn default_severity(&self) -> crate::diagnostics::Severity {
        crate::diagnostics::Severity::Warning
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        mo003(ctx.air, ctx.mode)
            .into_iter()
            .map(|d| RuleFinding {
                id: ctx.finding_ids.next(),
                source: FindingSource::RegisteredRule(MO003_ID),
                rule_id: Some(MO003_ID),
                paradigm_id: Some(MO_PARADIGM),
                default_severity: d.severity,
                span: Some(d.span),
                concept: d.concept,
                message: d.message,
                evidence: vec![],
                why: d.why,
                suggested_fix: d.suggested_fix,
                diagnostic_code: None,
            })
            .collect()
    }
}

pub struct Mo004Rule;
pub static MO004_RULE: Mo004Rule = Mo004Rule;

impl RuleDefinition for Mo004Rule {
    fn id(&self) -> RuleId {
        MO004_ID
    }
    fn paradigm(&self) -> ParadigmId {
        MO_PARADIGM
    }
    fn title(&self) -> &'static str {
        "handler co-located with canonical concept"
    }
    fn default_severity(&self) -> crate::diagnostics::Severity {
        crate::diagnostics::Severity::Warning
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        use super::lockfile_schema::MoSection;
        let section: MoSection = ctx.lockfile.paradigm_section("MO").unwrap_or_default();
        mo004(ctx.air, &section, ctx.mode)
            .into_iter()
            .map(|d| RuleFinding {
                id: ctx.finding_ids.next(),
                source: FindingSource::RegisteredRule(MO004_ID),
                rule_id: Some(MO004_ID),
                paradigm_id: Some(MO_PARADIGM),
                default_severity: d.severity,
                span: Some(d.span),
                concept: d.concept,
                message: d.message,
                evidence: vec![],
                why: d.why,
                suggested_fix: d.suggested_fix,
                diagnostic_code: None,
            })
            .collect()
    }
}

pub struct Mo005Rule;
pub static MO005_RULE: Mo005Rule = Mo005Rule;

impl RuleDefinition for Mo005Rule {
    fn id(&self) -> RuleId {
        MO005_ID
    }
    fn paradigm(&self) -> ParadigmId {
        MO_PARADIGM
    }
    fn title(&self) -> &'static str {
        "substantial declaration in entrypoint module"
    }
    fn default_severity(&self) -> crate::diagnostics::Severity {
        crate::diagnostics::Severity::Warning
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        use super::lockfile_schema::MoSection;
        let section: MoSection = ctx.lockfile.paradigm_section("MO").unwrap_or_default();
        mo005(ctx.air, &section, ctx.mode)
            .into_iter()
            .map(|d| RuleFinding {
                id: ctx.finding_ids.next(),
                source: FindingSource::RegisteredRule(MO005_ID),
                rule_id: Some(MO005_ID),
                paradigm_id: Some(MO_PARADIGM),
                default_severity: d.severity,
                span: Some(d.span),
                concept: d.concept,
                message: d.message,
                evidence: vec![],
                why: d.why,
                suggested_fix: d.suggested_fix,
                diagnostic_code: None,
            })
            .collect()
    }
}

#[cfg(test)]
#[path = "rules_tests.rs"]
mod rules_tests;
