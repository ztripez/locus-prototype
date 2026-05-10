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
//! - [`mo005`]: entrypoint modules (`main.rs`, `mod.rs`) contain type
//!   declarations, impl blocks, or substantial functions — forbidden because
//!   entrypoint modules are composition surfaces, not ownership sites.
//!   `lib.rs` is out of scope in this first pass (see follow-up issue).

use locus_air::{
    AirFile, AirHint, AirImport, AirItem, AirSpan, AirWorkspace, HintKind, Visibility,
};

use super::lockfile_schema::{MoSection, matches_name_glob, matches_pattern};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

/// MO001 — module file has too many public top-level types.
///
/// For each `AirFile` with a `module_path`, count `AirItem::Type` items
/// whose visibility is `Public`. Compare against the file's effective
/// budget:
/// - if the file's `module_path` matches an override's `module` pattern,
///   the override's `max_public_types` wins;
/// - otherwise the section's `default_max_public_types` (or the constant
///   fallback) is used.
///
/// One diagnostic per file (not per type) — the violation is the file's
/// responsibility, not any individual type.
///
/// Severity: Warning by default. `--agent-strict` elevates to Fatal via
/// [`CheckMode::elevate`].
///
/// Fires by default — the section's built-in fallback budget is treated
/// as real configuration. Configuration narrows: users raise the budget
/// on legitimately-broad modules via `paradigms.MO.overrides`, or replace
/// the workspace default via `default_max_public_types`. Add the prefix
/// to `acknowledged_empty` to silence the paradigm entirely.
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

            // Anchor the diagnostic at the file's first public type when
            // possible — otherwise at line 1 of the file. Either way, the
            // diagnostic is per-file, not per-type.
            let span = file
                .items
                .iter()
                .find_map(|item| match item {
                    AirItem::Type(t) if t.visibility == Visibility::Public => Some(t.span.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| locus_air::AirSpan::new(file.path.clone(), 1, 1));

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
                    "no `default_max_public_types` configured; using built-in fallback {}",
                    default_budget
                ));
            }

            out.push(Diagnostic {
                rule_id: "MO001".to_string(),
                severity: mode.elevate(Severity::Warning),
                span,
                concept: None,
                message: format!(
                    "module `{module_path}` has {count} public top-level types (budget {budget})"
                ),
                why,
                suggested_fix: Some(
                    "split the module into submodules each owning one architectural role, \
                     or — if this density is intended (e.g. an API surface) — raise the \
                     budget by adding an override to `paradigms.MO.overrides` in \
                     `locus.lock`"
                        .into(),
                ),
            });
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

/// MO002 — responsibility entropy in a single file.
///
/// Counts the number of distinct architectural roles a file carries:
/// (a) `AirHint::Canonical` present, (b) `AirHint::Boundary` present,
/// (c) `AirHint::Converter` present, (d) any function whose name matches
/// `handler_name_patterns` (default `*_handler`/`handle_*`), (e) any
/// `AirImport.path` matching `persistence_import_patterns`, (f) any
/// `AirItem::CallSite.callee` matching the built-in io pattern set.
///
/// Fires when the count `>= entropy_threshold` (default 3).
///
/// Severity: Warning by default; `--agent-strict` elevates to Fatal.
///
/// Fires by default — the section's built-in fallback threshold and
/// pattern lists are treated as real configuration. Configuration
/// narrows: users widen the entropy budget via `entropy_threshold`,
/// override per-module via `overrides`, or add the prefix to
/// `acknowledged_empty` to silence the paradigm entirely.
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
            out.push(Diagnostic {
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
                         `paradigms.MO.entropy_threshold` in `locus.lock`)"
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
                     `locus.lock`."
                        .into(),
                ),
            });
        }
    }
    out
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
            out.push(Diagnostic {
                rule_id: "MO003".to_string(),
                severity: mode.elevate(Severity::Warning),
                span,
                concept: None,
                message: format!(
                    "module `{module_path}` mixes canonical and boundary types"
                ),
                why: vec![
                    format!("file `{module_path}` has both a `// locus: ot canonical` and a `// locus: ot boundary` hint"),
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
            });
        }
    }
    out
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
            out.push(Diagnostic {
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
                        "function `{}` matches handler name pattern (one of {:?})",
                        handler.name, handler_patterns
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
                     `paradigms.MO.handler_name_patterns` in `locus.lock`.",
                    handler.name
                )),
            });
        }
    }
    out
}

