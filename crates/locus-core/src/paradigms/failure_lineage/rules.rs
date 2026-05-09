//! FL rule implementations.
//!
//! Implemented:
//! - [`fl001`]: a function in a domain module returns `Result<_, E>` where E
//!   is a declared boundary error type. Boundary errors leaking into domain
//!   function signatures break the failure-lineage invariant — the layer
//!   edge that should have wrapped the transport error didn't.
//! - [`fl002`]: a "panic-shaped" callee (`unwrap` / `expect` /
//!   `unwrap_or_default` / `panic` / `todo` / `unimplemented`) fires from a
//!   file whose `module_path` is not in `invariant_owner_paths`. The
//!   agent's "make it compile by unwrapping" anti-pattern.
//! - [`fl003`]: a silent-discard method call (`.ok()` / `.err()` /
//!   `.unwrap_or_else()`) outside `invariant_owner_paths`. Catches the
//!   inverse of FL002 — failure swallowed instead of failure shouted.
//! - [`fl004`]: a `let _ = expr;` discarded binding outside
//!   `invariant_owner_paths`, where `expr` is a call (`Method` /
//!   `Function` / `Macro`) and the callee isn't on the
//!   `silent_discard_allowed_callees` allowlist. Reads
//!   `AirItem::SilentDiscard` items the visitor emits since AIR v9.
//! - [`fl005`]: an `if let Ok(...) = expr { ... }` or `if let Err(...) =
//!   expr { ... }` with no `else` branch outside `invariant_owner_paths`.
//!   The unmatched arm is implicitly silent. Reads `AirItem::PartialResultMatch`
//!   items the visitor emits since AIR v9.
//! - [`fl013`]: a function returning `Result<_, String>` or `Result<_, &str>`
//!   that contains a call site stringifying via `to_string` / `format!` /
//!   `format` / `display`. The error is being lossily collapsed to a string
//!   on its way out of the function, erasing the failure lineage at the
//!   source. Lockfile-driven silence via the existing `invariant_owner_paths`.
//! - [`fl006`]: a `.map_err(|_| ...)` call that discards the closure's
//!   error argument outside `invariant_owner_paths`. Reads
//!   [`AirItem::ClosureMethodCall`] (AIR v10) — the source error is
//!   dropped before being mapped to the new type, so failure lineage is
//!   broken at the conversion site.
//! - [`fl007`]: a catch-all `Err(_) => <silent>` match arm whose body is
//!   `Empty`, `Literal`, or `Call` outside `invariant_owner_paths`. Reads
//!   [`AirItem::MatchArm`] — every `Err` variant is matched by `_` and
//!   the failure is silently routed to a default-producing body.
//! - [`fl010`]: a `.unwrap_or(...)` / `.or(...)` call whose default
//!   argument is a `Literal` or `Call` outside `invariant_owner_paths`.
//!   Reads [`AirItem::FallbackCall`] (AIR v12). The
//!   "invalid input silently replaced with a valid default" anti-pattern —
//!   distinct from FL002's `unwrap_or_default()` (no-arg) form, which
//!   is covered by `forbidden_callees`. FL010 deliberately skips
//!   `default_shape` `Empty` (FL002's territory), `Block` and `Other`
//!   (might be doing real work; conservative).
//! - [`fl011`]: a bare `_ => <silent>` arm whose body is `Empty`,
//!   `Literal`, or `Call` outside `invariant_owner_paths`. The
//!   "unknown enum variant routed to a default" anti-pattern — distinct
//!   from FL007 because the pattern is the bare wildcard, not an `Err`
//!   variant.
//! - [`fl012`]: a `loop` / `for` / `while` whose body uses `?` and has
//!   at least one `break`, outside `retry_policy_owner_paths`. Reads
//!   [`AirItem::RetryLoop`] (AIR v12). The "ad-hoc retry without
//!   accepted policy" anti-pattern — fallible work being repeated
//!   until success with no declared backoff / max-attempts / jitter.
//!
//! Future FL rules will tackle the patterns AIR still can't see: spawned-task
//! failures with no sink (richer fact production).

use locus_air::{
    AirCallSite, AirClosureMethodCall, AirFallbackCall, AirItem, AirMatchArm, AirRetryLoop,
    AirWorkspace, ArmBodyShape, CallKind, LoopKind,
};

use super::lockfile_schema::{FlSection, containing_module_of, matches_pattern};

/// Shared helper: is the (file, function) considered an invariant-owner
/// context for FL002–FL005 suppression?
///
/// File-level match: `module_path` matches any pattern.
/// Function-level match: the symbol's containing module (everything
/// before the last `::`) matches any pattern. This catches inline
/// `mod tests { ... }` blocks whose enclosing file's `module_path`
/// doesn't include `::tests::` but whose function symbols do.
fn callsite_in_invariant_owner(
    file_module: &str,
    function_symbol: Option<&str>,
    patterns: &[String],
) -> bool {
    if patterns.iter().any(|p| matches_pattern(p, file_module)) {
        return true;
    }
    if let Some(sym) = function_symbol {
        let containing = containing_module_of(sym);
        if patterns.iter().any(|p| matches_pattern(p, containing)) {
            return true;
        }
    }
    false
}
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

/// FL001 — boundary error leaks into a domain function signature.
///
/// For every `AirFile` whose `module_path` matches any pattern in
/// `domain_paths`, inspect each `AirItem::Function`. If the function's
/// `return_type` parses as `Result<T, E>` (top level — generics inside T are
/// skipped over) and `E` matches any pattern in `boundary_error_patterns`,
/// fire one diagnostic.
///
/// Severity: **Fatal** in both modes. Boundary errors leaking into domain
/// signatures is a structural failure: the layer edge that should have
/// wrapped the error in a domain error type didn't, and the failure has
/// already lost its owner by the time the function is called. Unlike the
/// mostly-heuristic FL futures, this one is deterministic — driven entirely
/// by signature-shape and explicit lockfile patterns — so the strict tier is
/// the right default. `CheckMode::elevate` is still applied for symmetry,
/// even though it's a no-op on Fatal.
pub fn fl001(air: &AirWorkspace, section: &FlSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.domain_paths.is_empty() || section.boundary_error_patterns.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            let Some(domain_pattern) = section
                .domain_paths
                .iter()
                .find(|pat| matches_pattern(pat, module_path))
            else {
                continue;
            };
            for item in &file.items {
                let AirItem::Function(func) = item else {
                    continue;
                };
                let Some(ret) = func.return_type.as_deref() else {
                    continue;
                };
                let Some(err_ty) = extract_result_error_type(ret) else {
                    continue;
                };
                let Some(boundary_pattern) = section
                    .boundary_error_patterns
                    .iter()
                    .find(|pat| matches_pattern(pat, err_ty))
                else {
                    continue;
                };
                out.push(Diagnostic {
                    rule_id: "FL001".to_string(),
                    severity: mode.elevate(Severity::Fatal),
                    span: func.span.clone(),
                    concept: None,
                    message: format!(
                        "domain function `{}` returns boundary error type `{}` \
                         (matched domain pattern `{}`, boundary pattern `{}`)",
                        func.name, err_ty, domain_pattern, boundary_pattern,
                    ),
                    why: vec![
                        format!("module `{module_path}` matches domain pattern `{domain_pattern}`"),
                        format!("function `{}` (`{}`)", func.name, func.symbol),
                        format!("return type `{ret}`"),
                        format!(
                            "extracted error type `{err_ty}` matches boundary pattern \
                             `{boundary_pattern}`"
                        ),
                        "domain function signatures must speak the domain's error \
                         vocabulary; transport / boundary errors leak the failure lineage \
                         past the layer that should have wrapped them"
                            .into(),
                    ],
                    suggested_fix: Some(format!(
                        "wrap `{err_ty}` in a domain error type at the layer's edge — \
                         either `impl From<{err_ty}> for <DomainError>` or an explicit \
                         `map_err` at the boundary — so `{}` returns the domain error \
                         instead",
                        func.name,
                    )),
                });
            }
        }
    }
    out
}

/// Extract the `E` from a top-level `Result<T, E>` rendered as a string.
///
/// Returns `None` if the string isn't a top-level `Result<...>`, the angle
/// brackets don't balance, or the `<...>` body doesn't have a top-level
/// comma (e.g. `Result<T>` from a custom `Result` alias with one type
/// parameter — not what FL001 reasons about).
///
/// The renderer in `locus-rust::type_render` strips superfluous spaces but
/// we still trim once to be defensive against future renderer changes. We
/// also accept a leading `::` (`::std::result::Result<T, E>` style) by
/// peeling it off once before the prefix check.
fn extract_result_error_type(rendered: &str) -> Option<&str> {
    let s = rendered.trim();
    let s = s.strip_prefix("::").unwrap_or(s);
    // Accept the bare `Result<...>` shape. We deliberately don't try to
    // resolve `std::result::Result` / `core::result::Result` here — the
    // adapter renders the path the user wrote, so a fully-qualified
    // `std::result::Result<T, E>` simply won't be matched. That's fine: the
    // overwhelmingly common form in domain code is bare `Result<...>`, and
    // false positives on a hand-qualified `Result` alias would be worse
    // than missing the diagnostic.
    let inner = s.strip_prefix("Result<")?.strip_suffix('>')?;
    // Find the top-level comma — angle-bracket-aware so `Result<HashMap<K,
    // V>, E>` correctly returns `E`, not `V>, E`.
    let mut depth: i32 = 0;
    let mut split_at: Option<usize> = None;
    for (idx, ch) in inner.char_indices() {
        match ch {
            '<' => depth += 1,
            '>' => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
            }
            ',' if depth == 0 => {
                split_at = Some(idx);
                break;
            }
            _ => {}
        }
    }
    let split_at = split_at?;
    let err_ty = inner[split_at + 1..].trim();
    if err_ty.is_empty() {
        None
    } else {
        Some(err_ty)
    }
}

/// FL002 — unwrap-family failure swallowing.
///
/// For every `AirItem::CallSite` whose `kind` is `Method` or `Macro` and
/// whose `callee` (last `::` segment for path-qualified macros) matches any
/// pattern in `forbidden_callees`, fire a diagnostic when the call site's
/// enclosing-file `module_path` does NOT match any pattern in
/// `invariant_owner_paths`. Function-shaped calls are intentionally
/// excluded — `panic!` is a `Macro`, `unwrap`/`expect` are `Method`s, and
/// `Function` calls would only false-positive on user code that happens to
/// name a function `unwrap`.
///
/// Severity: Warning by default; Fatal under `--agent-strict`. The fact is
/// deterministic — `mode.elevate(Severity::Warning)` — but the policy is a
/// lockfile decision, so the human-mode posture is "warn, don't break CI".
///
/// Silent until `invariant_owner_paths` is populated, mirroring every other
/// lockfile-driven rule. The default `forbidden_callees` list is non-empty
/// but the rule still doesn't fire until the user has declared *where the
/// legitimate panic-callsites live*.
pub fn fl002(air: &AirWorkspace, section: &FlSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.invariant_owner_paths.is_empty() || section.forbidden_callees.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            for item in &file.items {
                let AirItem::CallSite(cs) = item else {
                    continue;
                };
                // Function-shaped calls don't carry the unwrap/panic
                // semantics we care about (and would false-positive on user
                // free functions named `unwrap`). Method and Macro only.
                if !matches!(cs.kind, CallKind::Method | CallKind::Meta) {
                    continue;
                }
                if callsite_in_invariant_owner(
                    module_path,
                    cs.function.as_deref(),
                    &section.invariant_owner_paths,
                ) {
                    continue;
                }
                let last = cs.callee.rsplit("::").next().unwrap_or(&cs.callee);
                let Some(forbidden_pattern) = section
                    .forbidden_callees
                    .iter()
                    .find(|pat| matches_pattern(pat, last))
                else {
                    continue;
                };
                out.push(diagnostic_for_fl002(
                    cs,
                    module_path,
                    forbidden_pattern,
                    mode,
                ));
            }
        }
    }
    out
}

fn diagnostic_for_fl002(
    cs: &AirCallSite,
    module_path: &str,
    forbidden_pattern: &str,
    mode: CheckMode,
) -> Diagnostic {
    let function_label = cs
        .function
        .as_deref()
        .unwrap_or("<unknown enclosing function>");
    Diagnostic {
        rule_id: "FL002".to_string(),
        severity: mode.elevate(Severity::Warning),
        span: cs.span.clone(),
        concept: None,
        message: format!(
            "panic-shaped call `{}` in `{module_path}` (fn `{function_label}`) — \
             matches `paradigms.FL.forbidden_callees` pattern `{forbidden_pattern}`",
            cs.callee,
        ),
        why: vec![
            format!("callee `{}`", cs.callee),
            format!("enclosing function: `{function_label}`"),
            format!(
                "module `{module_path}` does not match any \
                 `paradigms.FL.invariant_owner_paths` pattern"
            ),
            format!(
                "callee matches forbidden pattern `{forbidden_pattern}` in \
                 `paradigms.FL.forbidden_callees`"
            ),
        ],
        suggested_fix: Some(format!(
            "replace this `{}` with structured error propagation — return \
             a `Result` and let the caller handle the failure path — or, if \
             `{module_path}` is a legitimate invariant owner (supervisor, \
             startup-asserting entry point, test-support module), accept it \
             by adding the module to `paradigms.FL.invariant_owner_paths` \
             in `locus.lock`",
            cs.callee,
        )),
    }
}

