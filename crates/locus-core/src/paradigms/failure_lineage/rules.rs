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
#[path = "rules_tests.rs"]
mod tests;