/// Entrypoint-module names that MO005 applies to.
///
/// A file's `module_path` last segment is compared against this set.
/// - `main` covers `src/main.rs` (Rust binary-crate convention).
/// - `mod` covers `<dir>/mod.rs` (directory sub-module root).
///
/// `lib` (`src/lib.rs`) is intentionally out of scope for this first pass.
/// lib.rs covers multiple distinct architectural shapes — thin re-export
/// surface, canonical-data crate surface (e.g. `locus-air` where every
/// `AirItem`/`AirType`/etc. is intentional public API), composition root,
/// and accidental god module — that require their own design pass before
/// MO005 can apply meaningfully. See follow-up issue for lib.rs entrypoint
/// handling.
const ENTRYPOINT_SUFFIXES: &[&str] = &["main", "mod"];

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

/// Check whether a file is an entrypoint module based on its module path
/// and file path.
///
/// Detection uses two complementary signals:
///
/// 1. **Module-path suffix** — the last segment of `module_path` is compared
///    against `ENTRYPOINT_SUFFIXES`. Catches `my_crate::commands::mod` and
///    `my_crate::other_main` as entrypoints because they explicitly use the
///    canonical names.
///
/// 2. **File-path basename** — the file's OS path (when provided) is also
///    checked. This catches the common case in Rust where binary-crate
///    `main.rs` produces a flat `module_path` equal to just the crate name
///    (e.g. `locus_cli` rather than `locus_cli::main`). Likewise `mod.rs`
///    files inside subdirectories have module paths like `pkg::commands`,
///    not `pkg::commands::mod`.
///
/// Both checks use the same `ENTRYPOINT_SUFFIXES` set so the semantics
/// are symmetric — either the logical name or the filesystem name can
/// trigger the rule. Note: `lib.rs` is excluded from `ENTRYPOINT_SUFFIXES`
/// in this first pass; see the constant's doc comment for rationale.
fn is_entrypoint_module_by_path(module_path: &str, file_path: &str) -> bool {
    // Check 1: last module_path segment.
    let last_segment = module_path.rsplit("::").next().unwrap_or(module_path);
    if ENTRYPOINT_SUFFIXES.contains(&last_segment) {
        return true;
    }
    // Check 2: file basename stem (e.g. "main" from ".../src/main.rs").
    let stem = std::path::Path::new(file_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    ENTRYPOINT_SUFFIXES.contains(&stem)
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
/// Returns `None` when the item is permitted; returns `Some(reason)` when
/// it is a forbidden declaration that MO005 should flag.
fn mo005_classify_item(item: &AirItem, same_file_items: &[AirItem]) -> Option<String> {
    match item {
        AirItem::Type(t) => {
            // Allow the unit struct that is the host for a composition-host impl.
            if is_composition_host_struct(t, same_file_items) {
                return None;
            }
            let kind = match t.kind {
                locus_air::TypeKind::Struct => "struct",
                locus_air::TypeKind::Enum => "enum",
                locus_air::TypeKind::Trait => "trait",
                locus_air::TypeKind::Alias => "type alias",
                locus_air::TypeKind::Union => "union",
            };
            Some(format!(
                "{kind} `{}` declared in entrypoint module — move to a sibling module",
                t.name
            ))
        }
        AirItem::Impl(i) => {
            // Allow the composition-host impl: `impl Paradigm for LocalUnitStruct`
            // in a mod.rs, where all methods are thin (impl block ≤120 lines).
            if is_composition_host_impl(i, same_file_items) {
                return None;
            }
            let target = &i.target_type;
            Some(format!(
                "impl block for `{target}` in entrypoint module — move to the module that owns `{target}`"
            ))
        }
        AirItem::Function(f) => {
            let is_permitted_name = ENTRYPOINT_FN_NAMES.contains(&f.name.as_str());
            let within_budget = f.line_count <= MO005_THIN_FN_MAX_LINES;
            if is_permitted_name && within_budget {
                // Thin composition-glue function: allowed.
                return None;
            }
            if is_permitted_name {
                // Named correctly but too large — this is itself a smell.
                Some(format!(
                    "function `{}` in entrypoint module spans {} lines (budget {}); \
                     move its body into a dedicated module",
                    f.name, f.line_count, MO005_THIN_FN_MAX_LINES
                ))
            } else {
                // Non-permitted name — regardless of line count this is a
                // domain/business function that belongs elsewhere.
                Some(format!(
                    "function `{}` in entrypoint module is not composition glue; \
                     move it to a sibling module (e.g. `commands/`, `routes/`, \
                     `handlers/`)",
                    f.name
                ))
            }
        }
        AirItem::Conversion(c) => {
            // Converter / From/TryFrom impls in an entrypoint are unlikely;
            // flag them as misplaced.
            Some(format!(
                "converter `{}` declared in entrypoint module — move to a `convert.rs` \
                 or the owning domain module",
                c.symbol
            ))
        }
        // All other item kinds — imports, hints, facts, call-sites, etc.
        // — are passive observations, not declarations. Permitted.
        _ => None,
    }
}

/// MO005 — entrypoint modules must be composition surfaces, not ownership
/// sites.
///
/// Fires for every `AirItem` in a file whose `module_path` ends in `::main`
/// or `::mod` (or whose file's basename is `main.rs`/`mod.rs`) that is a
/// type declaration, impl block, converter, or a function that is not a thin
/// composition-glue wrapper (≤25 lines named `main`/`run`/`init`/`setup`/
/// `start`).
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
/// `lib.rs` is out of scope in this first pass; see follow-up issue for
/// lib.rs entrypoint handling.
///
/// **Rationale:** entrypoint modules are the last place agents look when
/// adding new behavior, so they accumulate it. Enforcing that they contain
/// only composition glue keeps the dependency tree legible and prevents
/// god-module accumulation at the binary root.
///
/// Severity: Warning by default; `--agent-strict` elevates to Fatal.
///
/// No lockfile configuration in the first pass — exemption via the standard
/// `// locus: allow MO005` source-hint.
pub fn mo005(air: &AirWorkspace, mode: CheckMode) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            if !is_entrypoint_module_by_path(module_path, &file.path) {
                continue;
            }
            // Compute a human-readable entrypoint label for the diagnostic.
            // Prefer the file basename (e.g. "main.rs") so the message
            // remains clear even when the module_path is flat (crate root).
            let file_label = std::path::Path::new(&file.path)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(&file.path);
            for item in &file.items {
                if let Some(reason) = mo005_classify_item(item, &file.items) {
                    let span = match item {
                        AirItem::Type(t) => t.span.clone(),
                        AirItem::Function(f) => f.span.clone(),
                        AirItem::Impl(i) => i.span.clone(),
                        AirItem::Conversion(c) => c.span.clone(),
                        _ => AirSpan::new(file.path.clone(), 1, 1),
                    };
                    out.push(Diagnostic {
                        rule_id: "MO005".to_string(),
                        severity: mode.elevate(Severity::Warning),
                        span,
                        concept: None,
                        message: format!("MO005: {reason}"),
                        why: vec![
                            format!(
                                "`{file_label}` (module `{module_path}`) is an entrypoint \
                                 module — it must be a composition surface, not an ownership site"
                            ),
                            "entrypoint modules are composition surfaces — they wire \
                             modules together via `mod` declarations, imports, and thin \
                             `main`/`run`/`init` functions. Substantial declarations \
                             belong in dedicated sibling modules."
                                .into(),
                        ],
                        suggested_fix: Some(
                            "move this declaration into a dedicated sibling module \
                             (e.g. `cli.rs` for the root arg struct, `commands/` for \
                             command implementations). Entrypoint should contain only \
                             `mod` decls, imports, and a thin `main` or `run` function."
                                .into(),
                        ),
                    });
                }
            }
        }
    }
    out
}

#[cfg(test)]
#[path = "rules_tests.rs"]
mod rules_tests;
