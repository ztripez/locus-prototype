//! FL rule implementations.
//!
//! Implemented:
//! - [`fl001`]: a function in a domain module returns `Result<_, E>` where E
//!   is a declared boundary error type. Boundary errors leaking into domain
//!   function signatures break the failure-lineage invariant — the layer
//!   edge that should have wrapped the transport error didn't.
//!
//! Future FL rules will tackle the harder cases the spec calls out (silent
//! `.ok()` swallows, `unwrap_or_default` on required loads, retry loops
//! without policy). Those need AIR call-site detail we don't have yet, so
//! FL001 starts with the variant that's purely signature-shaped.

use locus_air::{AirItem, AirWorkspace};

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
        }
    }

    fn domain_section() -> FlSection {
        FlSection {
            domain_paths: vec!["x::domain::*".into()],
            boundary_error_patterns: vec!["reqwest::Error".into(), "sqlx::*".into()],
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
        };
        assert!(fl001(&air, &only_domain, CheckMode::Human).is_empty());
        let only_boundary = FlSection {
            domain_paths: Vec::new(),
            boundary_error_patterns: vec!["reqwest::Error".into()],
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
}
