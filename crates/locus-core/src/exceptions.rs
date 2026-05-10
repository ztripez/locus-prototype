//! Diagnostic suppression via `// locus: allow` source hints and lockfile
//! exceptions. Exceptions are local, explicit, and reviewable
//! (`docs/project-jumpoff.md` §"Exceptions") — every suppression carries a
//! reason and an expiry date.
//!
//! Two suppression sources, both filtered through [`apply_exceptions`]:
//!
//! 1. **Source hints** — `HintKind::Allow { rule, reason, expires }` parsed
//!    from `// locus: allow XX### reason="..." expires="YYYY-MM-DD"` comments
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

// locus: ot canonical

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

/// One row in `locus debt`'s output. Holds enough to identify the
/// suppression site without losing the source distinction (a hint at
/// `src/foo.rs:42` and a lockfile entry targeting that same line are
/// independent debt items).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExceptionEntry {
    pub source: ExceptionSource,
    pub rule: String,
    pub target: String,
    pub reason: Option<String>,
    pub expires: Option<String>,
    pub status: ExceptionStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExceptionSource {
    Hint,
    Lockfile,
    /// An entry in `paradigms.CX.exempt_paths`. Legacy string entries appear
    /// here with `status = LegacyNoMetadata`; struct entries with all debt
    /// fields filled appear as `Active`; struct entries with missing metadata
    /// appear as `LegacyNoMetadata` (they need upgrading).
    CxExemptPath,
    /// An entry in `Lockfile.acknowledged_empty`. Legacy string entries appear
    /// here with `status = LegacyNoMetadata`; struct entries with all debt
    /// fields filled appear as `Active` or `Expired`; struct entries with
    /// missing metadata appear as `LegacyNoMetadata`.
    AcknowledgedEmpty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExceptionStatus {
    Active,
    Expired,
    /// `// locus: allow` source hint without an `expires=` clause. The hint
    /// suppresses forever; surfaced separately so the user notices.
    Unbounded,
    /// A `CX.exempt_paths` entry that pre-dates the debt-metadata schema
    /// (legacy `String` form) OR a struct-form entry that is missing one or
    /// more required metadata fields (`reason`, `expires`, `owner`). Surfaces
    /// in `locus debt` so the user can migrate or annotate the entry.
    LegacyNoMetadata,
}

/// Walk every `// locus: allow` hint in `air` and every `Lockfile.exceptions`
/// entry, returning a row per suppression with its current status. Used
/// by `locus debt`. `today` follows the same convention as
/// [`apply_exceptions`]: `Some("YYYY-MM-DD")` for deterministic runs,
/// `None` to skip expiry checks.
///
/// Also enumerates `paradigms.CX.exempt_paths` entries and
/// `Lockfile.acknowledged_empty` entries. Legacy `String` entries surface as
/// [`ExceptionStatus::LegacyNoMetadata`]; struct entries with complete
/// metadata surface as [`ExceptionStatus::Active`] or
/// [`ExceptionStatus::Expired`]; struct entries with missing metadata also
/// surface as [`ExceptionStatus::LegacyNoMetadata`].
pub fn collect_exceptions(
    air: &AirWorkspace,
    lockfile: &Lockfile,
    today: Option<&str>,
) -> Vec<ExceptionEntry> {
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            for hint in &file.hints {
                let HintKind::Allow {
                    rule,
                    reason,
                    expires,
                } = &hint.kind
                else {
                    continue;
                };
                let line = hint
                    .target_span
                    .as_ref()
                    .map(|t| t.line_start)
                    .unwrap_or(hint.span.line_start);
                let status = match expires.as_deref() {
                    None => ExceptionStatus::Unbounded,
                    Some(exp) => {
                        if is_expired(Some(exp), today) {
                            ExceptionStatus::Expired
                        } else {
                            ExceptionStatus::Active
                        }
                    }
                };
                out.push(ExceptionEntry {
                    source: ExceptionSource::Hint,
                    rule: rule.clone(),
                    target: format!("{}:{}", file.path, line),
                    reason: reason.clone(),
                    expires: expires.clone(),
                    status,
                });
            }
        }
    }
    for ex in &lockfile.exceptions {
        let status = if is_expired(Some(&ex.expires), today) {
            ExceptionStatus::Expired
        } else {
            ExceptionStatus::Active
        };
        out.push(ExceptionEntry {
            source: ExceptionSource::Lockfile,
            rule: ex.rule.clone(),
            target: ex.target.clone(),
            reason: Some(ex.reason.clone()),
            expires: Some(ex.expires.clone()),
            status,
        });
    }
    // Enumerate CX.exempt_paths entries.
    collect_cx_exempt_paths(lockfile, today, &mut out);

    // Enumerate acknowledged_empty entries.
    collect_acknowledged_empty_entries(lockfile, today, &mut out);

    out.sort_by(|a, b| {
        (status_order(a.status), &a.rule, &a.target).cmp(&(
            status_order(b.status),
            &b.rule,
            &b.target,
        ))
    });
    out
}