/// FL003 — silent error discard.
///
/// Catches the *opposite* failure mode from FL002. Where FL002 flags loud
/// panics that abort the process, FL003 flags **silent** discards: method
/// calls that convert a `Result` into a value-or-default without
/// propagating the error. Spec: `docs/PARADIGMS.md` line 804–807
/// (".ok() / unwrap_or_default masking, etc.").
///
/// Detection is restricted to **method calls** (`AirCallSite` with
/// `kind == Method`) — bare-name `Function` calls and macros never carry
/// silent-discard semantics. Receiver-type resolution is out of AIR's
/// scope today, so we match purely on callee name; in practice the std
/// surface for `.ok()` / `.err()` is `Result`-only, so the
/// false-positive rate is low. Users who hit a legitimate non-Result
/// `.ok()` (e.g. via a third-party trait) suppress with
/// `// locus: allow FL003 reason="..." expires="..."`.
///
/// Severity: Warning by default; Fatal under `--agent-strict`.
///
/// Shares `invariant_owner_paths` with FL002. The semantics line up:
/// "modules where the rule's anti-pattern is legitimate" applies equally
/// to test fixtures that legitimately do `result.ok()` to assert
/// best-effort behaviour. Silent until `invariant_owner_paths` is
/// populated.
///
/// Note on coverage: this rule sees `.ok()` / `.err()` calls only.
/// Other silent-error patterns require visitor work that's not done yet:
///
/// - `let _ = result;` — the visitor doesn't emit an item for discarded
///   bindings.
/// - `if let Ok(x) = result { ... }` — match-arm bodies aren't tracked.
/// - `match result { Ok(x) => x, Err(_) => default }` — same.
///
/// Those land when AIR adds the corresponding source-fact items.
pub fn fl003(air: &AirWorkspace, section: &FlSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.invariant_owner_paths.is_empty() || section.silent_discard_callees.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            for item in &file.items {
                let AirItem::CallSite(cs) = item else {
                    continue;
                };
                // Method-only — `.ok()` is the smoking gun, and we don't
                // want to flag a free function happening to be named `ok`.
                if !matches!(cs.kind, CallKind::Method) {
                    continue;
                }
                if callsite_in_invariant_owner(
                    module_path,
                    cs.function.as_deref(),
                    &section.invariant_owner_paths,
                ) {
                    continue;
                }
                let last = cs.callee.rsplit("::").next().unwrap_or(&cs.callee);
                let Some(silent_pattern) = section
                    .silent_discard_callees
                    .iter()
                    .find(|pat| matches_pattern(pat, last))
                else {
                    continue;
                };
                out.push(diagnostic_for_fl003(cs, module_path, silent_pattern, mode));
            }
        }
    }
    out
}

fn diagnostic_for_fl003(
    cs: &AirCallSite,
    module_path: &str,
    silent_pattern: &str,
    mode: CheckMode,
) -> Diagnostic {
    let function_label = cs
        .function
        .as_deref()
        .unwrap_or("<unknown enclosing function>");
    Diagnostic {
        rule_id: "FL003".to_string(),
        severity: mode.elevate(Severity::Warning),
        span: cs.span.clone(),
        concept: None,
        message: format!(
            "silent error discard `.{}()` in `{module_path}` (fn `{function_label}`) — \
             matches `paradigms.FL.silent_discard_callees` pattern `{silent_pattern}`",
            cs.callee,
        ),
        why: vec![
            format!("method call `.{}()`", cs.callee),
            format!("enclosing function: `{function_label}`"),
            format!(
                "module `{module_path}` does not match any \
                 `paradigms.FL.invariant_owner_paths` pattern"
            ),
            format!(
                "callee matches silent-discard pattern `{silent_pattern}` in \
                 `paradigms.FL.silent_discard_callees` — converts a `Result` \
                 into a value or `Option` without propagating the error"
            ),
        ],
        suggested_fix: Some(format!(
            "propagate the error with `?` and let the caller decide, or \
             explicitly handle the `Err` branch — `let value = result.{}()` \
             discards the failure lineage. If `{module_path}` is a legitimate \
             invariant owner (supervisor, test-support module), add it to \
             `paradigms.FL.invariant_owner_paths`. For a one-off intentional \
             discard, suppress with `// locus: allow FL003 reason=\"…\" \
             expires=\"YYYY-MM-DD\"`",
            cs.callee,
        )),
    }
}

/// FL004 — `let _ = expr;` silent-discard binding.
///
/// Closes the gap FL003 leaves open: FL003 sees `result.ok()` /
/// `.err()` / `.unwrap_or_else()` (method-call shape), but it can't see
/// the `let _ = result;` shape because it's a binding, not a method call.
/// AIR v9 added [`AirItem::SilentDiscard`] for exactly this case — the
/// visitor records `let _ = <call>` statements with the rendered callee.
///
/// Detection rules:
/// - Only `DiscardKind::Method` / `Function` / `Macro` are considered.
///   `Other` discards (`let _ = some_field;`) are skipped — the
///   false-positive surface for arbitrary expression discards is too
///   large to be useful.
/// - The discarded callee must NOT match any pattern in
///   `silent_discard_allowed_callees` (default covers the canonical
///   fire-and-forget shapes: `lock`, `send`, `drop`, `set_logger`,
///   `subscribe`, `try_init`).
/// - The enclosing file's `module_path` must NOT match any
///   `invariant_owner_paths` pattern.
///
/// Severity: `mode.elevate(Severity::Warning)` — Warning in human, Fatal
/// under `--agent-strict`. Same posture as FL002 / FL003.
///
/// Lockfile-driven silence: stays quiet until `invariant_owner_paths`
/// is populated, regardless of how the allowlist is configured.
pub fn fl004(air: &AirWorkspace, section: &FlSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.invariant_owner_paths.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            for item in &file.items {
                let AirItem::SilentDiscard(d) = item else {
                    continue;
                };
                if matches!(d.kind, locus_air::DiscardKind::Other) {
                    continue;
                }
                let Some(callee) = d.callee.as_deref() else {
                    continue;
                };
                if section
                    .silent_discard_allowed_callees
                    .iter()
                    .any(|pat| matches_pattern(pat, callee))
                {
                    continue;
                }
                if callsite_in_invariant_owner(
                    module_path,
                    d.function.as_deref(),
                    &section.invariant_owner_paths,
                ) {
                    continue;
                }
                out.push(diagnostic_for_fl004(d, module_path, callee, mode));
            }
        }
    }
    out
}

fn diagnostic_for_fl004(
    d: &locus_air::AirSilentDiscard,
    module_path: &str,
    callee: &str,
    mode: CheckMode,
) -> Diagnostic {
    let function_label = d
        .function
        .as_deref()
        .unwrap_or("<unknown enclosing function>");
    let kind_label = match d.kind {
        locus_air::DiscardKind::Method => "method",
        locus_air::DiscardKind::Function => "function",
        locus_air::DiscardKind::Meta => "macro",
        locus_air::DiscardKind::Other => "expression",
    };
    Diagnostic {
        rule_id: "FL004".to_string(),
        severity: mode.elevate(Severity::Warning),
        span: d.span.clone(),
        concept: None,
        message: format!(
            "discarded binding `let _ = {callee}(...)` ({kind_label}) in `{module_path}` \
             (fn `{function_label}`) — failure (if any) is silently dropped"
        ),
        why: vec![
            format!("`let _ = ...` discards the binding without inspecting the value"),
            format!("discarded callee: `{callee}` ({kind_label})"),
            format!("enclosing function: `{function_label}`"),
            format!(
                "module `{module_path}` does not match any \
                 `paradigms.FL.invariant_owner_paths` pattern"
            ),
            format!(
                "callee does not match any \
                 `paradigms.FL.silent_discard_allowed_callees` pattern"
            ),
        ],
        suggested_fix: Some(format!(
            "if `{callee}` returns a `Result`, propagate the error with \
             `?` instead of dropping it; if the discard is intentional \
             and the callee is a known fire-and-forget pattern (e.g. \
             `lock`, `send`, `drop`), add it to \
             `paradigms.FL.silent_discard_allowed_callees`. For a one-off \
             accepted discard, suppress with `// locus: allow FL004 reason=\"…\" \
             expires=\"YYYY-MM-DD\"`. If `{module_path}` is a legitimate \
             invariant owner, add it to `paradigms.FL.invariant_owner_paths`"
        )),
    }
}

/// FL005 — partial `if let Ok/Err = ...` without `else`.
///
/// Catches the pattern `if let Ok(x) = result { ... }` (or its `Err`
/// inverse) with no `else` branch — the unmatched arm is silent, and
/// any failure (or success) on that path is dropped without
/// acknowledgement. Reads [`AirItem::PartialResultMatch`] items the visitor
/// emits since AIR v9.
///
/// Severity: `mode.elevate(Severity::Warning)` — Warning in human, Fatal
/// under `--agent-strict`. Symmetric with FL003 / FL004.
///
/// Lockfile-driven silence: stays quiet until `invariant_owner_paths`
/// is populated.
pub fn fl005(air: &AirWorkspace, section: &FlSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.invariant_owner_paths.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            for item in &file.items {
                let AirItem::PartialResultMatch(p) = item else {
                    continue;
                };
                if callsite_in_invariant_owner(
                    module_path,
                    p.function.as_deref(),
                    &section.invariant_owner_paths,
                ) {
                    continue;
                }
                out.push(diagnostic_for_fl005(p, module_path, mode));
            }
        }
    }
    out
}

fn diagnostic_for_fl005(
    p: &locus_air::AirPartialResultMatch,
    module_path: &str,
    mode: CheckMode,
) -> Diagnostic {
    let function_label = p
        .function
        .as_deref()
        .unwrap_or("<unknown enclosing function>");
    // AIR v13 turned `variant` from a String into `ResultMatchVariant`.
    // Render the architectural-side label (Success/Failure) and the
    // surface-side label (Ok/Err for Rust, ok/err for TS, etc. — Rust
    // adapter today uses Ok/Err) so the diagnostic stays readable in
    // both vocabularies.
    let (matched_label, unmatched_label) = match p.variant {
        locus_air::ResultMatchVariant::Success => ("Ok", "Err"),
        locus_air::ResultMatchVariant::Failure => ("Err", "Ok"),
    };
    Diagnostic {
        rule_id: "FL005".to_string(),
        severity: mode.elevate(Severity::Warning),
        span: p.span.clone(),
        concept: None,
        message: format!(
            "partial `if let {matched_label}(...) = ...` (no `else` branch) in `{module_path}` \
             (fn `{function_label}`) — the `{unmatched_label}` arm is implicitly silent"
        ),
        why: vec![
            format!(
                "`if let {matched_label}(...) = ...` matches only the `{matched_label}` variant; the `{unmatched_label}` \
                 arm has no body and falls through silently"
            ),
            format!("enclosing function: `{function_label}`"),
            format!(
                "module `{module_path}` does not match any \
                 `paradigms.FL.invariant_owner_paths` pattern"
            ),
        ],
        suggested_fix: Some(format!(
            "rewrite as a `match` with both arms, or add an `else` branch \
             that handles the `{unmatched_label}` case (log, propagate, or \
             explicitly accept). If `{module_path}` is a legitimate \
             invariant owner (supervisor, test-support module), add it to \
             `paradigms.FL.invariant_owner_paths`. For a one-off accepted \
             partial match, suppress with `// locus: allow FL005 reason=\"…\" \
             expires=\"YYYY-MM-DD\"`"
        )),
    }
}

