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
//!
//! Future FL rules will tackle the harder cases the spec calls out (silent
//! `.ok()` swallows, retry loops without policy).

use locus_air::{AirCallSite, AirItem, AirWorkspace, CallKind};

use super::lockfile_schema::{FlSection, matches_pattern};
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
            if section
                .invariant_owner_paths
                .iter()
                .any(|pat| matches_pattern(pat, module_path))
            {
                continue; // file itself is an accepted invariant owner
            }
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

    use super::super::lockfile_schema::default_forbidden_callees;

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
            domain_paths: Vec::new(),
            boundary_error_patterns: Vec::new(),
            forbidden_callees: default_forbidden_callees(),
            invariant_owner_paths: vec!["x::supervisor::*".into()],
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
        let section = FlSection {
            domain_paths: Vec::new(),
            boundary_error_patterns: Vec::new(),
            forbidden_callees: default_forbidden_callees(),
            invariant_owner_paths: Vec::new(),
        };
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
}
