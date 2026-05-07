//! Diagnostic suppression via `// ot: allow` source hints and lockfile
//! exceptions. Exceptions are local, explicit, and reviewable
//! (`docs/project-jumpoff.md` §"Exceptions") — every suppression carries a
//! reason and an expiry date.
//!
//! Two suppression sources, both filtered through [`apply_exceptions`]:
//!
//! 1. **Source hints** — `HintKind::Allow { rule, reason, expires }` parsed
//!    from `// ot: allow XX### reason="..." expires="YYYY-MM-DD"` comments
//!    by the language adapter. The hint binds to the next non-blank line;
//!    a diagnostic in that line range with a matching rule prefix is
//!    suppressed.
//! 2. **Lockfile exceptions** — entries in `Lockfile::exceptions` (top-level,
//!    not paradigm-namespaced — exceptions cross paradigm boundaries by
//!    design). Target syntax: `path/to/file.rs[:line]`. Wildcard `*` matches
//!    every file.
//!
//! Expired exceptions don't suppress; instead they emit a `LOCUS001`
//! "expired exception" diagnostic so the user notices the lapse without
//! their existing diagnostic suite collapsing.

// ot: canonical

use crate::diagnostics::{Diagnostic, Severity};
use crate::lockfile::Lockfile;
use locus_air::{AirSpan, AirWorkspace, HintKind};

/// Sentinel rule ID emitted when an exception's expiry date has passed.
pub const EXPIRED_EXCEPTION_RULE: &str = "LOCUS001";

/// Filter `diagnostics` through allow-hints + lockfile exceptions.
///
/// `today` is the current date as `YYYY-MM-DD`; if `None`, expiry checks
/// are skipped (every exception is considered live). Production callers
/// pass `Some(today_utc())`; tests pass a fixed string for determinism.
pub fn apply_exceptions(
    diagnostics: Vec<Diagnostic>,
    air: &AirWorkspace,
    lockfile: &Lockfile,
    today: Option<&str>,
) -> Vec<Diagnostic> {
    let mut out = Vec::with_capacity(diagnostics.len());
    let mut expired: Vec<Diagnostic> = Vec::new();

    'each_diag: for d in diagnostics {
        // 1. Source-hint allow.
        for pkg in &air.packages {
            for file in &pkg.files {
                if !same_file(&file.path, &d.span.file) {
                    continue;
                }
                for hint in &file.hints {
                    let HintKind::Allow {
                        rule,
                        reason,
                        expires,
                    } = &hint.kind
                    else {
                        continue;
                    };
                    if !rule_matches(rule, &d.rule_id) {
                        continue;
                    }
                    let Some(target) = &hint.target_span else {
                        continue;
                    };
                    if !span_overlaps(target, &d.span) {
                        continue;
                    }
                    if is_expired(expires.as_deref(), today) {
                        expired.push(expired_diagnostic(
                            &d.rule_id,
                            &d.span,
                            expires.as_deref().unwrap_or(""),
                            reason.as_deref(),
                            "source hint",
                        ));
                        continue;
                    }
                    continue 'each_diag;
                }
            }
        }
        // 2. Lockfile exception.
        for ex in &lockfile.exceptions {
            if !rule_matches(&ex.rule, &d.rule_id) {
                continue;
            }
            if !lockfile_target_matches(&ex.target, &d.span) {
                continue;
            }
            if is_expired(Some(ex.expires.as_str()), today) {
                expired.push(expired_diagnostic(
                    &d.rule_id,
                    &d.span,
                    &ex.expires,
                    Some(&ex.reason),
                    "lockfile exception",
                ));
                continue;
            }
            continue 'each_diag;
        }
        out.push(d);
    }

    // Deduplicate expired warnings by (rule_id, file, line, expires) so a
    // misconfigured workspace doesn't spam.
    expired.sort_by(|a, b| {
        (
            &a.rule_id,
            &a.span.file,
            a.span.line_start,
            a.message.clone(),
        )
            .cmp(&(
                &b.rule_id,
                &b.span.file,
                b.span.line_start,
                b.message.clone(),
            ))
    });
    expired.dedup_by(|a, b| {
        a.rule_id == b.rule_id
            && a.span.file == b.span.file
            && a.span.line_start == b.span.line_start
            && a.message == b.message
    });
    out.extend(expired);
    out
}