/// FL013 — lossy error stringification in error returns.
///
/// Catches the pattern: a function returns `Result<T, String>` (or
/// `Result<T, &str>`) and somewhere inside it a call site stringifies a
/// value — `.to_string()`, `format!(...)`, `format(...)`, `.display()`.
/// The function isn't carrying a typed error out; it's collapsing
/// whatever failed into a string at the source, so the call site that
/// should have produced a typed `Err(SomeErrorVariant)` lossy-converted
/// instead. Failure lineage is gone the moment the function returns.
///
/// Detection:
/// - Walk every `AirItem::Function` whose `return_type` parses as
///   `Result<_, String>` or `Result<_, &str>` (the same extractor FL001
///   uses, with normalisation that strips one leading `&`). Custom
///   `Result<T>` aliases without a top-level comma are skipped.
/// - For each matching function, walk every `AirItem::CallSite` whose
///   `function == Some(func.symbol)`. Fire when **any** call site's
///   callee (last `::` segment) matches one of `["to_string",
///   "format!", "format", "display"]`. Bare-name match — receiver type
///   resolution is out of scope, same posture as FL003.
/// - Skip files (and call-site enclosing-modules) that match
///   `invariant_owner_paths`. We reuse the existing FL field rather
///   than introduce a new lockfile knob — the semantics line up
///   (modules where the rule's anti-pattern is legitimate, e.g.
///   test fixtures, CLI surfaces, supervisors).
///
/// Severity: `mode.elevate(Severity::Warning)` — Warning in human, Fatal
/// under `--agent-strict`. Same posture as FL002 / FL003 / FL004 / FL005.
///
/// Silent until `invariant_owner_paths` is populated, mirroring every
/// other lockfile-driven FL rule.
pub fn fl013(air: &AirWorkspace, section: &FlSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.invariant_owner_paths.is_empty() {
        return Vec::new();
    }

    const STRINGIFY_CALLEES: &[&str] = &["to_string", "format!", "format", "display"];

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            // Pre-compute file-level invariant-owner status. If the file
            // is on the allowlist, every call site inside it is too.
            let file_is_owner = section
                .invariant_owner_paths
                .iter()
                .any(|p| matches_pattern(p, module_path));
            if file_is_owner {
                continue;
            }

            // Pass 1: collect Result<_, String|&str> functions in this file.
            let stringy_fns: Vec<&locus_air::AirFunction> = file
                .items
                .iter()
                .filter_map(|item| match item {
                    AirItem::Function(func) => {
                        let ret = func.return_type.as_deref()?;
                        let err_ty = extract_result_error_type(ret)?;
                        let normalised = err_ty.trim().strip_prefix('&').unwrap_or(err_ty).trim();
                        if normalised == "String" || normalised == "str" {
                            Some(func)
                        } else {
                            None
                        }
                    }
                    _ => None,
                })
                .collect();
            if stringy_fns.is_empty() {
                continue;
            }

            // Pass 2: scan call sites; for each matching enclosing-fn, fire
            // on the first stringifying callee we see (one diag per fn —
            // multiple stringifications inside one function reduce to a
            // single signal; the fix is the same regardless of count).
            for func in stringy_fns {
                if callsite_in_invariant_owner(
                    module_path,
                    Some(&func.symbol),
                    &section.invariant_owner_paths,
                ) {
                    continue;
                }
                let hit = file.items.iter().find_map(|item| {
                    let AirItem::CallSite(cs) = item else {
                        return None;
                    };
                    if cs.function.as_deref() != Some(func.symbol.as_str()) {
                        return None;
                    }
                    let last = cs.callee.rsplit("::").next().unwrap_or(&cs.callee);
                    if STRINGIFY_CALLEES.contains(&last) {
                        Some(cs)
                    } else {
                        None
                    }
                });
                if let Some(cs) = hit {
                    out.push(diagnostic_for_fl013(func, cs, module_path, mode));
                }
            }
        }
    }
    out
}

fn diagnostic_for_fl013(
    func: &locus_air::AirFunction,
    cs: &locus_air::AirCallSite,
    module_path: &str,
    mode: CheckMode,
) -> Diagnostic {
    let ret = func.return_type.as_deref().unwrap_or("?");
    Diagnostic {
        rule_id: "FL013".to_string(),
        severity: mode.elevate(Severity::Warning),
        span: cs.span.clone(),
        concept: None,
        message: format!(
            "lossy error stringification in `{}` (`{ret}`) — call site `{}` collapses \
             a typed value into a string before it leaves the function",
            func.name, cs.callee,
        ),
        why: vec![
            format!("function `{}` (`{}`)", func.name, func.symbol),
            format!("return type `{ret}` — `String` / `&str` errors carry no failure lineage"),
            format!("call site `{}` (line {})", cs.callee, cs.span.line_start),
            format!(
                "module `{module_path}` does not match any \
                 `paradigms.FL.invariant_owner_paths` pattern"
            ),
            "stringifying an error at the source erases the variant the caller \
             would otherwise pattern-match against; the failure mode is gone by \
             the time the function returns"
                .into(),
        ],
        suggested_fix: Some(format!(
            "define a typed error (`enum {}Error {{ ... }}` or similar) and use \
             `?` propagation so each failure mode keeps its own variant; if \
             `{module_path}` is a legitimate string-error surface (top-level \
             CLI, test fixture, supervisor), add it to \
             `paradigms.FL.invariant_owner_paths`. For a one-off accepted \
             stringification, suppress with `// locus: allow FL013 reason=\"…\" \
             expires=\"YYYY-MM-DD\"`",
            capitalize_first_fl013(&func.name),
        )),
    }
}

/// Capitalize-first helper, local to FL013. Duplicated rather than imported
/// from a sibling paradigm (CLAUDE.md: paradigms must not depend on each
/// other).
fn capitalize_first_fl013(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {
            let mut out = String::with_capacity(s.len());
            out.push(c.to_ascii_uppercase());
            out.extend(chars);
            out
        }
        _ => s.to_string(),
    }
}

/// Render an [`ArmBodyShape`] for diagnostic messages.
fn body_shape_label(shape: ArmBodyShape) -> &'static str {
    match shape {
        ArmBodyShape::Empty => "empty body",
        ArmBodyShape::Literal => "literal default",
        ArmBodyShape::Call => "call expression",
        ArmBodyShape::Return => "return",
        ArmBodyShape::ErrorPropagation => "?-propagation",
        ArmBodyShape::Block => "block",
        ArmBodyShape::Other => "other",
    }
}

/// True when an arm body is one of the silent / default-producing shapes
/// FL007 and FL011 fire on.
fn is_silent_body_shape(shape: ArmBodyShape) -> bool {
    matches!(
        shape,
        ArmBodyShape::Empty | ArmBodyShape::Literal | ArmBodyShape::Call
    )
}

/// FL006 — `map_err(|_| ...)` losing source context.
///
/// Reads [`AirItem::ClosureMethodCall`] items (AIR v10). Fires on a
/// `.map_err(...)` call whose closure discards its argument
/// (`closure_discards_arg == true`, i.e. `|_|` / `||` / `|_, x|`)
/// outside `invariant_owner_paths`. The original `Err` value is dropped
/// before it can be wrapped in the new error type, so failure lineage
/// is broken at the conversion site.
///
/// Severity: `mode.elevate(Severity::Warning)` — Warning in human, Fatal
/// under `--agent-strict`. Same posture as FL002–FL005.
///
/// Lockfile-driven silence: stays quiet until `invariant_owner_paths`
/// is populated.
pub fn fl006(air: &AirWorkspace, section: &FlSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.invariant_owner_paths.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            for item in &file.items {
                let AirItem::ClosureMethodCall(cmc) = item else {
                    continue;
                };
                if cmc.callee != "map_err" {
                    continue;
                }
                if !cmc.closure_discards_arg {
                    continue;
                }
                if callsite_in_invariant_owner(
                    module_path,
                    cmc.function.as_deref(),
                    &section.invariant_owner_paths,
                ) {
                    continue;
                }
                out.push(diagnostic_for_fl006(cmc, module_path, mode));
            }
        }
    }
    out
}

fn diagnostic_for_fl006(
    cmc: &AirClosureMethodCall,
    module_path: &str,
    mode: CheckMode,
) -> Diagnostic {
    let function_label = cmc
        .function
        .as_deref()
        .unwrap_or("<unknown enclosing function>");
    Diagnostic {
        rule_id: "FL006".to_string(),
        severity: mode.elevate(Severity::Warning),
        span: cmc.span.clone(),
        concept: None,
        message: format!(
            "map_err(|_| ...) in `{module_path}` (fn `{function_label}`) discards \
             the original error — failure lineage broken at the conversion site"
        ),
        why: vec![
            format!("module `{module_path}`"),
            format!("enclosing function: `{function_label}`"),
            "closure pattern is `_` — original `Err` value is dropped".into(),
            "`map_err` should preserve or transform the source error, not erase it".into(),
        ],
        suggested_fix: Some(
            "replace |_| with |e| <transform>(e) so the source error is wrapped or \
             logged before being mapped to the new type; or accept the file via \
             `paradigms.FL.invariant_owner_paths` if this is a legitimate adapter \
             boundary that has already logged the source. For a one-off, suppress \
             with `// locus: allow FL006 reason=\"…\" expires=\"YYYY-MM-DD\"`"
                .into(),
        ),
    }
}

/// FL007 — catch-all `Err(_) =>` arm body silently swallows.
///
/// Reads [`AirItem::MatchArm`] items (AIR v10). Fires on an arm whose
/// pattern matches an `Err` variant *and* contains a wildcard binder
/// (`Err(_) => …`) AND whose body shape is one of `Empty`, `Literal`,
/// `Call` — the silent default-producing shapes. Arms that `Return`,
/// `Propagate` (use `?`), or run a multi-statement `Block` are not
/// flagged: their author has already taken explicit action.
///
/// Pattern detection is text-based: the visitor records the arm's
/// pattern as rendered text, so we look for the `Err` prefix /
/// `Err(` substring in combination with the boolean
/// `pattern_has_wildcard`. This intentionally accepts both the bare
/// `Err(_)` form and qualified shapes like `MyError::Err(_)` or
/// `Result::Err(_)`.
///
/// Severity: `mode.elevate(Severity::Warning)`.
///
/// Lockfile-driven silence: stays quiet until `invariant_owner_paths`
/// is populated.
pub fn fl007(air: &AirWorkspace, section: &FlSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.invariant_owner_paths.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            for item in &file.items {
                let AirItem::MatchArm(arm) = item else {
                    continue;
                };
                if !arm.pattern_has_wildcard {
                    continue;
                }
                if !pattern_targets_err_variant(&arm.pattern) {
                    continue;
                }
                if !is_silent_body_shape(arm.body_shape) {
                    continue;
                }
                if callsite_in_invariant_owner(
                    module_path,
                    arm.function.as_deref(),
                    &section.invariant_owner_paths,
                ) {
                    continue;
                }
                out.push(diagnostic_for_fl007(arm, module_path, mode));
            }
        }
    }
    out
}

/// Is the arm pattern an `Err` variant — bare `Err(...)` or path-qualified
/// (`Result::Err(...)`, `MyEnum::Err(...)`)? FL007 fires on these; FL011
/// is the bare-`_` complement.
fn pattern_targets_err_variant(pattern: &str) -> bool {
    let p = pattern.trim();
    p == "Err"
        || p.starts_with("Err(")
        || p.contains("::Err(")
        || p.contains("::Err ")
        || p.ends_with("::Err")
}

fn diagnostic_for_fl007(arm: &AirMatchArm, module_path: &str, mode: CheckMode) -> Diagnostic {
    let function_label = arm
        .function
        .as_deref()
        .unwrap_or("<unknown enclosing function>");
    let body_label = body_shape_label(arm.body_shape);
    Diagnostic {
        rule_id: "FL007".to_string(),
        severity: mode.elevate(Severity::Warning),
        span: arm.span.clone(),
        concept: None,
        message: format!(
            "catch-all `Err(_) => {body_label}` arm in `{module_path}` (fn `{function_label}`) \
             silently swallows the failure"
        ),
        why: vec![
            format!("module `{module_path}`"),
            format!("enclosing function: `{function_label}`"),
            format!("arm pattern `{}` matches every `Err` variant", arm.pattern),
            format!("arm body is a `{body_label}` (silent default)"),
            "the failure has no owner — caller can't tell anything went wrong".into(),
        ],
        suggested_fix: Some(format!(
            "rewrite to bind the error and either log/wrap it or propagate via `?` — \
             e.g. `Err(e) => return Err(MyError::from(e))`. If `{module_path}` is a \
             legitimate invariant owner (supervisor, test-support module), add it \
             to `paradigms.FL.invariant_owner_paths`. For a one-off accepted \
             swallow, suppress with `// locus: allow FL007 reason=\"…\" \
             expires=\"YYYY-MM-DD\"`"
        )),
    }
}

/// FL011 — default-variant arm as failure sink.
///
/// Reads [`AirItem::MatchArm`] items. Fires on an arm whose pattern is
/// the bare wildcard `_` (or `_ if guard` — anything that trims to `_`
/// at the head) AND whose body shape is `Empty`, `Literal`, or `Call`.
/// The "I don't know what to do here so I'll just default" anti-pattern
/// on enum scrutinees: an unknown variant should be an explicit error
/// or explicit ignore, not a silent fall-through.
///
/// Distinct from FL007: FL007 fires on `Err(_)` shapes (any pattern
/// targeting the `Err` variant); FL011 fires only on the bare `_`
/// pattern. They never overlap.
///
/// Severity: `mode.elevate(Severity::Warning)`.
///
/// Lockfile-driven silence: stays quiet until `invariant_owner_paths`
/// is populated.
pub fn fl011(air: &AirWorkspace, section: &FlSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.invariant_owner_paths.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            for item in &file.items {
                let AirItem::MatchArm(arm) = item else {
                    continue;
                };
                if !is_bare_wildcard_pattern(&arm.pattern) {
                    continue;
                }
                if !is_silent_body_shape(arm.body_shape) {
                    continue;
                }
                if callsite_in_invariant_owner(
                    module_path,
                    arm.function.as_deref(),
                    &section.invariant_owner_paths,
                ) {
                    continue;
                }
                out.push(diagnostic_for_fl011(arm, module_path, mode));
            }
        }
    }
    out
}