/// Append one [`ExceptionEntry`] per `CX.exempt_paths` entry to `out`.
///
/// Classification:
/// - Legacy `String` entry → `LegacyNoMetadata` (pre-dates the schema).
/// - Struct entry with any of `reason`/`expires`/`owner` missing → `LegacyNoMetadata`.
/// - Struct entry with all three fields populated:
///   - Expired (`expires` < `today`) → `Expired`.
///   - Otherwise → `Active`.
fn collect_cx_exempt_paths(
    lockfile: &Lockfile,
    today: Option<&str>,
    out: &mut Vec<ExceptionEntry>,
) {
    use crate::paradigms::complexity_budget::lockfile_schema::{CxExemptPathEntry, CxSection};

    // Only enumerate if CX is explicitly present in the lockfile. Calling
    // `paradigm_section` on a missing key returns `Ok(CxSection::default())`
    // which has 2 default exempt_paths — we'd surface phantom debt entries
    // for every un-onboarded workspace. Return early when no CX section exists.
    if !lockfile.paradigms.contains_key("CX") {
        return;
    }
    let cx: CxSection = match lockfile.paradigm_section("CX") {
        Ok(s) => s,
        Err(_) => return,
    };

    for entry in &cx.exempt_paths {
        let (pattern, reason, expires, owner) = match entry {
            CxExemptPathEntry::Legacy(s) => (s.as_str(), None, None, None),
            CxExemptPathEntry::Full(ep) => (
                ep.pattern.as_str(),
                ep.reason.as_deref(),
                ep.expires.as_deref(),
                ep.owner.as_deref(),
            ),
        };

        let has_metadata = reason.is_some_and(|r| !r.is_empty())
            && expires.is_some_and(|e| !e.is_empty())
            && owner.is_some_and(|o| !o.is_empty());

        let status = if !has_metadata {
            ExceptionStatus::LegacyNoMetadata
        } else if is_expired(expires, today) {
            ExceptionStatus::Expired
        } else {
            ExceptionStatus::Active
        };

        out.push(ExceptionEntry {
            source: ExceptionSource::CxExemptPath,
            rule: "CX007".to_string(),
            target: format!("paradigms.CX.exempt_paths:{pattern}"),
            reason: reason.map(str::to_string),
            expires: expires.map(str::to_string),
            status,
        });
    }
}

/// Append one [`ExceptionEntry`] per `Lockfile.acknowledged_empty` entry to
/// `out`.
///
/// Classification:
/// - Legacy `String` entry → `LegacyNoMetadata` (pre-dates the schema).
/// - Struct entry with any of `reason`/`expires`/`owner` missing → `LegacyNoMetadata`.
/// - Struct entry with all three fields populated:
///   - Expired (`expires` < `today`) → `Expired`.
///   - Otherwise → `Active`.
fn collect_acknowledged_empty_entries(
    lockfile: &Lockfile,
    today: Option<&str>,
    out: &mut Vec<ExceptionEntry>,
) {
    use crate::lockfile::AcknowledgedEmptyEntry;

    for entry in &lockfile.acknowledged_empty {
        let (prefix, reason, expires, owner) = match entry {
            AcknowledgedEmptyEntry::Legacy(s) => (s.as_str(), None, None, None),
            AcknowledgedEmptyEntry::Full(meta) => (
                meta.prefix.as_str(),
                meta.reason.as_deref(),
                meta.expires.as_deref(),
                meta.owner.as_deref(),
            ),
        };

        let has_metadata = reason.is_some_and(|r| !r.is_empty())
            && expires.is_some_and(|e| !e.is_empty())
            && owner.is_some_and(|o| !o.is_empty());

        let status = if !has_metadata {
            ExceptionStatus::LegacyNoMetadata
        } else if is_expired(expires, today) {
            ExceptionStatus::Expired
        } else {
            ExceptionStatus::Active
        };

        out.push(ExceptionEntry {
            source: ExceptionSource::AcknowledgedEmpty,
            rule: "LOCUS002".to_string(),
            target: format!("acknowledged_empty:{prefix}"),
            reason: reason.map(str::to_string),
            expires: expires.map(str::to_string),
            status,
        });
    }
}