/// Today's date as `YYYY-MM-DD` (UTC). Honours the `LOCUS_TODAY`
/// environment variable for deterministic testing / CI replay.
pub fn today_utc() -> String {
    if let Ok(s) = std::env::var("LOCUS_TODAY")
        && !s.is_empty()
    {
        return s;
    }
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let (y, m, d) = days_to_ymd(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Howard Hinnant's days-from-civil algorithm
/// (<https://howardhinnant.github.io/date_algorithms.html#civil_from_days>).
/// Maps days since the Unix epoch to a (year, month, day) tuple. Saves us a
/// `chrono` dependency for the only date arithmetic Locus does.
fn days_to_ymd(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

fn rule_matches(allow: &str, diag_rule: &str) -> bool {
    if allow == "*" {
        return true;
    }
    if allow.eq_ignore_ascii_case(diag_rule) {
        return true;
    }
    // A 2-letter allow (e.g. `OT`) suppresses every rule under that prefix.
    if allow.len() == 2 && diag_rule.starts_with(allow) {
        return true;
    }
    false
}

fn same_file(a: &str, b: &str) -> bool {
    a == b || a.ends_with(b) || b.ends_with(a)
}

fn span_overlaps(target: &AirSpan, diag: &AirSpan) -> bool {
    if !same_file(&target.file, &diag.file) {
        return false;
    }
    target.line_start <= diag.line_end && diag.line_start <= target.line_end
}

fn lockfile_target_matches(target: &str, span: &AirSpan) -> bool {
    if target == "*" {
        return true;
    }
    if let Some((file, line)) = target.rsplit_once(':')
        && let Ok(line_num) = line.parse::<u32>()
    {
        return same_file(file, &span.file)
            && span.line_start <= line_num
            && line_num <= span.line_end;
    }
    same_file(target, &span.file)
}

fn is_expired(expires: Option<&str>, today: Option<&str>) -> bool {
    match (expires, today) {
        (Some(exp), Some(now)) => exp < now,
        _ => false,
    }
}

fn expired_diagnostic(
    rule_id: &str,
    span: &AirSpan,
    expires: &str,
    reason: Option<&str>,
    source: &str,
) -> Diagnostic {
    let why = {
        let mut w = vec![format!(
            "{source} for `{rule_id}` expired on `{expires}` — \
             diagnostic is no longer suppressed"
        )];
        if let Some(r) = reason {
            w.push(format!("original reason: {r}"));
        }
        w.push("renew the exception or fix the underlying issue".to_string());
        w
    };
    Diagnostic {
        rule_id: EXPIRED_EXCEPTION_RULE.to_string(),
        severity: Severity::Warning,
        span: span.clone(),
        concept: None,
        message: format!("expired {source} for `{rule_id}` (expired {expires})"),
        why,
        suggested_fix: Some(format!(
            "either remove the expired {source} for `{rule_id}` and fix \
             the underlying issue, or renew it with a new `expires` date"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::Severity;
    use crate::lockfile::Exception;
    use locus_air::{
        AIR_SCHEMA_VERSION, AirFile, AirHint, AirPackage, AirSpan, AirWorkspace, HintKind,
    };

    fn diag(rule: &str, file: &str, line: u32) -> Diagnostic {
        Diagnostic {
            rule_id: rule.into(),
            severity: Severity::Fatal,
            span: AirSpan::new(file, line, line),
            concept: None,
            message: format!("{rule} fired"),
            why: Vec::new(),
            suggested_fix: None,
        }
    }

    fn allow_hint(rule: &str, expires: Option<&str>, target_line: u32) -> AirHint {
        AirHint {
            kind: HintKind::Allow {
                rule: rule.into(),
                reason: Some("test".into()),
                expires: expires.map(|s| s.into()),
            },
            raw: format!("// ot: allow {rule}"),
            span: AirSpan::new("t.rs", target_line - 1, target_line - 1),
            target_span: Some(AirSpan::new("t.rs", target_line, target_line)),
        }
    }

    fn workspace_with_hints(file_path: &str, hints: Vec<AirHint>) -> AirWorkspace {
        AirWorkspace {
            schema_version: AIR_SCHEMA_VERSION,
            packages: vec![AirPackage {
                name: "x".into(),
                version: "0".into(),
                root_dir: "/".into(),
                files: vec![AirFile {
                    path: file_path.into(),
                    module_path: None,
                    items: Vec::new(),
                    hints,
                    parse_error: None,
                    line_count: 1,
                }],
            }],
            facts: Vec::new(),
        }
    }

    #[test]
    fn allow_hint_suppresses_matching_diagnostic() {
        let diags = vec![diag("OT002", "t.rs", 10)];
        let air = workspace_with_hints("t.rs", vec![allow_hint("OT002", None, 10)]);
        let kept = apply_exceptions(diags, &air, &Lockfile::empty(), None);
        assert!(kept.is_empty());
    }

    #[test]
    fn allow_hint_does_not_suppress_other_rules() {
        let diags = vec![diag("OT003", "t.rs", 10)];
        let air = workspace_with_hints("t.rs", vec![allow_hint("OT002", None, 10)]);
        let kept = apply_exceptions(diags, &air, &Lockfile::empty(), None);
        assert_eq!(kept.len(), 1);
    }

    #[test]
    fn allow_hint_only_suppresses_overlapping_lines() {
        let diags = vec![diag("OT002", "t.rs", 99)];
        let air = workspace_with_hints("t.rs", vec![allow_hint("OT002", None, 10)]);
        let kept = apply_exceptions(diags, &air, &Lockfile::empty(), None);
        assert_eq!(kept.len(), 1);
    }

    #[test]
    fn expired_allow_hint_emits_locus001_warning_and_keeps_original() {
        let diags = vec![diag("OT002", "t.rs", 10)];
        let air = workspace_with_hints("t.rs", vec![allow_hint("OT002", Some("2024-01-01"), 10)]);
        let kept = apply_exceptions(diags, &air, &Lockfile::empty(), Some("2026-05-07"));
        assert_eq!(kept.len(), 2, "expected original + LOCUS001; got {kept:?}");
        let kinds: Vec<&str> = kept.iter().map(|d| d.rule_id.as_str()).collect();
        assert!(kinds.contains(&"OT002"));
        assert!(kinds.contains(&EXPIRED_EXCEPTION_RULE));
    }

    #[test]
    fn unexpired_allow_hint_still_suppresses_with_today_set() {
        let diags = vec![diag("OT002", "t.rs", 10)];
        let air = workspace_with_hints("t.rs", vec![allow_hint("OT002", Some("2030-01-01"), 10)]);
        let kept = apply_exceptions(diags, &air, &Lockfile::empty(), Some("2026-05-07"));
        assert!(kept.is_empty());
    }

    #[test]
    fn lockfile_exception_suppresses_when_target_matches() {
        let diags = vec![diag("DG001", "src/handler.rs", 17)];
        let air = workspace_with_hints("src/handler.rs", Vec::new());
        let mut lf = Lockfile::empty();
        lf.exceptions.push(Exception {
            rule: "DG001".into(),
            target: "src/handler.rs:17".into(),
            reason: "transitional".into(),
            expires: "2030-01-01".into(),
        });
        let kept = apply_exceptions(diags, &air, &lf, Some("2026-05-07"));
        assert!(kept.is_empty());
    }

    #[test]
    fn lockfile_exception_with_only_file_matches_any_line() {
        let diags = vec![
            diag("DG001", "src/handler.rs", 4),
            diag("DG001", "src/handler.rs", 99),
        ];
        let air = workspace_with_hints("src/handler.rs", Vec::new());
        let mut lf = Lockfile::empty();
        lf.exceptions.push(Exception {
            rule: "DG001".into(),
            target: "src/handler.rs".into(),
            reason: "scoped to file".into(),
            expires: "2030-01-01".into(),
        });
        let kept = apply_exceptions(diags, &air, &lf, Some("2026-05-07"));
        assert!(kept.is_empty());
    }

    #[test]
    fn rule_prefix_allow_suppresses_every_rule_in_paradigm() {
        let diags = vec![
            diag("OT001", "t.rs", 10),
            diag("OT002", "t.rs", 10),
            diag("DG001", "t.rs", 10),
        ];
        let air = workspace_with_hints("t.rs", vec![allow_hint("OT", None, 10)]);
        let kept = apply_exceptions(diags, &air, &Lockfile::empty(), None);
        // OT* suppressed; DG001 untouched.
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].rule_id, "DG001");
    }

    #[test]
    fn star_allow_suppresses_everything() {
        let diags = vec![diag("OT001", "t.rs", 10), diag("DG001", "t.rs", 10)];
        let air = workspace_with_hints("t.rs", vec![allow_hint("*", None, 10)]);
        let kept = apply_exceptions(diags, &air, &Lockfile::empty(), None);
        assert!(kept.is_empty());
    }

    #[test]
    fn today_utc_honours_locus_today_env_var() {
        // SAFETY: cargo test runs each test in the same process, so we
        // unset after asserting to avoid contaminating peers. The simpler
        // alternative — running tests in serial — is overkill for one var.
        unsafe {
            std::env::set_var("LOCUS_TODAY", "2099-12-31");
        }
        assert_eq!(today_utc(), "2099-12-31");
        unsafe {
            std::env::remove_var("LOCUS_TODAY");
        }
    }

    #[test]
    fn days_to_ymd_round_trip_known_dates() {
        // 2026-01-01 = day 20454 since 1970-01-01 (sanity-check the algo).
        assert_eq!(days_to_ymd(20_454), (2026, 1, 1));
        assert_eq!(days_to_ymd(0), (1970, 1, 1));
    }
}