/// True when the arm pattern is a bare wildcard — `_` or `_ if guard`.
/// We deliberately accept the guard form so `_ if cond => 0` still
/// trips FL011: a guarded silent default is the same anti-pattern.
fn is_bare_wildcard_pattern(pattern: &str) -> bool {
    let p = pattern.trim();
    p == "_" || p.starts_with("_ if ") || p.starts_with("_ @ ")
}

fn diagnostic_for_fl011(arm: &AirMatchArm, module_path: &str, mode: CheckMode) -> Diagnostic {
    let function_label = arm
        .function
        .as_deref()
        .unwrap_or("<unknown enclosing function>");
    let body_label = body_shape_label(arm.body_shape);
    Diagnostic {
        rule_id: "FL011".to_string(),
        severity: mode.elevate(Severity::Warning),
        span: arm.span.clone(),
        concept: None,
        message: format!(
            "bare `_` arm in `{module_path}` (fn `{function_label}`) returns a \
             `{body_label}` default — unknown variants silently routed to a sink"
        ),
        why: vec![
            format!("scrutinee `{}`", arm.scrutinee),
            format!("module `{module_path}`"),
            format!("enclosing function: `{function_label}`"),
            "arm pattern is `_`".into(),
            format!("arm body is a `{body_label}` (silent default)"),
            "an unknown enum variant should be an error or explicit ignore, \
             not a default-value fall-through"
                .into(),
        ],
        suggested_fix: Some(format!(
            "enumerate the missing variants explicitly so the compiler enforces \
             exhaustiveness; or, if the catch-all is intentional, rewrite as \
             `_ => Err(SomeError::Unknown)` so the failure has an owner. If \
             `{module_path}` is a legitimate invariant owner, add it to \
             `paradigms.FL.invariant_owner_paths`. For a one-off accepted \
             default, suppress with `// locus: allow FL011 reason=\"…\" \
             expires=\"YYYY-MM-DD\"`"
        )),
    }
}

/// FL010 — invalid input silently converted into a valid default state.
///
/// Reads [`AirItem::FallbackCall`] items (AIR v12). Fires when the
/// callee is `unwrap_or` or `or` AND the default-arg shape is
/// `Literal` or `Call`. Skips:
///
/// - `unwrap_or_default` — FL002's territory (covered by the default
///   `forbidden_callees` list).
/// - `default_shape == Empty` — same case as `unwrap_or_default`, no
///   explicit default.
/// - `default_shape == Block` or `Other` — multi-statement fallback
///   blocks might be doing real work (logging, propagation,
///   compensating action). Conservative skip; the deterministic
///   blocking rule shouldn't speculate about block contents.
///
/// Severity: `mode.elevate(Severity::Warning)` — Warning in human,
/// Fatal under `--agent-strict`.
///
/// Lockfile-driven silence: stays quiet until `invariant_owner_paths`
/// is populated, mirroring every other FL silent-discard rule.
pub fn fl010(air: &AirWorkspace, section: &FlSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.invariant_owner_paths.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            for item in &file.items {
                let AirItem::FallbackCall(call) = item else {
                    continue;
                };
                if !is_fl010_callee(&call.callee) {
                    continue;
                }
                if !is_fl010_default_shape(call.default_shape) {
                    continue;
                }
                if callsite_in_invariant_owner(
                    module_path,
                    call.function.as_deref(),
                    &section.invariant_owner_paths,
                ) {
                    continue;
                }
                out.push(diagnostic_for_fl010(call, module_path, mode));
            }
        }
    }
    out
}

/// True for the FL010-relevant fallback callees. `unwrap_or_default`
/// is deliberately excluded — FL002 covers the no-arg form.
fn is_fl010_callee(callee: &str) -> bool {
    matches!(callee, "unwrap_or" | "or")
}

/// True when the default-arg shape is one FL010 fires on. `Empty`
/// means the call had no default arg (i.e. `unwrap_or_default()`),
/// which FL002 already covers. `Block` and `Other` could be doing
/// real recovery work, so the deterministic rule passes on them.
fn is_fl010_default_shape(shape: ArmBodyShape) -> bool {
    matches!(shape, ArmBodyShape::Literal | ArmBodyShape::Call)
}

fn diagnostic_for_fl010(call: &AirFallbackCall, module_path: &str, mode: CheckMode) -> Diagnostic {
    let function_label = call
        .function
        .as_deref()
        .unwrap_or("<unknown enclosing function>");
    let shape_label = body_shape_label(call.default_shape);
    let shape_token = fallback_shape_token(call.default_shape);
    Diagnostic {
        rule_id: "FL010".to_string(),
        severity: mode.elevate(Severity::Warning),
        span: call.span.clone(),
        concept: None,
        message: format!(
            ".{}(...) returns a silent {shape_token} default in `{module_path}` \
             (fn `{function_label}`) — invalid input gets converted to valid state",
            call.callee,
        ),
        why: vec![
            format!("module `{module_path}`"),
            format!("enclosing function: `{function_label}`"),
            format!("callee `{}`", call.callee),
            format!("default-arg shape: {shape_label}"),
            "the failure path is silently replaced with a default value".into(),
            format!(
                "no `paradigms.FL.invariant_owner_paths` entry covers `{module_path}` \
                 — the rule treats this site as a non-owner"
            ),
        ],
        suggested_fix: Some(format!(
            "propagate the failure with `?`; or map the error to a typed \
             domain error (e.g. `.map_err(MyError::from)?`); or, if \
             `{module_path}` is a legitimate fallback owner (supervisor, \
             startup-asserting bin entry), add it to \
             `paradigms.FL.invariant_owner_paths`. For a one-off accepted \
             default, suppress with `// locus: allow FL010 reason=\"…\" \
             expires=\"YYYY-MM-DD\"`"
        )),
    }
}

/// Render `default_shape` as a snake_case token suitable for the
/// FL010 headline message — mirrors how FL011/FL007 use
/// `body_shape_label` but without spaces for the "silent X default"
/// phrasing.
fn fallback_shape_token(shape: ArmBodyShape) -> &'static str {
    match shape {
        ArmBodyShape::Empty => "empty",
        ArmBodyShape::Literal => "literal",
        ArmBodyShape::Call => "call",
        ArmBodyShape::Return => "return",
        ArmBodyShape::ErrorPropagation => "propagate",
        ArmBodyShape::Block => "block",
        ArmBodyShape::Other => "other",
    }
}

/// FL012 — retry-shaped loop without accepted retry policy.
///
/// Reads [`AirItem::RetryLoop`] items (AIR v12). Fires when:
///
/// - `propagates: true` (the loop body uses `?`),
/// - `has_break: true` (there's a success-exit path), and
/// - the file's `module_path` (or the function's containing module)
///   is **not** in `retry_policy_owner_paths`.
///
/// The user declares which modules legitimately implement retry policies
/// (backoff, max attempts, jitter). Loops elsewhere are likely ad-hoc
/// retries — repeated fallible work without the cross-cutting policy
/// concerns a real retry needs.
///
/// Severity: `mode.elevate(Severity::Warning)` — Warning in human,
/// Fatal under `--agent-strict`.
///
/// Lockfile-driven silence: stays quiet until `retry_policy_owner_paths`
/// is populated. Same UX shape as the other FL lockfile-driven rules.
pub fn fl012(air: &AirWorkspace, section: &FlSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.retry_policy_owner_paths.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            for item in &file.items {
                let AirItem::RetryLoop(loopy) = item else {
                    continue;
                };
                if !loopy.propagates {
                    continue;
                }
                if !loopy.has_break {
                    continue;
                }
                if callsite_in_invariant_owner(
                    module_path,
                    loopy.function.as_deref(),
                    &section.retry_policy_owner_paths,
                ) {
                    continue;
                }
                out.push(diagnostic_for_fl012(loopy, module_path, mode));
            }
        }
    }
    out
}

fn diagnostic_for_fl012(loopy: &AirRetryLoop, module_path: &str, mode: CheckMode) -> Diagnostic {
    let function_label = loopy
        .function
        .as_deref()
        .unwrap_or("<unknown enclosing function>");
    let kind_label = loop_kind_label(loopy.loop_kind);
    Diagnostic {
        rule_id: "FL012".to_string(),
        severity: mode.elevate(Severity::Warning),
        span: loopy.span.clone(),
        concept: None,
        message: format!(
            "retry-shaped {kind_label} loop in `{module_path}` (fn `{function_label}`) \
             — propagation + break with no declared retry policy"
        ),
        why: vec![
            format!("module `{module_path}`"),
            format!("enclosing function: `{function_label}`"),
            format!("loop kind: `{kind_label}`"),
            "loop body uses `?` and contains `break` — fits the \
             retry-without-policy shape"
                .into(),
            format!(
                "no `paradigms.FL.retry_policy_owner_paths` entry covers \
                 `{module_path}` — the rule treats this site as ad-hoc"
            ),
        ],
        suggested_fix: Some(format!(
            "extract the retry into a declared retry-policy module that \
             owns backoff, max attempts, and jitter; or, if `{module_path}` \
             is a legitimate retry owner, add it to \
             `paradigms.FL.retry_policy_owner_paths`. For a one-off accepted \
             retry, suppress with `// locus: allow FL012 reason=\"…\" \
             expires=\"YYYY-MM-DD\"`"
        )),
    }
}