fn status_order(s: ExceptionStatus) -> u8 {
    match s {
        ExceptionStatus::Expired => 0,
        ExceptionStatus::LegacyNoMetadata => 1,
        ExceptionStatus::Unbounded => 2,
        ExceptionStatus::Active => 3,
    }
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
            raw: format!("// locus: allow {rule}"),
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

    #[test]
    fn collect_exceptions_classifies_each_source_and_status() {
        let air = workspace_with_hints(
            "t.rs",
            vec![
                allow_hint("OT002", Some("2030-01-01"), 10),
                allow_hint("DG001", Some("2024-01-01"), 20),
                allow_hint("CX001", None, 30),
            ],
        );
        let mut lf = Lockfile::empty();
        lf.exceptions.push(Exception {
            rule: "DG001".into(),
            target: "src/legacy.rs:42".into(),
            reason: "interim".into(),
            expires: "2030-12-01".into(),
        });
        lf.exceptions.push(Exception {
            rule: "OT004".into(),
            target: "src/old.rs".into(),
            reason: "to migrate".into(),
            expires: "2024-06-01".into(),
        });

        let entries = collect_exceptions(&air, &lf, Some("2026-05-07"));
        assert_eq!(entries.len(), 5);

        let mut counts = (0, 0, 0);
        for e in &entries {
            match e.status {
                ExceptionStatus::Active => counts.0 += 1,
                ExceptionStatus::Expired => counts.1 += 1,
                ExceptionStatus::Unbounded => counts.2 += 1,
                ExceptionStatus::LegacyNoMetadata => {} // none expected here
            }
        }
        assert_eq!(counts, (2, 2, 1));

        // Expired rows sort first.
        assert_eq!(entries[0].status, ExceptionStatus::Expired);
        assert_eq!(entries[1].status, ExceptionStatus::Expired);
    }

    #[test]
    fn collect_exceptions_target_includes_hint_line() {
        let air = workspace_with_hints("t.rs", vec![allow_hint("OT002", Some("2030-01-01"), 42)]);
        let entries = collect_exceptions(&air, &Lockfile::empty(), Some("2026-05-07"));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].target, "t.rs:42");
        assert_eq!(entries[0].source, ExceptionSource::Hint);
    }

    // ---- CX exempt_paths debt enumeration ---------------------------

    fn lockfile_with_cx_exempt_paths(exempt_paths: serde_json::Value) -> Lockfile {
        let mut lf = Lockfile::empty();
        lf.paradigms.insert(
            "CX".to_string(),
            serde_json::json!({"exempt_paths": exempt_paths}),
        );
        lf
    }

    #[test]
    fn collect_exceptions_surfaces_legacy_string_exempt_paths_as_legacy_no_metadata() {
        let lf = lockfile_with_cx_exempt_paths(serde_json::json!(["*::tests::*", "locus_air::*"]));
        let air = workspace_with_hints("t.rs", vec![]);
        let entries = collect_exceptions(&air, &lf, Some("2026-05-07"));
        // Two legacy entries; both should be LegacyNoMetadata.
        let cx_entries: Vec<_> = entries
            .iter()
            .filter(|e| e.source == ExceptionSource::CxExemptPath)
            .collect();
        assert_eq!(
            cx_entries.len(),
            2,
            "two exempt_paths → two entries; got {cx_entries:#?}"
        );
        for e in &cx_entries {
            assert_eq!(
                e.status,
                ExceptionStatus::LegacyNoMetadata,
                "legacy string → LegacyNoMetadata; got {:?}",
                e.status
            );
            assert_eq!(e.rule, "CX007");
        }
        assert!(
            cx_entries.iter().any(|e| e.target.contains("*::tests::*")),
            "target should contain the pattern; got {:?}",
            cx_entries.iter().map(|e| &e.target).collect::<Vec<_>>()
        );
    }

    #[test]
    fn collect_exceptions_struct_entry_with_full_metadata_is_active() {
        let lf = lockfile_with_cx_exempt_paths(serde_json::json!([
            {"pattern": "locus_air::*", "reason": "canonical", "expires": "2030-01-01", "owner": "@core"}
        ]));
        let air = workspace_with_hints("t.rs", vec![]);
        let entries = collect_exceptions(&air, &lf, Some("2026-05-07"));
        let cx_entries: Vec<_> = entries
            .iter()
            .filter(|e| e.source == ExceptionSource::CxExemptPath)
            .collect();
        assert_eq!(cx_entries.len(), 1);
        assert_eq!(cx_entries[0].status, ExceptionStatus::Active);
        assert_eq!(cx_entries[0].reason.as_deref(), Some("canonical"));
    }

    #[test]
    fn collect_exceptions_struct_entry_with_expired_date_is_expired() {
        let lf = lockfile_with_cx_exempt_paths(serde_json::json!([
            {"pattern": "foo::*", "reason": "old", "expires": "2020-01-01", "owner": "@core"}
        ]));
        let air = workspace_with_hints("t.rs", vec![]);
        let entries = collect_exceptions(&air, &lf, Some("2026-05-07"));
        let cx_entries: Vec<_> = entries
            .iter()
            .filter(|e| e.source == ExceptionSource::CxExemptPath)
            .collect();
        assert_eq!(cx_entries.len(), 1);
        assert_eq!(cx_entries[0].status, ExceptionStatus::Expired);
    }

    #[test]
    fn collect_exceptions_struct_entry_missing_metadata_is_legacy_no_metadata() {
        let lf = lockfile_with_cx_exempt_paths(serde_json::json!([
            {"pattern": "bar::*"}  // struct form but no reason/expires/owner
        ]));
        let air = workspace_with_hints("t.rs", vec![]);
        let entries = collect_exceptions(&air, &lf, Some("2026-05-07"));
        let cx_entries: Vec<_> = entries
            .iter()
            .filter(|e| e.source == ExceptionSource::CxExemptPath)
            .collect();
        assert_eq!(cx_entries.len(), 1);
        assert_eq!(
            cx_entries[0].status,
            ExceptionStatus::LegacyNoMetadata,
            "struct form with missing fields → LegacyNoMetadata"
        );
    }

    #[test]
    fn collect_exceptions_no_cx_section_does_not_produce_phantom_entries() {
        // An empty lockfile (no CX key) must not produce any CxExemptPath
        // entries — the default CxSection's exempt_paths should not surface.
        let air = workspace_with_hints("t.rs", vec![]);
        let entries = collect_exceptions(&air, &Lockfile::empty(), Some("2026-05-07"));
        assert!(
            entries
                .iter()
                .all(|e| e.source != ExceptionSource::CxExemptPath),
            "no CX section → no CxExemptPath entries; got {entries:#?}"
        );
    }

    #[test]
    fn collect_exceptions_legacy_no_metadata_sorts_before_unbounded() {
        // Ordering: Expired → LegacyNoMetadata → Unbounded → Active.
        let lf = lockfile_with_cx_exempt_paths(serde_json::json!(["*::tests::*"]));
        let air = workspace_with_hints(
            "t.rs",
            vec![allow_hint("OT002", None, 10)], // Unbounded
        );
        let entries = collect_exceptions(&air, &lf, Some("2026-05-07"));
        // Should be 2 entries: one Unbounded hint, one LegacyNoMetadata cx exempt.
        let statuses: Vec<_> = entries.iter().map(|e| e.status).collect();
        assert_eq!(statuses.len(), 2);
        // LegacyNoMetadata sorts before Unbounded.
        assert_eq!(statuses[0], ExceptionStatus::LegacyNoMetadata);
        assert_eq!(statuses[1], ExceptionStatus::Unbounded);
    }

    // ---- acknowledged_empty debt enumeration -------------------------

    fn lockfile_with_ack_empty(entries: Vec<crate::lockfile::AcknowledgedEmptyEntry>) -> Lockfile {
        let mut lf = Lockfile::empty();
        lf.acknowledged_empty = entries;
        lf
    }

    #[test]
    fn collect_exceptions_surfaces_legacy_ack_empty_as_legacy_no_metadata() {
        use crate::lockfile::AcknowledgedEmptyEntry;
        let lf = lockfile_with_ack_empty(vec![
            AcknowledgedEmptyEntry::Legacy("BO".to_string()),
            AcknowledgedEmptyEntry::Legacy("PA".to_string()),
        ]);
        let air = workspace_with_hints("t.rs", vec![]);
        let entries = collect_exceptions(&air, &lf, Some("2026-05-07"));
        let ack: Vec<_> = entries
            .iter()
            .filter(|e| e.source == ExceptionSource::AcknowledgedEmpty)
            .collect();
        assert_eq!(ack.len(), 2, "two legacy entries → two rows; got {ack:#?}");
        for e in &ack {
            assert_eq!(e.status, ExceptionStatus::LegacyNoMetadata);
            assert_eq!(e.rule, "LOCUS002");
        }
        assert!(
            ack.iter().any(|e| e.target.contains("BO")),
            "target should name the prefix; got {:?}",
            ack.iter().map(|e| &e.target).collect::<Vec<_>>()
        );
    }

    #[test]
    fn collect_exceptions_ack_empty_struct_with_full_metadata_is_active() {
        use crate::lockfile::{AcknowledgedEmpty, AcknowledgedEmptyEntry};
        let lf = lockfile_with_ack_empty(vec![AcknowledgedEmptyEntry::Full(AcknowledgedEmpty {
            prefix: "RW".to_string(),
            expires: Some("2030-01-01".to_string()),
            reason: Some("no runtime owners yet".to_string()),
            owner: Some("@core".to_string()),
            debt_id: None,
            introduced_by: None,
        })]);
        let air = workspace_with_hints("t.rs", vec![]);
        let entries = collect_exceptions(&air, &lf, Some("2026-05-07"));
        let ack: Vec<_> = entries
            .iter()
            .filter(|e| e.source == ExceptionSource::AcknowledgedEmpty)
            .collect();
        assert_eq!(ack.len(), 1);
        assert_eq!(ack[0].status, ExceptionStatus::Active);
        assert_eq!(ack[0].reason.as_deref(), Some("no runtime owners yet"));
    }

    #[test]
    fn collect_exceptions_ack_empty_struct_with_expired_date_is_expired() {
        use crate::lockfile::{AcknowledgedEmpty, AcknowledgedEmptyEntry};
        let lf = lockfile_with_ack_empty(vec![AcknowledgedEmptyEntry::Full(AcknowledgedEmpty {
            prefix: "DA".to_string(),
            expires: Some("2020-01-01".to_string()),
            reason: Some("old".to_string()),
            owner: Some("@core".to_string()),
            debt_id: None,
            introduced_by: None,
        })]);
        let air = workspace_with_hints("t.rs", vec![]);
        let entries = collect_exceptions(&air, &lf, Some("2026-05-07"));
        let ack: Vec<_> = entries
            .iter()
            .filter(|e| e.source == ExceptionSource::AcknowledgedEmpty)
            .collect();
        assert_eq!(ack.len(), 1);
        assert_eq!(ack[0].status, ExceptionStatus::Expired);
    }

    #[test]
    fn collect_exceptions_ack_empty_struct_missing_metadata_is_legacy_no_metadata() {
        use crate::lockfile::{AcknowledgedEmpty, AcknowledgedEmptyEntry};
        // struct form but no reason/expires/owner
        let lf = lockfile_with_ack_empty(vec![AcknowledgedEmptyEntry::Full(AcknowledgedEmpty {
            prefix: "CF".to_string(),
            ..Default::default()
        })]);
        let air = workspace_with_hints("t.rs", vec![]);
        let entries = collect_exceptions(&air, &lf, Some("2026-05-07"));
        let ack: Vec<_> = entries
            .iter()
            .filter(|e| e.source == ExceptionSource::AcknowledgedEmpty)
            .collect();
        assert_eq!(ack.len(), 1);
        assert_eq!(
            ack[0].status,
            ExceptionStatus::LegacyNoMetadata,
            "struct form with missing fields → LegacyNoMetadata"
        );
    }
}
