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
//!   The unmatched arm is implicitly silent. Reads `AirItem::PartialIfLet`
//!   items the visitor emits since AIR v9.
//!
//! Future FL rules will tackle the patterns AIR still can't see: `match
//! result { ..., Err(_) => () }` (arm-body inspection), spawned-task
//! failures with no sink (richer fact production).

use locus_air::{AirCallSite, AirItem, AirWorkspace, CallKind};

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
                if !matches!(cs.kind, CallKind::Method | CallKind::Macro) {
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
/// `// ot: allow FL003 reason="..." expires="..."`.
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
             discard, suppress with `// ot: allow FL003 reason=\"…\" \
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
        locus_air::DiscardKind::Macro => "macro",
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
             accepted discard, suppress with `// ot: allow FL004 reason=\"…\" \
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
/// acknowledgement. Reads [`AirItem::PartialIfLet`] items the visitor
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
                let AirItem::PartialIfLet(p) = item else {
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
    p: &locus_air::AirPartialIfLet,
    module_path: &str,
    mode: CheckMode,
) -> Diagnostic {
    let function_label = p
        .function
        .as_deref()
        .unwrap_or("<unknown enclosing function>");
    let unmatched = if p.variant == "Ok" { "Err" } else { "Ok" };
    Diagnostic {
        rule_id: "FL005".to_string(),
        severity: mode.elevate(Severity::Warning),
        span: p.span.clone(),
        concept: None,
        message: format!(
            "partial `if let {}(...) = ...` (no `else` branch) in `{module_path}` \
             (fn `{function_label}`) — the `{unmatched}` arm is implicitly silent",
            p.variant
        ),
        why: vec![
            format!(
                "`if let {}(...) = ...` matches only the `{}` variant; the `{unmatched}` \
                 arm has no body and falls through silently",
                p.variant, p.variant,
            ),
            format!("enclosing function: `{function_label}`"),
            format!(
                "module `{module_path}` does not match any \
                 `paradigms.FL.invariant_owner_paths` pattern"
            ),
        ],
        suggested_fix: Some(format!(
            "rewrite as a `match` with both arms, or add an `else` branch \
             that handles the `{unmatched}` case (log, propagate, or \
             explicitly accept). If `{module_path}` is a legitimate \
             invariant owner (supervisor, test-support module), add it to \
             `paradigms.FL.invariant_owner_paths`. For a one-off accepted \
             partial match, suppress with `// ot: allow FL005 reason=\"…\" \
             expires=\"YYYY-MM-DD\"`"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use locus_air::{
        AIR_SCHEMA_VERSION, AirFile, AirFunction, AirPackage, AirSpan, AirWorkspace, Visibility,
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
                CallKind::Macro,
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
                CallKind::Macro,
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
        AirItem::PartialIfLet(locus_air::AirPartialIfLet {
            variant: variant.to_string(),
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
}