/// Render a [`LoopKind`] for diagnostic messages. Lower-case so the
/// headline reads "retry-shaped loop / for / while loop in `…`".
fn loop_kind_label(kind: LoopKind) -> &'static str {
    match kind {
        LoopKind::Loop => "loop",
        LoopKind::For => "for",
        LoopKind::While => "while",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use locus_air::{
        AIR_SCHEMA_VERSION, AirFallbackCall, AirFile, AirFunction, AirPackage, AirRetryLoop,
        AirSpan, AirWorkspace, ArmBodyShape, LoopKind, Visibility,
    };

    fn func(name: &str, return_type: Option<&str>) -> AirItem {
        AirItem::Function(AirFunction {
            name: name.into(),
            symbol: format!("x::domain::user::{name}"),
            visibility: Visibility::Public,
            params: Vec::new(),
            return_type: return_type.map(str::to_string),
            span: AirSpan::new("src/domain/user.rs", 10, 20),
            line_count: 11,
            decorators: Vec::new(),
            symbol_segments: Vec::new(),
            doc: None,
        })
    }

    fn air_with_module(module: &str, items: Vec<AirItem>) -> AirWorkspace {
        AirWorkspace {
            schema_version: AIR_SCHEMA_VERSION,
            packages: vec![AirPackage {
                name: "x".into(),
                version: "0".into(),
                root_dir: "/".into(),
                files: vec![AirFile {
                    path: "src/domain/user.rs".into(),
                    module_path: Some(module.into()),
                    items,
                    hints: Vec::new(),
                    parse_error: None,
                    line_count: 50,
                }],
            }],
            facts: Vec::new(),
        }
    }

    fn domain_section() -> FlSection {
        FlSection {
            domain_paths: vec!["x::domain::*".into()],
            boundary_error_patterns: vec!["reqwest::Error".into(), "sqlx::*".into()],
            ..Default::default()
        }
    }

    // ---- fl001 behavioural tests ----

    #[test]
    fn fl001_fires_when_domain_function_returns_boundary_error() {
        let air = air_with_module(
            "x::domain::user",
            vec![func("fetch_user", Some("Result<User, reqwest::Error>"))],
        );
        let diags = fl001(&air, &domain_section(), CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "FL001");
        assert_eq!(diags[0].severity, Severity::Fatal);
        assert!(diags[0].message.contains("fetch_user"));
        assert!(diags[0].message.contains("reqwest::Error"));
        assert!(diags[0].message.contains("x::domain::*"));
        assert!(
            diags[0]
                .why
                .iter()
                .any(|w| w.contains("Result<User, reqwest::Error>")),
            "why list should surface the full return type; got: {:?}",
            diags[0].why
        );
    }

    #[test]
    fn fl001_fires_via_wildcard_boundary_pattern() {
        // `sqlx::*` matches `sqlx::Error` and any nested type.
        let air = air_with_module(
            "x::domain::orders",
            vec![func(
                "load_order",
                Some("Result<Order, sqlx::postgres::PgError>"),
            )],
        );
        let diags = fl001(&air, &domain_section(), CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("sqlx::postgres::PgError"));
        assert!(diags[0].message.contains("sqlx::*"));
    }

    #[test]
    fn fl001_quiet_when_function_returns_domain_error() {
        let air = air_with_module(
            "x::domain::user",
            vec![func("fetch_user", Some("Result<User, UserError>"))],
        );
        assert!(fl001(&air, &domain_section(), CheckMode::Human).is_empty());
    }

    #[test]
    fn fl001_quiet_outside_domain_paths() {
        // Same boundary error type, but the function lives in an adapter
        // module — perfectly legal there.
        let air = air_with_module(
            "x::adapters::http",
            vec![func("fetch_user", Some("Result<User, reqwest::Error>"))],
        );
        assert!(fl001(&air, &domain_section(), CheckMode::Human).is_empty());
    }

    #[test]
    fn fl001_silent_when_lockfile_lists_empty() {
        // Either list empty → rule must be silent. Two cases.
        let air = air_with_module(
            "x::domain::user",
            vec![func("fetch_user", Some("Result<User, reqwest::Error>"))],
        );
        let only_domain = FlSection {
            domain_paths: vec!["x::domain::*".into()],
            boundary_error_patterns: Vec::new(),
            ..Default::default()
        };
        assert!(fl001(&air, &only_domain, CheckMode::Human).is_empty());
        let only_boundary = FlSection {
            domain_paths: Vec::new(),
            boundary_error_patterns: vec!["reqwest::Error".into()],
            ..Default::default()
        };
        assert!(fl001(&air, &only_boundary, CheckMode::Human).is_empty());
        assert!(fl001(&air, &FlSection::default(), CheckMode::Human).is_empty());
    }

    #[test]
    fn fl001_quiet_on_non_result_return_type() {
        let air = air_with_module(
            "x::domain::user",
            vec![
                func("count_users", Some("u64")),
                func("noop", None),
                // Custom `Result<T>` alias with one type parameter — top-level
                // comma absent, so FL001 must skip it (no leakage to detect).
                func("custom_alias", Some("Result<User>")),
            ],
        );
        assert!(fl001(&air, &domain_section(), CheckMode::Human).is_empty());
    }

    #[test]
    fn fl001_handles_generics_in_ok_position() {
        // `Result<HashMap<K, V>, reqwest::Error>` — naive comma split would
        // pick `V>, reqwest::Error` as the error. The angle-bracket-aware
        // splitter must recover the right one.
        let air = air_with_module(
            "x::domain::user",
            vec![func(
                "fetch_users",
                Some("Result<HashMap<UserId, User>, reqwest::Error>"),
            )],
        );
        let diags = fl001(&air, &domain_section(), CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert!(
            diags[0].message.contains("`reqwest::Error`"),
            "extracted error type should be reqwest::Error, not the V>... fragment; got: {}",
            diags[0].message
        );
    }

    #[test]
    fn fl001_emits_one_diag_per_offending_function() {
        let air = air_with_module(
            "x::domain::user",
            vec![
                func("fetch_user", Some("Result<User, reqwest::Error>")),
                func("save_user", Some("Result<(), sqlx::Error>")),
                func("count_users", Some("u64")), // not flagged
                func("ok_user", Some("Result<User, UserError>")), // not flagged
            ],
        );
        let diags = fl001(&air, &domain_section(), CheckMode::Human);
        assert_eq!(diags.len(), 2);
        let messages: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
        assert!(messages.iter().any(|m| m.contains("fetch_user")));
        assert!(messages.iter().any(|m| m.contains("save_user")));
        assert!(!messages.iter().any(|m| m.contains("count_users")));
        assert!(!messages.iter().any(|m| m.contains("ok_user")));
    }

    #[test]
    fn fl001_severity_stays_fatal_in_human_mode() {
        // Unlike most paradigm rules where Human → Warning, FL001 is
        // structural enough that Human and AgentStrict are both Fatal.
        let air = air_with_module(
            "x::domain::user",
            vec![func("fetch_user", Some("Result<User, reqwest::Error>"))],
        );
        let human = fl001(&air, &domain_section(), CheckMode::Human);
        let strict = fl001(&air, &domain_section(), CheckMode::AgentStrict);
        assert_eq!(human.len(), 1);
        assert_eq!(strict.len(), 1);
        assert_eq!(human[0].severity, Severity::Fatal);
        assert_eq!(strict[0].severity, Severity::Fatal);
    }

    #[test]
    fn fl001_skips_files_with_no_module_path() {
        // No `module_path` → can't be matched against `domain_paths`. Stay
        // silent rather than guess from the file path.
        let air = AirWorkspace {
            schema_version: AIR_SCHEMA_VERSION,
            packages: vec![AirPackage {
                name: "x".into(),
                version: "0".into(),
                root_dir: "/".into(),
                files: vec![AirFile {
                    path: "src/orphan.rs".into(),
                    module_path: None,
                    items: vec![func("fetch_user", Some("Result<User, reqwest::Error>"))],
                    hints: Vec::new(),
                    parse_error: None,
                    line_count: 5,
                }],
            }],
            facts: Vec::new(),
        };
        assert!(fl001(&air, &domain_section(), CheckMode::Human).is_empty());
    }

    // ---- extract_result_error_type unit tests ----

    #[test]
    fn extract_result_error_type_basic() {
        assert_eq!(
            extract_result_error_type("Result<User, reqwest::Error>"),
            Some("reqwest::Error"),
        );
    }

    #[test]
    fn extract_result_error_type_with_generic_ok() {
        assert_eq!(
            extract_result_error_type("Result<HashMap<UserId, User>, reqwest::Error>"),
            Some("reqwest::Error"),
        );
        assert_eq!(
            extract_result_error_type("Result<Vec<(K, V)>, MyError>"),
            Some("MyError"),
        );
    }

    #[test]
    fn extract_result_error_type_rejects_malformed() {
        assert_eq!(extract_result_error_type("u64"), None);
        assert_eq!(extract_result_error_type("Result<User>"), None);
        assert_eq!(extract_result_error_type("Result<User, >"), None);
        assert_eq!(extract_result_error_type("Result<User, MyError"), None);
        // Stray closing bracket inside Ok position trips the depth check
        // before we ever find a top-level comma → rejected.
        assert_eq!(extract_result_error_type("Result<U>>, X>"), None);
    }

    #[test]
    fn extract_result_error_type_strips_leading_double_colon() {
        assert_eq!(
            extract_result_error_type("::Result<User, MyError>"),
            Some("MyError"),
        );
    }

    #[test]
    fn extract_result_error_type_does_not_match_qualified_result() {
        // We deliberately don't try to handle `std::result::Result<...>`. A
        // fully-qualified Result simply isn't matched — false positives on
        // user-defined `Result` aliases would be worse than missing this.
        assert_eq!(
            extract_result_error_type("std::result::Result<User, MyError>"),
            None,
        );
    }

    // ---- fl002 behavioural tests ----

    fn call_site(callee: &str, kind: CallKind, function: Option<&str>, line: u32) -> AirItem {
        AirItem::CallSite(AirCallSite {
            callee: callee.to_string(),
            kind,
            function: function.map(|s| s.to_string()),
            span: AirSpan::new("src/domain/user.rs", line, line),
        })
    }

    /// Onboarded baseline for FL002: at least one invariant-owner pattern is
    /// declared; default `forbidden_callees` covers the unwrap family.
    fn fl002_section() -> FlSection {
        FlSection {
            invariant_owner_paths: vec!["x::supervisor::*".into()],
            ..Default::default()
        }
    }

    /// Onboarded baseline for FL003: at least one invariant-owner pattern is
    /// declared; default `silent_discard_callees` covers the `.ok()` family.
    fn fl003_section() -> FlSection {
        FlSection {
            invariant_owner_paths: vec!["x::supervisor::*".into()],
            ..Default::default()
        }
    }

    #[test]
    fn fl002_fires_on_unwrap_method_call_in_non_invariant_owner_module() {
        let air = air_with_module(
            "x::domain::user",
            vec![call_site(
                "unwrap",
                CallKind::Method,
                Some("x::domain::user::greet"),
                7,
            )],
        );
        let diags = fl002(&air, &fl002_section(), CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "FL002");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(diags[0].concept.is_none());
        assert_eq!(diags[0].span.line_start, 7);
        assert!(
            diags[0].message.contains("unwrap"),
            "message should surface the callee; got: {}",
            diags[0].message,
        );
        assert!(
            diags[0].message.contains("x::domain::user"),
            "message should surface the module path; got: {}",
            diags[0].message,
        );
        assert!(
            diags[0]
                .why
                .iter()
                .any(|w| w.contains("invariant_owner_paths")),
            "why should reference invariant_owner_paths; got: {:?}",
            diags[0].why,
        );
        assert!(
            diags[0]
                .why
                .iter()
                .any(|w| w.contains("x::domain::user::greet")),
            "why should reference enclosing function; got: {:?}",
            diags[0].why,
        );
        assert!(
            diags[0]
                .why
                .iter()
                .any(|w| w.contains("forbidden_callees") || w.contains("unwrap")),
            "why should reference the forbidden callee; got: {:?}",
            diags[0].why,
        );
    }

    #[test]
    fn fl002_fires_on_panic_macro_call_without_double_colon() {
        // `panic!` invocation: visitor records `callee = "panic"`, `Macro` kind.
        let air = air_with_module(
            "x::domain::user",
            vec![call_site(
                "panic",
                CallKind::Meta,
                Some("x::domain::user::oops"),
                12,
            )],
        );
        let diags = fl002(&air, &fl002_section(), CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert!(
            diags[0].message.contains("panic"),
            "message should mention the panic callee; got: {}",
            diags[0].message,
        );
    }

    #[test]
    fn fl002_quiet_when_callee_is_in_invariant_owner_module() {
        // Same `unwrap` call, but the file's module_path matches an invariant
        // owner pattern — accepted, no diagnostic.
        let air = air_with_module(
            "x::supervisor::startup",
            vec![call_site(
                "unwrap",
                CallKind::Method,
                Some("x::supervisor::startup::boot"),
                3,
            )],
        );
        assert!(fl002(&air, &fl002_section(), CheckMode::Human).is_empty());
    }

    #[test]
    fn fl002_silent_when_invariant_owner_paths_empty() {
        // No `invariant_owner_paths` configured → silent posture, even though
        // `forbidden_callees` defaults are non-empty.
        let air = air_with_module(
            "x::domain::user",
            vec![call_site(
                "unwrap",
                CallKind::Method,
                Some("x::domain::user::greet"),
                7,
            )],
        );
        let section = FlSection::default();
        assert!(
            fl002(&air, &section, CheckMode::Human).is_empty(),
            "rule should wait for explicit invariant_owner_paths declaration",
        );
    }

    #[test]
    fn fl002_agent_strict_elevates_warning_to_fatal() {
        let air = air_with_module(
            "x::domain::user",
            vec![call_site(
                "unwrap",
                CallKind::Method,
                Some("x::domain::user::greet"),
                7,
            )],
        );
        let diags = fl002(&air, &fl002_section(), CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    #[test]
    fn fl002_quiet_on_function_kind_callees() {
        // Function-shaped calls are intentionally excluded — a user free
        // function named `unwrap` shouldn't trip FL002.
        let air = air_with_module(
            "x::domain::user",
            vec![call_site(
                "unwrap",
                CallKind::Function,
                Some("x::domain::user::greet"),
                7,
            )],
        );
        assert!(fl002(&air, &fl002_section(), CheckMode::Human).is_empty());
    }

    #[test]
    fn fl002_quiet_on_unrelated_callees() {
        let air = air_with_module(
            "x::domain::user",
            vec![call_site(
                "len",
                CallKind::Method,
                Some("x::domain::user::greet"),
                7,
            )],
        );
        assert!(fl002(&air, &fl002_section(), CheckMode::Human).is_empty());
    }

    #[test]
    fn fl002_matches_path_qualified_macro_via_last_segment() {
        // `std::panic!` → callee `"std::panic"`. We match on the last `::`
        // segment, so this should still fire.
        let air = air_with_module(
            "x::domain::user",
            vec![call_site(
                "std::panic",
                CallKind::Meta,
                Some("x::domain::user::oops"),
                7,
            )],
        );
        let diags = fl002(&air, &fl002_section(), CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert!(
            diags[0].message.contains("std::panic"),
            "message should preserve the full callee; got: {}",
            diags[0].message,
        );
    }

    // ---- fl003 behavioural tests ----

    #[test]
    fn fl003_fires_on_dot_ok_method_call_in_non_invariant_owner_module() {
        let air = air_with_module(
            "x::domain::user",
            vec![call_site(
                "ok",
                CallKind::Method,
                Some("x::domain::user::greet"),
                12,
            )],
        );
        let diags = fl003(&air, &fl003_section(), CheckMode::Human);
        assert_eq!(
            diags.len(),
            1,
            "expected one FL003 diagnostic; got {diags:?}"
        );
        assert_eq!(diags[0].rule_id, "FL003");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(diags[0].message.contains("silent error discard"));
        assert!(diags[0].message.contains("ok"));
        assert!(
            diags[0]
                .why
                .iter()
                .any(|w| w.contains("silent_discard_callees")),
            "why list should reference the lockfile field; got: {:?}",
            diags[0].why,
        );
    }

    #[test]
    fn fl003_fires_on_dot_err_method_call() {
        let air = air_with_module(
            "x::domain::user",
            vec![call_site(
                "err",
                CallKind::Method,
                Some("x::domain::user::lookup"),
                30,
            )],
        );
        let diags = fl003(&air, &fl003_section(), CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("err"));
    }

    #[test]
    fn fl003_quiet_on_function_kind_callsites() {
        // A free function literally named `ok` shouldn't trip FL003 — only
        // method calls carry the silent-discard semantics on Result.
        let air = air_with_module(
            "x::domain::user",
            vec![call_site(
                "ok",
                CallKind::Function,
                Some("x::domain::user::greet"),
                7,
            )],
        );
        assert!(fl003(&air, &fl003_section(), CheckMode::Human).is_empty());
    }

    #[test]
    fn fl003_quiet_in_invariant_owner_module() {
        let air = air_with_module(
            "x::supervisor::root",
            vec![call_site(
                "ok",
                CallKind::Method,
                Some("x::supervisor::root::run"),
                4,
            )],
        );
        assert!(fl003(&air, &fl003_section(), CheckMode::Human).is_empty());
    }

    #[test]
    fn fl003_silent_when_invariant_owner_paths_empty() {
        let air = air_with_module(
            "x::domain::user",
            vec![call_site(
                "ok",
                CallKind::Method,
                Some("x::domain::user::greet"),
                12,
            )],
        );
        assert!(
            fl003(&air, &FlSection::default(), CheckMode::Human).is_empty(),
            "rule should wait for explicit invariant_owner_paths declaration",
        );
    }

    #[test]
    fn fl003_silent_when_silent_discard_callees_empty() {
        let air = air_with_module(
            "x::domain::user",
            vec![call_site(
                "ok",
                CallKind::Method,
                Some("x::domain::user::greet"),
                12,
            )],
        );
        let section = FlSection {
            invariant_owner_paths: vec!["x::supervisor::*".into()],
            silent_discard_callees: Vec::new(),
            ..Default::default()
        };
        assert!(fl003(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn fl003_quiet_on_unrelated_callees() {
        let air = air_with_module(
            "x::domain::user",
            vec![
                call_site("len", CallKind::Method, Some("x::domain::user::greet"), 4),
                call_site("clone", CallKind::Method, Some("x::domain::user::greet"), 5),
            ],
        );
        assert!(fl003(&air, &fl003_section(), CheckMode::Human).is_empty());
    }

    #[test]
    fn fl003_agent_strict_elevates_warning_to_fatal() {
        let air = air_with_module(
            "x::domain::user",
            vec![call_site(
                "ok",
                CallKind::Method,
                Some("x::domain::user::greet"),
                12,
            )],
        );
        let diags = fl003(&air, &fl003_section(), CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    // ---- fl004 + fl005 helpers ----

    fn discard(
        callee: Option<&str>,
        kind: locus_air::DiscardKind,
        function: Option<&str>,
        line: u32,
    ) -> AirItem {
        AirItem::SilentDiscard(locus_air::AirSilentDiscard {
            callee: callee.map(|s| s.to_string()),
            kind,
            function: function.map(|s| s.to_string()),
            span: AirSpan::new("src/domain/user.rs", line, line),
        })
    }

    fn partial_if_let(variant: &str, function: Option<&str>, line: u32) -> AirItem {
        // AIR v13: variant is a typed enum, not a String. Map the
        // legacy "Ok"/"Err" test fixture vocabulary to the new
        // ResultMatchVariant.
        let variant = match variant {
            "Ok" => locus_air::ResultMatchVariant::Success,
            "Err" => locus_air::ResultMatchVariant::Failure,
            other => panic!("test fixture passed unexpected variant: {other}"),
        };
        AirItem::PartialResultMatch(locus_air::AirPartialResultMatch {
            variant,
            function: function.map(|s| s.to_string()),
            span: AirSpan::new("src/domain/user.rs", line, line),
        })
    }

    /// Onboarded baseline for FL004 / FL005 — same shape as the FL003
    /// baseline; both rules consult `invariant_owner_paths` only and
    /// rely on the seeded `silent_discard_allowed_callees` defaults.
    fn fl004_section() -> FlSection {
        FlSection {
            invariant_owner_paths: vec!["x::supervisor::*".into()],
            ..Default::default()
        }
    }

    // ---- fl004 behavioural tests ----

    #[test]
    fn fl004_fires_on_discarded_method_call_in_non_invariant_owner_module() {
        let air = air_with_module(
            "x::domain::user",
            vec![discard(
                Some("write"),
                locus_air::DiscardKind::Method,
                Some("x::domain::user::greet"),
                17,
            )],
        );
        let diags = fl004(&air, &fl004_section(), CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "FL004");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(diags[0].message.contains("write"));
        assert!(diags[0].message.contains("let _ ="));
    }

    #[test]
    fn fl004_quiet_on_allowlist_callees() {
        // `lock`, `send`, `drop`, `set_logger`, `subscribe`, `try_init`
        // are seeded as legitimate fire-and-forget patterns.
        let air = air_with_module(
            "x::domain::user",
            vec![
                discard(
                    Some("lock"),
                    locus_air::DiscardKind::Method,
                    Some("x::domain::user::greet"),
                    1,
                ),
                discard(
                    Some("send"),
                    locus_air::DiscardKind::Method,
                    Some("x::domain::user::greet"),
                    2,
                ),
                discard(
                    Some("drop"),
                    locus_air::DiscardKind::Function,
                    Some("x::domain::user::greet"),
                    3,
                ),
            ],
        );
        assert!(fl004(&air, &fl004_section(), CheckMode::Human).is_empty());
    }

    #[test]
    fn fl004_quiet_on_other_kind_discards() {
        // `let _ = some_field;` is `DiscardKind::Other` — we don't flag
        // arbitrary expression discards (false-positive surface too large).
        let air = air_with_module(
            "x::domain::user",
            vec![discard(
                None,
                locus_air::DiscardKind::Other,
                Some("x::domain::user::greet"),
                4,
            )],
        );
        assert!(fl004(&air, &fl004_section(), CheckMode::Human).is_empty());
    }

    #[test]
    fn fl004_quiet_in_invariant_owner_module() {
        let air = air_with_module(
            "x::supervisor::root",
            vec![discard(
                Some("write"),
                locus_air::DiscardKind::Method,
                Some("x::supervisor::root::run"),
                17,
            )],
        );
        assert!(fl004(&air, &fl004_section(), CheckMode::Human).is_empty());
    }

    #[test]
    fn fl004_silent_when_invariant_owner_paths_empty() {
        let air = air_with_module(
            "x::domain::user",
            vec![discard(
                Some("write"),
                locus_air::DiscardKind::Method,
                Some("x::domain::user::greet"),
                17,
            )],
        );
        assert!(fl004(&air, &FlSection::default(), CheckMode::Human).is_empty());
    }

    #[test]
    fn fl004_agent_strict_elevates_warning_to_fatal() {
        let air = air_with_module(
            "x::domain::user",
            vec![discard(
                Some("write"),
                locus_air::DiscardKind::Method,
                Some("x::domain::user::greet"),
                17,
            )],
        );
        let diags = fl004(&air, &fl004_section(), CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    // ---- fl005 behavioural tests ----

    #[test]
    fn fl005_fires_on_partial_if_let_ok_in_non_invariant_owner_module() {
        let air = air_with_module(
            "x::domain::user",
            vec![partial_if_let("Ok", Some("x::domain::user::greet"), 22)],
        );
        let diags = fl005(&air, &fl004_section(), CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "FL005");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(diags[0].message.contains("if let Ok"));
        assert!(diags[0].message.contains("Err"));
    }

    #[test]
    fn fl005_fires_on_partial_if_let_err() {
        let air = air_with_module(
            "x::domain::user",
            vec![partial_if_let("Err", Some("x::domain::user::greet"), 22)],
        );
        let diags = fl005(&air, &fl004_section(), CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("if let Err"));
        assert!(diags[0].message.contains("Ok"));
    }

    #[test]
    fn fl005_quiet_in_invariant_owner_module() {
        let air = air_with_module(
            "x::supervisor::root",
            vec![partial_if_let("Ok", Some("x::supervisor::root::run"), 22)],
        );
        assert!(fl005(&air, &fl004_section(), CheckMode::Human).is_empty());
    }

    #[test]
    fn fl005_silent_when_invariant_owner_paths_empty() {
        let air = air_with_module(
            "x::domain::user",
            vec![partial_if_let("Ok", Some("x::domain::user::greet"), 22)],
        );
        assert!(fl005(&air, &FlSection::default(), CheckMode::Human).is_empty());
    }

    #[test]
    fn fl005_agent_strict_elevates_warning_to_fatal() {
        let air = air_with_module(
            "x::domain::user",
            vec![partial_if_let("Ok", Some("x::domain::user::greet"), 22)],
        );
        let diags = fl005(&air, &fl004_section(), CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    // ---- fl013 behavioural tests ----

    /// Onboarded baseline for FL013 — same shape as fl003/fl004 baselines.
    fn fl013_section() -> FlSection {
        FlSection {
            invariant_owner_paths: vec!["x::supervisor::*".into()],
            ..Default::default()
        }
    }

    #[test]
    fn fl013_fires_when_string_result_fn_calls_to_string() {
        // `fn save() -> Result<(), String>` body contains `.to_string()`.
        let air = air_with_module(
            "x::domain::user",
            vec![
                func("save", Some("Result<(), String>")),
                call_site(
                    "to_string",
                    CallKind::Method,
                    Some("x::domain::user::save"),
                    14,
                ),
            ],
        );
        let diags = fl013(&air, &fl013_section(), CheckMode::Human);
        assert_eq!(diags.len(), 1, "expected one FL013 diag, got {diags:?}");
        assert_eq!(diags[0].rule_id, "FL013");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert_eq!(diags[0].span.line_start, 14);
        assert!(diags[0].message.contains("save"));
        assert!(diags[0].message.contains("to_string"));
        assert!(diags[0].message.contains("Result<(), String>"));
        assert!(
            diags[0]
                .suggested_fix
                .as_deref()
                .unwrap_or("")
                .contains("?"),
            "suggested fix should mention `?` propagation; got: {:?}",
            diags[0].suggested_fix,
        );
    }

    #[test]
    fn fl013_fires_on_format_macro_in_str_result() {
        // `fn save() -> Result<(), &str>` body contains `format!(...)`.
        let air = air_with_module(
            "x::domain::user",
            vec![
                func("describe", Some("Result<String, &str>")),
                call_site(
                    "format",
                    CallKind::Meta,
                    Some("x::domain::user::describe"),
                    7,
                ),
            ],
        );
        let diags = fl013(&air, &fl013_section(), CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("describe"));
        assert!(diags[0].message.contains("format"));
    }

    #[test]
    fn fl013_quiet_when_error_type_is_typed() {
        // `Result<_, MyError>` is fine — even if the function calls
        // `.to_string()` somewhere internally, FL013 doesn't apply.
        let air = air_with_module(
            "x::domain::user",
            vec![
                func("greet", Some("Result<(), MyError>")),
                call_site(
                    "to_string",
                    CallKind::Method,
                    Some("x::domain::user::greet"),
                    3,
                ),
            ],
        );
        assert!(fl013(&air, &fl013_section(), CheckMode::Human).is_empty());
    }

    #[test]
    fn fl013_quiet_when_string_fn_has_no_stringification_call() {
        // Function returns `Result<_, String>` but its body has only a
        // `.len()` call — no FL013 trigger.
        let air = air_with_module(
            "x::domain::user",
            vec![
                func("save", Some("Result<(), String>")),
                call_site("len", CallKind::Method, Some("x::domain::user::save"), 4),
            ],
        );
        assert!(fl013(&air, &fl013_section(), CheckMode::Human).is_empty());
    }

    #[test]
    fn fl013_quiet_in_invariant_owner_module() {
        // Same offending pattern, but the file is on the invariant-owner
        // allowlist (file_module matches `x::supervisor::*`). FL013 must
        // suppress the diagnostic. We build the AIR explicitly so the
        // module_path and the enclosing-fn symbols line up under
        // `x::supervisor::*` (the shared `func` helper hardcodes a
        // domain-shaped symbol prefix).
        let air = AirWorkspace {
            schema_version: AIR_SCHEMA_VERSION,
            packages: vec![AirPackage {
                name: "x".into(),
                version: "0".into(),
                root_dir: "/".into(),
                files: vec![AirFile {
                    path: "src/supervisor/root.rs".into(),
                    module_path: Some("x::supervisor::root".into()),
                    items: vec![
                        AirItem::Function(AirFunction {
                            name: "save".into(),
                            symbol: "x::supervisor::root::save".into(),
                            visibility: Visibility::Public,
                            params: Vec::new(),
                            return_type: Some("Result<(), String>".into()),
                            span: AirSpan::new("src/supervisor/root.rs", 1, 5),
                            line_count: 5,
                            decorators: Vec::new(),
                            symbol_segments: Vec::new(),
                            doc: None,
                        }),
                        AirItem::CallSite(AirCallSite {
                            callee: "to_string".into(),
                            kind: CallKind::Method,
                            function: Some("x::supervisor::root::save".into()),
                            span: AirSpan::new("src/supervisor/root.rs", 3, 3),
                        }),
                    ],
                    hints: Vec::new(),
                    parse_error: None,
                    line_count: 5,
                }],
            }],
            facts: Vec::new(),
        };
        assert!(fl013(&air, &fl013_section(), CheckMode::Human).is_empty());
    }

    #[test]
    fn fl013_silent_when_invariant_owner_paths_empty() {
        let air = air_with_module(
            "x::domain::user",
            vec![
                func("save", Some("Result<(), String>")),
                call_site(
                    "to_string",
                    CallKind::Method,
                    Some("x::domain::user::save"),
                    14,
                ),
            ],
        );
        assert!(fl013(&air, &FlSection::default(), CheckMode::Human).is_empty());
    }

    #[test]
    fn fl013_agent_strict_elevates_warning_to_fatal() {
        let air = air_with_module(
            "x::domain::user",
            vec![
                func("save", Some("Result<(), String>")),
                call_site(
                    "to_string",
                    CallKind::Method,
                    Some("x::domain::user::save"),
                    14,
                ),
            ],
        );
        let diags = fl013(&air, &fl013_section(), CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    // ---- fl006 / fl007 / fl011 helpers ----

    fn closure_method_call(
        callee: &str,
        discards: bool,
        body_shape: ArmBodyShape,
        function: Option<&str>,
        line: u32,
    ) -> AirItem {
        AirItem::ClosureMethodCall(AirClosureMethodCall {
            callee: callee.to_string(),
            closure_discards_arg: discards,
            body_shape,
            function: function.map(|s| s.to_string()),
            span: AirSpan::new("src/domain/user.rs", line, line),
        })
    }

    fn match_arm(
        scrutinee: &str,
        pattern: &str,
        has_wildcard: bool,
        body_shape: ArmBodyShape,
        function: Option<&str>,
        line: u32,
    ) -> AirItem {
        AirItem::MatchArm(AirMatchArm {
            scrutinee: scrutinee.to_string(),
            pattern: pattern.to_string(),
            pattern_has_wildcard: has_wildcard,
            body_shape,
            function: function.map(|s| s.to_string()),
            span: AirSpan::new("src/domain/user.rs", line, line),
        })
    }

    /// Onboarded baseline — same shape as fl003/fl004/fl005 baselines;
    /// FL006/FL007/FL011 all consult `invariant_owner_paths` only.
    fn fl_arm_section() -> FlSection {
        FlSection {
            invariant_owner_paths: vec!["x::supervisor::*".into()],
            ..Default::default()
        }
    }

    // ---- fl006 behavioural tests ----

    #[test]
    fn fl006_fires_on_map_err_with_underscore_closure_in_non_invariant_owner_module() {
        let air = air_with_module(
            "x::domain::user",
            vec![closure_method_call(
                "map_err",
                true,
                ArmBodyShape::Call,
                Some("x::domain::user::greet"),
                42,
            )],
        );
        let diags = fl006(&air, &fl_arm_section(), CheckMode::Human);
        assert_eq!(diags.len(), 1, "expected one FL006 diag, got {diags:?}");
        assert_eq!(diags[0].rule_id, "FL006");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(diags[0].concept.is_none());
        assert_eq!(diags[0].span.line_start, 42);
        assert!(
            diags[0].message.contains("map_err"),
            "message should surface the callee; got: {}",
            diags[0].message,
        );
        assert!(diags[0].message.contains("x::domain::user"));
        assert!(diags[0].message.contains("x::domain::user::greet"));
        assert!(
            diags[0]
                .why
                .iter()
                .any(|w| w.contains("closure pattern is `_`")),
            "why list should call out the discarded closure arg; got: {:?}",
            diags[0].why,
        );
    }

    #[test]
    fn fl006_fires_inside_inline_test_module_when_test_paths_not_in_invariant_owners() {
        // Inline `mod tests { fn x() { result.map_err(|_| ()); } }` —
        // file `module_path` is `x::domain::user`, function symbol is
        // `x::domain::user::tests::it_works`. `containing_module_of`
        // strips the last segment → `x::domain::user::tests`, which
        // doesn't match the supervisor-only baseline → FL006 still fires.
        let air = air_with_module(
            "x::domain::user",
            vec![closure_method_call(
                "map_err",
                true,
                ArmBodyShape::Call,
                Some("x::domain::user::tests::it_works"),
                7,
            )],
        );
        let diags = fl006(&air, &fl_arm_section(), CheckMode::Human);
        assert_eq!(diags.len(), 1);
        // Now make the test module an invariant owner — diagnostic disappears.
        let owner_section = FlSection {
            invariant_owner_paths: vec!["*::tests".into()],
            ..Default::default()
        };
        assert!(fl006(&air, &owner_section, CheckMode::Human).is_empty());
    }

    #[test]
    fn fl006_quiet_when_closure_uses_its_arg() {
        // `result.map_err(|e| MyError::from(e))` — `closure_discards_arg`
        // is false. No FL006.
        let air = air_with_module(
            "x::domain::user",
            vec![closure_method_call(
                "map_err",
                false,
                ArmBodyShape::Call,
                Some("x::domain::user::greet"),
                42,
            )],
        );
        assert!(fl006(&air, &fl_arm_section(), CheckMode::Human).is_empty());
    }

    #[test]
    fn fl006_quiet_when_callee_is_not_map_err() {
        // `unwrap_or_else(|_| ...)` is FL003's territory; FL006 must
        // ignore non-`map_err` callees regardless of closure shape.
        let air = air_with_module(
            "x::domain::user",
            vec![
                closure_method_call(
                    "unwrap_or_else",
                    true,
                    ArmBodyShape::Call,
                    Some("x::domain::user::greet"),
                    1,
                ),
                closure_method_call(
                    "or_else",
                    true,
                    ArmBodyShape::Call,
                    Some("x::domain::user::greet"),
                    2,
                ),
                closure_method_call(
                    "and_then",
                    true,
                    ArmBodyShape::Call,
                    Some("x::domain::user::greet"),
                    3,
                ),
            ],
        );
        assert!(fl006(&air, &fl_arm_section(), CheckMode::Human).is_empty());
    }

    #[test]
    fn fl006_silent_when_invariant_owner_paths_empty() {
        let air = air_with_module(
            "x::domain::user",
            vec![closure_method_call(
                "map_err",
                true,
                ArmBodyShape::Call,
                Some("x::domain::user::greet"),
                42,
            )],
        );
        assert!(fl006(&air, &FlSection::default(), CheckMode::Human).is_empty());
    }

    #[test]
    fn fl006_quiet_in_invariant_owner_module() {
        let air = air_with_module(
            "x::supervisor::root",
            vec![closure_method_call(
                "map_err",
                true,
                ArmBodyShape::Call,
                Some("x::supervisor::root::run"),
                7,
            )],
        );
        assert!(fl006(&air, &fl_arm_section(), CheckMode::Human).is_empty());
    }

    #[test]
    fn fl006_agent_strict_elevates_warning_to_fatal() {
        let air = air_with_module(
            "x::domain::user",
            vec![closure_method_call(
                "map_err",
                true,
                ArmBodyShape::Call,
                Some("x::domain::user::greet"),
                42,
            )],
        );
        let diags = fl006(&air, &fl_arm_section(), CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    // ---- fl007 behavioural tests ----

    #[test]
    fn fl007_fires_on_err_underscore_arm_with_literal_body() {
        // `match result { Ok(x) => x, Err(_) => 0 }` — Err arm body is
        // `Literal`, FL007 fires.
        let air = air_with_module(
            "x::domain::user",
            vec![match_arm(
                "result",
                "Err(_)",
                true,
                ArmBodyShape::Literal,
                Some("x::domain::user::lookup"),
                12,
            )],
        );
        let diags = fl007(&air, &fl_arm_section(), CheckMode::Human);
        assert_eq!(diags.len(), 1, "expected one FL007 diag, got {diags:?}");
        assert_eq!(diags[0].rule_id, "FL007");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(diags[0].concept.is_none());
        assert!(diags[0].message.contains("Err(_)"));
        assert!(diags[0].message.contains("silently swallows"));
        assert!(
            diags[0]
                .why
                .iter()
                .any(|w| w.contains("arm pattern `Err(_)`")),
            "why should surface the pattern text; got: {:?}",
            diags[0].why,
        );
    }

    #[test]
    fn fl007_fires_on_err_underscore_arm_with_call_body() {
        // `Err(_) => Default::default()` — body shape is `Call`. Still silent.
        let air = air_with_module(
            "x::domain::user",
            vec![match_arm(
                "result",
                "Err(_)",
                true,
                ArmBodyShape::Call,
                Some("x::domain::user::lookup"),
                4,
            )],
        );
        let diags = fl007(&air, &fl_arm_section(), CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("call expression"));
    }

    #[test]
    fn fl007_quiet_when_arm_body_propagates() {
        // `Err(e) => return Err(e.into())` — well, propagation by `?` is
        // the canonical example. Body shape is `Propagate`; FL007 stays
        // quiet because the arm has explicitly handled the failure.
        let air = air_with_module(
            "x::domain::user",
            vec![match_arm(
                "result",
                "Err(_)",
                true,
                ArmBodyShape::ErrorPropagation,
                Some("x::domain::user::lookup"),
                4,
            )],
        );
        assert!(fl007(&air, &fl_arm_section(), CheckMode::Human).is_empty());
    }

    #[test]
    fn fl007_quiet_when_arm_body_returns() {
        // `Err(_) => return None` — body is `Return`. Control flow leaves
        // the function explicitly, so the failure has an owner.
        let air = air_with_module(
            "x::domain::user",
            vec![match_arm(
                "result",
                "Err(_)",
                true,
                ArmBodyShape::Return,
                Some("x::domain::user::lookup"),
                4,
            )],
        );
        assert!(fl007(&air, &fl_arm_section(), CheckMode::Human).is_empty());
    }

    #[test]
    fn fl007_quiet_when_pattern_does_not_target_err() {
        // `Ok(_) => 0` — wildcard binder is present but the pattern
        // doesn't match `Err`. FL011 reasons about `_` patterns; FL007
        // is the `Err`-specific rule and must not fire here.
        let air = air_with_module(
            "x::domain::user",
            vec![match_arm(
                "result",
                "Ok(_)",
                true,
                ArmBodyShape::Literal,
                Some("x::domain::user::lookup"),
                4,
            )],
        );
        assert!(fl007(&air, &fl_arm_section(), CheckMode::Human).is_empty());
    }

    #[test]
    fn fl007_silent_when_invariant_owner_paths_empty() {
        let air = air_with_module(
            "x::domain::user",
            vec![match_arm(
                "result",
                "Err(_)",
                true,
                ArmBodyShape::Literal,
                Some("x::domain::user::lookup"),
                12,
            )],
        );
        assert!(fl007(&air, &FlSection::default(), CheckMode::Human).is_empty());
    }

    #[test]
    fn fl007_agent_strict_elevates_warning_to_fatal() {
        let air = air_with_module(
            "x::domain::user",
            vec![match_arm(
                "result",
                "Err(_)",
                true,
                ArmBodyShape::Literal,
                Some("x::domain::user::lookup"),
                12,
            )],
        );
        let diags = fl007(&air, &fl_arm_section(), CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    // ---- fl011 behavioural tests ----

    #[test]
    fn fl011_fires_on_bare_wildcard_arm_with_literal_body() {
        // `match status { Status::A => 1, _ => 0 }` — bare `_` arm body
        // is `Literal`. FL011 fires.
        let air = air_with_module(
            "x::domain::user",
            vec![match_arm(
                "status",
                "_",
                true,
                ArmBodyShape::Literal,
                Some("x::domain::user::classify"),
                9,
            )],
        );
        let diags = fl011(&air, &fl_arm_section(), CheckMode::Human);
        assert_eq!(diags.len(), 1, "expected one FL011 diag, got {diags:?}");
        assert_eq!(diags[0].rule_id, "FL011");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(diags[0].concept.is_none());
        assert!(diags[0].message.contains("bare `_` arm"));
        assert!(diags[0].message.contains("literal default"));
        assert!(
            diags[0]
                .why
                .iter()
                .any(|w| w.contains("scrutinee `status`")),
            "why should surface the scrutinee; got: {:?}",
            diags[0].why,
        );
    }

    #[test]
    fn fl011_fires_on_bare_wildcard_arm_with_call_body() {
        // `_ => Default::default()` — body shape `Call`. Still silent.
        let air = air_with_module(
            "x::domain::user",
            vec![match_arm(
                "status",
                "_",
                true,
                ArmBodyShape::Call,
                Some("x::domain::user::classify"),
                3,
            )],
        );
        let diags = fl011(&air, &fl_arm_section(), CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("call expression"));
    }

    #[test]
    fn fl011_quiet_on_err_underscore_pattern() {
        // `Err(_)` is FL007's territory. FL011 must ignore patterns that
        // aren't the bare `_`.
        let air = air_with_module(
            "x::domain::user",
            vec![match_arm(
                "result",
                "Err(_)",
                true,
                ArmBodyShape::Literal,
                Some("x::domain::user::lookup"),
                4,
            )],
        );
        assert!(fl011(&air, &fl_arm_section(), CheckMode::Human).is_empty());
    }

    #[test]
    fn fl011_quiet_when_arm_body_returns() {
        // `_ => return None` — body is `Return`. Explicit handling, no FL011.
        let air = air_with_module(
            "x::domain::user",
            vec![match_arm(
                "status",
                "_",
                true,
                ArmBodyShape::Return,
                Some("x::domain::user::classify"),
                3,
            )],
        );
        assert!(fl011(&air, &fl_arm_section(), CheckMode::Human).is_empty());
    }

    #[test]
    fn fl011_quiet_when_arm_body_is_block() {
        // Multi-statement block — could be doing real work. FL011 doesn't
        // pre-judge.
        let air = air_with_module(
            "x::domain::user",
            vec![match_arm(
                "status",
                "_",
                true,
                ArmBodyShape::Block,
                Some("x::domain::user::classify"),
                3,
            )],
        );
        assert!(fl011(&air, &fl_arm_section(), CheckMode::Human).is_empty());
    }

    #[test]
    fn fl011_silent_when_invariant_owner_paths_empty() {
        let air = air_with_module(
            "x::domain::user",
            vec![match_arm(
                "status",
                "_",
                true,
                ArmBodyShape::Literal,
                Some("x::domain::user::classify"),
                9,
            )],
        );
        assert!(fl011(&air, &FlSection::default(), CheckMode::Human).is_empty());
    }

    #[test]
    fn fl011_agent_strict_elevates_warning_to_fatal() {
        let air = air_with_module(
            "x::domain::user",
            vec![match_arm(
                "status",
                "_",
                true,
                ArmBodyShape::Literal,
                Some("x::domain::user::classify"),
                9,
            )],
        );
        let diags = fl011(&air, &fl_arm_section(), CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    // ---- fl010 / fl012 helpers ----

    fn fallback_call(
        callee: &str,
        shape: ArmBodyShape,
        function: Option<&str>,
        line: u32,
    ) -> AirItem {
        AirItem::FallbackCall(AirFallbackCall {
            pattern: locus_air::FallbackPattern::ValueOr,
            callee: callee.to_string(),
            default_shape: shape,
            function: function.map(|s| s.to_string()),
            span: AirSpan::new("src/domain/user.rs", line, line),
        })
    }

    fn retry_loop(
        kind: LoopKind,
        propagates: bool,
        has_break: bool,
        function: Option<&str>,
        line: u32,
    ) -> AirItem {
        AirItem::RetryLoop(AirRetryLoop {
            loop_kind: kind,
            propagates,
            has_break,
            function: function.map(|s| s.to_string()),
            span: AirSpan::new("src/domain/user.rs", line, line),
        })
    }

    /// Onboarded baseline for FL012 — populates `retry_policy_owner_paths`
    /// with a single supervisor pattern. Mirrors `fl_arm_section`'s shape.
    fn fl012_section() -> FlSection {
        FlSection {
            retry_policy_owner_paths: vec!["x::retry::*".into()],
            ..Default::default()
        }
    }

    // ---- fl010 behavioural tests ----

    #[test]
    fn fl010_fires_on_unwrap_or_with_literal_default() {
        // `result.unwrap_or(0)` outside any invariant owner.
        let air = air_with_module(
            "x::domain::user",
            vec![fallback_call(
                "unwrap_or",
                ArmBodyShape::Literal,
                Some("x::domain::user::greet"),
                42,
            )],
        );
        let diags = fl010(&air, &fl_arm_section(), CheckMode::Human);
        assert_eq!(diags.len(), 1, "expected one FL010 diag, got {diags:?}");
        assert_eq!(diags[0].rule_id, "FL010");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(diags[0].concept.is_none());
        assert_eq!(diags[0].span.line_start, 42);
        assert!(
            diags[0].message.contains(".unwrap_or(...)"),
            "message should surface the callee; got: {}",
            diags[0].message,
        );
        assert!(diags[0].message.contains("silent literal default"));
        assert!(diags[0].message.contains("x::domain::user"));
        assert!(diags[0].message.contains("x::domain::user::greet"));
        assert!(
            diags[0]
                .why
                .iter()
                .any(|w| w.contains("callee `unwrap_or`")),
            "why list should record the callee; got: {:?}",
            diags[0].why,
        );
        assert!(
            diags[0]
                .why
                .iter()
                .any(|w| w.contains("default-arg shape: literal default")),
            "why list should record the default-arg shape; got: {:?}",
            diags[0].why,
        );
    }

    #[test]
    fn fl010_fires_on_or_with_call_default() {
        // `option.or(Vec::new())` — callee `or`, shape `Call`. Still silent.
        let air = air_with_module(
            "x::domain::user",
            vec![fallback_call(
                "or",
                ArmBodyShape::Call,
                Some("x::domain::user::collect"),
                7,
            )],
        );
        let diags = fl010(&air, &fl_arm_section(), CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains(".or(...)"));
        assert!(diags[0].message.contains("silent call default"));
    }

    #[test]
    fn fl010_quiet_on_unwrap_or_default_no_arg_form() {
        // `unwrap_or_default()` (Empty) is FL002's territory via the
        // default `forbidden_callees` list. FL010 must skip it even
        // when the callee string slips through here.
        let air = air_with_module(
            "x::domain::user",
            vec![
                fallback_call(
                    "unwrap_or_default",
                    ArmBodyShape::Empty,
                    Some("x::domain::user::greet"),
                    1,
                ),
                // And FL010 also shouldn't fire when default_shape is Empty
                // even on `unwrap_or` (defensive — the visitor wouldn't
                // emit this combination, but the rule must still skip).
                fallback_call(
                    "unwrap_or",
                    ArmBodyShape::Empty,
                    Some("x::domain::user::greet"),
                    2,
                ),
            ],
        );
        assert!(fl010(&air, &fl_arm_section(), CheckMode::Human).is_empty());
    }

    #[test]
    fn fl010_quiet_when_default_shape_is_block_or_other() {
        // Multi-statement fallback block / unrecognised shape — could be
        // doing real recovery work. FL010 stays conservative.
        let air = air_with_module(
            "x::domain::user",
            vec![
                fallback_call(
                    "unwrap_or",
                    ArmBodyShape::Block,
                    Some("x::domain::user::greet"),
                    1,
                ),
                fallback_call(
                    "unwrap_or",
                    ArmBodyShape::Other,
                    Some("x::domain::user::greet"),
                    2,
                ),
                fallback_call(
                    "unwrap_or",
                    ArmBodyShape::Return,
                    Some("x::domain::user::greet"),
                    3,
                ),
                fallback_call(
                    "unwrap_or",
                    ArmBodyShape::ErrorPropagation,
                    Some("x::domain::user::greet"),
                    4,
                ),
            ],
        );
        assert!(fl010(&air, &fl_arm_section(), CheckMode::Human).is_empty());
    }

    #[test]
    fn fl010_silent_when_invariant_owner_paths_empty() {
        let air = air_with_module(
            "x::domain::user",
            vec![fallback_call(
                "unwrap_or",
                ArmBodyShape::Literal,
                Some("x::domain::user::greet"),
                42,
            )],
        );
        assert!(fl010(&air, &FlSection::default(), CheckMode::Human).is_empty());
    }

    #[test]
    fn fl010_agent_strict_elevates_warning_to_fatal() {
        let air = air_with_module(
            "x::domain::user",
            vec![fallback_call(
                "unwrap_or",
                ArmBodyShape::Literal,
                Some("x::domain::user::greet"),
                42,
            )],
        );
        let diags = fl010(&air, &fl_arm_section(), CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    #[test]
    fn fl010_per_symbol_carve_out_for_inline_test_modules() {
        // File `module_path` is `x::domain::user` (doesn't match
        // `*::tests::*`). The function symbol is
        // `x::domain::user::tests::it_works` whose containing module is
        // `x::domain::user::tests` — that's what `*::tests::*` carves out.
        let air = air_with_module(
            "x::domain::user",
            vec![fallback_call(
                "unwrap_or",
                ArmBodyShape::Literal,
                Some("x::domain::user::tests::it_works"),
                7,
            )],
        );
        // Without the carve-out, FL010 fires.
        let diags = fl010(&air, &fl_arm_section(), CheckMode::Human);
        assert_eq!(diags.len(), 1);
        // With `*::tests::*` carved out, it disappears.
        let owner_section = FlSection {
            invariant_owner_paths: vec!["*::tests::*".into()],
            ..Default::default()
        };
        assert!(fl010(&air, &owner_section, CheckMode::Human).is_empty());
    }

    // ---- fl012 behavioural tests ----

    #[test]
    fn fl012_fires_on_loop_with_propagation_and_break() {
        // `loop { try_thing()?; if ok { break; } }` outside any retry-policy owner.
        let air = air_with_module(
            "x::domain::user",
            vec![retry_loop(
                LoopKind::Loop,
                true,
                true,
                Some("x::domain::user::poll"),
                33,
            )],
        );
        let diags = fl012(&air, &fl012_section(), CheckMode::Human);
        assert_eq!(diags.len(), 1, "expected one FL012 diag, got {diags:?}");
        assert_eq!(diags[0].rule_id, "FL012");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(diags[0].concept.is_none());
        assert_eq!(diags[0].span.line_start, 33);
        assert!(
            diags[0].message.contains("retry-shaped loop loop"),
            "message should surface the loop kind; got: {}",
            diags[0].message,
        );
        assert!(diags[0].message.contains("x::domain::user"));
        assert!(diags[0].message.contains("x::domain::user::poll"));
        assert!(
            diags[0]
                .why
                .iter()
                .any(|w| w.contains("uses `?` and contains `break`")),
            "why should explain the retry shape; got: {:?}",
            diags[0].why,
        );
    }

    #[test]
    fn fl012_fires_on_for_loop_with_propagation_and_break() {
        // `for _ in 0..N { try_thing()?; if maybe { break; } }`.
        let air = air_with_module(
            "x::domain::user",
            vec![retry_loop(
                LoopKind::For,
                true,
                true,
                Some("x::domain::user::poll"),
                3,
            )],
        );
        let diags = fl012(&air, &fl012_section(), CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("retry-shaped for loop"));
        assert!(
            diags[0].why.iter().any(|w| w.contains("loop kind: `for`")),
            "why should record the loop kind; got: {:?}",
            diags[0].why,
        );
    }

    #[test]
    fn fl012_quiet_when_loop_propagates_but_has_no_break() {
        // `for x in xs { do_thing(x)?; }` — propagates but never breaks.
        // Just an iterator, not a retry.
        let air = air_with_module(
            "x::domain::user",
            vec![retry_loop(
                LoopKind::For,
                true,
                false,
                Some("x::domain::user::process"),
                3,
            )],
        );
        assert!(fl012(&air, &fl012_section(), CheckMode::Human).is_empty());
    }

    #[test]
    fn fl012_quiet_when_loop_breaks_but_has_no_propagation() {
        // `loop { if cond { break; } }` — breaks but no `?`. No fallible op.
        let air = air_with_module(
            "x::domain::user",
            vec![retry_loop(
                LoopKind::Loop,
                false,
                true,
                Some("x::domain::user::wait"),
                3,
            )],
        );
        assert!(fl012(&air, &fl012_section(), CheckMode::Human).is_empty());
    }

    #[test]
    fn fl012_quiet_in_declared_retry_policy_owner() {
        // `x::retry::backoff` matches the `x::retry::*` owner pattern.
        let air = air_with_module(
            "x::retry::backoff",
            vec![retry_loop(
                LoopKind::Loop,
                true,
                true,
                Some("x::retry::backoff::run"),
                3,
            )],
        );
        assert!(fl012(&air, &fl012_section(), CheckMode::Human).is_empty());
    }

    #[test]
    fn fl012_silent_when_retry_policy_owner_paths_empty() {
        let air = air_with_module(
            "x::domain::user",
            vec![retry_loop(
                LoopKind::Loop,
                true,
                true,
                Some("x::domain::user::poll"),
                3,
            )],
        );
        assert!(fl012(&air, &FlSection::default(), CheckMode::Human).is_empty());
    }

    #[test]
    fn fl012_agent_strict_elevates_warning_to_fatal() {
        let air = air_with_module(
            "x::domain::user",
            vec![retry_loop(
                LoopKind::While,
                true,
                true,
                Some("x::domain::user::poll"),
                33,
            )],
        );
        let diags = fl012(&air, &fl012_section(), CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
        assert!(diags[0].message.contains("retry-shaped while loop"));
    }
}
