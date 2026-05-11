use std::io::{self, BufWriter, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use locus_core::Lockfile;

// locus: ot boundary cli.debt cli
#[derive(clap::Args, Debug)]
pub struct DebtArgs {
    /// Workspace root (containing Cargo.toml).
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
    /// Emit one JSON object per line instead of human-readable text.
    #[arg(long)]
    pub json: bool,
    /// Group output by rule id so hotspot rules are obvious.
    #[arg(long)]
    pub by_rule: bool,
}

pub fn run(args: DebtArgs) -> Result<()> {
    use locus_core::exceptions::{collect_exceptions, today_utc};

    let air = locus_rust::scan(&args.workspace)
        .with_context(|| format!("scan failed: {}", args.workspace.display()))?;
    let lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let today = today_utc();
    let entries = collect_exceptions(&air, &lockfile, Some(&today));

    if args.json {
        return print_json(&entries, args.by_rule);
    }

    if args.by_rule {
        print_by_rule_text(&entries);
    } else {
        print_text(&entries);
    }
    Ok(())
}

fn print_json(entries: &[locus_core::exceptions::ExceptionEntry], by_rule: bool) -> Result<()> {
    use locus_core::exceptions::{ExceptionSource, ExceptionStatus};

    if by_rule {
        let grouped = group_by_rule(entries);
        let stdout = io::stdout();
        let mut w = BufWriter::new(stdout.lock());
        for row in grouped {
            serde_json::to_writer(&mut w, &row)?;
            w.write_all(b"\n")?;
        }
        return Ok(());
    }
    let stdout = io::stdout();
    let mut w = BufWriter::new(stdout.lock());
    for e in entries {
        let row = serde_json::json!({
            "source": match e.source {
                ExceptionSource::Hint => "hint",
                ExceptionSource::Lockfile => "lockfile",
                ExceptionSource::CxExemptPath => "cx_exempt_path",
                ExceptionSource::AcknowledgedEmpty => "acknowledged_empty",
            },
            "rule": e.rule,
            "target": e.target,
            "reason": e.reason,
            "expires": e.expires,
            "status": match e.status {
                ExceptionStatus::Active => "active",
                ExceptionStatus::Expired => "expired",
                ExceptionStatus::Unbounded => "unbounded",
                ExceptionStatus::LegacyNoMetadata => "legacy_no_metadata",
            },
        });
        serde_json::to_writer(&mut w, &row)?;
        w.write_all(b"\n")?;
    }
    Ok(())
}

pub fn group_by_rule(entries: &[locus_core::exceptions::ExceptionEntry]) -> Vec<serde_json::Value> {
    use locus_core::exceptions::ExceptionStatus;
    use std::collections::BTreeMap;

    // (total, active, expired, unbounded, legacy_no_metadata)
    let mut rows: BTreeMap<String, (usize, usize, usize, usize, usize)> = BTreeMap::new();
    for e in entries {
        let slot = rows.entry(e.rule.clone()).or_insert((0, 0, 0, 0, 0));
        slot.0 += 1;
        match e.status {
            ExceptionStatus::Active => slot.1 += 1,
            ExceptionStatus::Expired => slot.2 += 1,
            ExceptionStatus::Unbounded => slot.3 += 1,
            ExceptionStatus::LegacyNoMetadata => slot.4 += 1,
        }
    }

    rows.into_iter()
        .map(
            |(rule, (total, active, expired, unbounded, legacy_no_metadata))| {
                serde_json::json!({
                    "rule": rule,
                    "total": total,
                    "active": active,
                    "expired": expired,
                    "unbounded": unbounded,
                    "legacy_no_metadata": legacy_no_metadata,
                })
            },
        )
        .collect()
}

fn format_entry(e: &locus_core::exceptions::ExceptionEntry) -> String {
    use locus_core::exceptions::ExceptionSource;
    let source = match e.source {
        ExceptionSource::Hint => "hint",
        ExceptionSource::Lockfile => "lock",
        ExceptionSource::CxExemptPath => "cx-exempt",
        ExceptionSource::AcknowledgedEmpty => "ack-empty",
    };
    let expires = e.expires.as_deref().unwrap_or("—");
    let reason = e.reason.as_deref().unwrap_or("");
    format!(
        "  {:<8} {:<40} expires {:<12} ({}) {}",
        e.rule, e.target, expires, source, reason
    )
}

fn print_status_section(
    entries: &[locus_core::exceptions::ExceptionEntry],
    status: locus_core::exceptions::ExceptionStatus,
    header: &str,
) {
    let rows: Vec<_> = entries.iter().filter(|e| e.status == status).collect();
    if !rows.is_empty() {
        println!();
        println!("{header}");
        for e in rows {
            println!("{}", format_entry(e));
        }
    }
}

fn print_by_rule_text(entries: &[locus_core::exceptions::ExceptionEntry]) {
    let grouped = group_by_rule(entries);
    println!("debt by rule ({} rules with suppressions)", grouped.len());
    for row in grouped {
        println!(
            "  {:<6} total {:<4} active {:<4} expired {:<4} unbounded {:<4} legacy-no-metadata {:<4}",
            row["rule"].as_str().unwrap_or(""),
            row["total"].as_u64().unwrap_or(0),
            row["active"].as_u64().unwrap_or(0),
            row["expired"].as_u64().unwrap_or(0),
            row["unbounded"].as_u64().unwrap_or(0),
            row["legacy_no_metadata"].as_u64().unwrap_or(0)
        );
    }
}

fn print_text(entries: &[locus_core::exceptions::ExceptionEntry]) {
    use locus_core::exceptions::ExceptionStatus;

    let (mut active, mut expired, mut unbounded, mut legacy_no_metadata) =
        (0usize, 0usize, 0usize, 0usize);
    for e in entries {
        match e.status {
            ExceptionStatus::Active => active += 1,
            ExceptionStatus::Expired => expired += 1,
            ExceptionStatus::Unbounded => unbounded += 1,
            ExceptionStatus::LegacyNoMetadata => legacy_no_metadata += 1,
        }
    }
    println!(
        "debt: {active} active, {expired} expired, {unbounded} unbounded, \
         {legacy_no_metadata} legacy-no-metadata ({} total)",
        entries.len()
    );

    print_status_section(
        entries,
        ExceptionStatus::Expired,
        "EXPIRED  (re-run `locus check` for LOCUS001 advisories)",
    );
    print_status_section(
        entries,
        ExceptionStatus::LegacyNoMetadata,
        "LEGACY-NO-METADATA  (pre-schema entries — add reason/expires/owner \
         or migrate to struct form)",
    );
    print_status_section(
        entries,
        ExceptionStatus::Unbounded,
        "UNBOUNDED  (no expiry — review or add one)",
    );
    print_status_section(entries, ExceptionStatus::Active, "ACTIVE");
}

#[cfg(test)]
mod tests {
    use super::*;
    use locus_core::exceptions::{ExceptionEntry, ExceptionSource, ExceptionStatus};

    fn entry(rule: &str, status: ExceptionStatus) -> ExceptionEntry {
        ExceptionEntry {
            source: ExceptionSource::Hint,
            rule: rule.to_string(),
            target: "src/lib.rs:1".to_string(),
            reason: None,
            expires: None,
            status,
        }
    }

    #[test]
    fn groups_counts_by_rule_and_status() {
        let entries = vec![
            entry("DG003", ExceptionStatus::Active),
            entry("DG003", ExceptionStatus::Expired),
            entry("DG003", ExceptionStatus::Unbounded),
            entry("OT004", ExceptionStatus::Active),
            entry("OT004", ExceptionStatus::Active),
        ];

        let rows = group_by_rule(&entries);
        assert_eq!(rows.len(), 2);

        let dg = rows
            .iter()
            .find(|r| r["rule"] == "DG003")
            .expect("DG003 row");
        assert_eq!(dg["total"], 3);
        assert_eq!(dg["active"], 1);
        assert_eq!(dg["expired"], 1);
        assert_eq!(dg["unbounded"], 1);

        let ot = rows
            .iter()
            .find(|r| r["rule"] == "OT004")
            .expect("OT004 row");
        assert_eq!(ot["total"], 2);
        assert_eq!(ot["active"], 2);
        assert_eq!(ot["expired"], 0);
        assert_eq!(ot["unbounded"], 0);
    }
}
