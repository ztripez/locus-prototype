//! CLI smoke tests for `locus check --format json|sarif` (issue #29).
//!
//! These tests cover the wiring between the governance pipeline,
//! `locus-report`, and the new `--format` flag. They assert structural
//! shape — schema version, severity mapping, presence of rules array —
//! rather than full text, since the diagnostic set on the sample
//! fixture evolves whenever new rules land.

use assert_cmd::Command;
use serde_json::Value;

fn fixture_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/sample-crate")
}

fn run_check(format: &str) -> Value {
    let bin = env!("CARGO_BIN_EXE_locus");
    let out = Command::new(bin)
        .arg("check")
        .arg("--workspace")
        .arg(fixture_path())
        .arg("--format")
        .arg(format)
        .output()
        .expect("invoke locus check");
    let stdout = std::str::from_utf8(&out.stdout).expect("utf-8 stdout");
    serde_json::from_str(stdout).unwrap_or_else(|e| {
        panic!(
            "stdout was not valid JSON for --format {format}: {e}\nstdout:\n{stdout}\nstderr:\n{}",
            String::from_utf8_lossy(&out.stderr)
        )
    })
}

#[test]
fn json_format_emits_versioned_schema_with_results_and_summary() {
    let v = run_check("json");
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["tool"]["name"], "Locus");
    assert!(v["tool"]["version"].is_string());
    assert!(v["results"].is_array());
    let summary = &v["summary"];
    assert!(summary["fatal"].is_u64());
    assert!(summary["warning"].is_u64());
    assert!(summary["advisory"].is_u64());

    // Sample fixture has a known OT002 finding plus several LOCUS002
    // vacancy advisories — assert *something* came through so we know
    // the pipeline actually ran end-to-end.
    let results = v["results"].as_array().unwrap();
    assert!(
        !results.is_empty(),
        "expected at least one diagnostic from sample fixture"
    );
    let first = &results[0];
    assert!(first["rule_id"].is_string());
    assert!(first["severity"].is_string());
    assert!(first["location"]["file"].is_string());
    // Governance-produced diagnostics carry a `decision` block.
    assert!(
        first["decision"]["policy_id"].is_string(),
        "expected governance-produced diagnostic to include decision metadata, got: {first}"
    );
}

#[test]
fn sarif_format_emits_v210_envelope() {
    let v = run_check("sarif");
    assert_eq!(v["version"], "2.1.0");
    assert!(
        v["$schema"]
            .as_str()
            .unwrap_or("")
            .contains("sarif-schema-2.1.0"),
        "expected SARIF schema URL, got: {}",
        v["$schema"]
    );
    let runs = v["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 1);
    let driver = &runs[0]["tool"]["driver"];
    assert_eq!(driver["name"], "Locus");
    assert!(driver["version"].is_string());
    assert!(driver["informationUri"].is_string());
    let rules = driver["rules"].as_array().unwrap();
    assert!(!rules.is_empty(), "expected rule descriptors");
    let levels: std::collections::BTreeSet<&str> = rules
        .iter()
        .map(|r| r["defaultConfiguration"]["level"].as_str().unwrap())
        .collect();
    assert!(
        levels
            .iter()
            .all(|l| matches!(*l, "error" | "warning" | "note")),
        "all SARIF levels must be error/warning/note, got: {levels:?}"
    );
    let results = runs[0]["results"].as_array().unwrap();
    assert!(!results.is_empty());
    let first = &results[0];
    assert!(first["ruleId"].is_string());
    assert!(matches!(
        first["level"].as_str().unwrap(),
        "error" | "warning" | "note"
    ));
    assert!(first["message"]["text"].is_string());
    assert!(first["locations"][0]["physicalLocation"]["artifactLocation"]["uri"].is_string());
}

#[test]
fn deprecated_json_flag_still_emits_stable_json() {
    // `--json` is hidden from `--help` but kept as an alias so pre-#29
    // CI scripts and editor integrations keep working. It must produce
    // the same envelope as `--format json`.
    let bin = env!("CARGO_BIN_EXE_locus");
    let out = Command::new(bin)
        .arg("check")
        .arg("--workspace")
        .arg(fixture_path())
        .arg("--json")
        .output()
        .expect("invoke locus check");
    let stdout = std::str::from_utf8(&out.stdout).expect("utf-8 stdout");
    let v: Value =
        serde_json::from_str(stdout).expect("--json alias must still produce valid JSON");
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["tool"]["name"], "Locus");
    assert!(v["results"].is_array());
}

#[test]
fn deprecated_json_flag_overrides_format_text() {
    // When both `--json` and `--format text` are passed, the deprecated
    // alias wins. Documented behaviour so old scripts that explicitly
    // pass `--json` aren't surprised by a future addition of `--format`.
    let bin = env!("CARGO_BIN_EXE_locus");
    let out = Command::new(bin)
        .arg("check")
        .arg("--workspace")
        .arg(fixture_path())
        .arg("--json")
        .arg("--format")
        .arg("text")
        .output()
        .expect("invoke locus check");
    let stdout = std::str::from_utf8(&out.stdout).expect("utf-8 stdout");
    let _: Value = serde_json::from_str(stdout)
        .expect("--json must override --format text and still produce JSON");
}

#[test]
fn default_format_is_human_text() {
    // `--format` defaults to text — same output as the legacy human
    // mode. We assert the well-known summary line is present.
    let bin = env!("CARGO_BIN_EXE_locus");
    let out = Command::new(bin)
        .arg("check")
        .arg("--workspace")
        .arg(fixture_path())
        .output()
        .expect("invoke locus check");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("summary:") && stdout.contains("warning(s)"),
        "expected text summary, got: {stdout}"
    );
}
