//! Integration tests for `locus query`.

use assert_cmd::Command;

fn sample_fixture() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/sample-crate")
}

fn markers_fixture() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/markers-crate")
}

#[test]
fn query_canonical_finds_ot_canonical_hints() {
    let bin = env!("CARGO_BIN_EXE_locus");
    let assert = Command::new(bin)
        .arg("query")
        .arg("canonical")
        .arg("--workspace")
        .arg(sample_fixture())
        .assert()
        .success();
    let out = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    // sample-crate's `identity.rs` has `// locus: ot canonical` on User
    assert!(out.contains("sample_crate::identity::User"), "out: {out}");
}

#[test]
fn query_boundary_finds_dto_hints() {
    let bin = env!("CARGO_BIN_EXE_locus");
    let assert = Command::new(bin)
        .arg("query")
        .arg("boundary")
        .arg("--workspace")
        .arg(sample_fixture())
        .assert()
        .success();
    let out = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    assert!(out.contains("UserDto") || out.contains("dto"), "out: {out}");
}

#[test]
fn query_converter_finds_conversion_items() {
    let bin = env!("CARGO_BIN_EXE_locus");
    let assert = Command::new(bin)
        .arg("query")
        .arg("converter")
        .arg("--workspace")
        .arg(sample_fixture())
        .assert()
        .success();
    let out = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    // sample-crate has `From<User> for UserDto` conversions; output should
    // be non-empty (either "(no matches detected)" or a row — we expect rows)
    assert!(
        !out.contains("(no matches detected)"),
        "expected converter rows; got: {out}"
    );
}

#[test]
fn query_unknown_kind_exits_2_with_supported_list() {
    let bin = env!("CARGO_BIN_EXE_locus");
    let assert = Command::new(bin)
        .arg("query")
        .arg("definitely-not-a-real-kind")
        .arg("--workspace")
        .arg(sample_fixture())
        .assert()
        .failure()
        .code(2);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    assert!(stderr.contains("unknown query kind"), "stderr: {stderr}");
    assert!(
        stderr.contains("canonical"),
        "supported list missing: {stderr}"
    );
    assert!(
        stderr.contains("hot-path"),
        "supported list missing: {stderr}"
    );
}

#[test]
fn query_zero_matches_exits_0() {
    let bin = env!("CARGO_BIN_EXE_locus");
    // hot-path has no marker fixtures in sample-crate
    let assert = Command::new(bin)
        .arg("query")
        .arg("hot-path")
        .arg("--workspace")
        .arg(sample_fixture())
        .assert()
        .success();
    let out = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    assert!(out.contains("no matches detected"), "out: {out}");
}

#[test]
fn query_json_output_parses() {
    let bin = env!("CARGO_BIN_EXE_locus");
    let assert = Command::new(bin)
        .arg("query")
        .arg("canonical")
        .arg("--workspace")
        .arg(sample_fixture())
        .arg("--json")
        .assert()
        .success();
    let out = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    let parsed: serde_json::Value =
        serde_json::from_str(&out).unwrap_or_else(|e| panic!("not valid JSON: {e}\nout: {out}"));
    assert!(parsed.is_array(), "expected array; got: {out}");
    let arr = parsed.as_array().unwrap();
    assert!(!arr.is_empty(), "expected at least one row");
    let first = &arr[0];
    assert!(first.get("symbol").is_some());
    assert!(first.get("path").is_some());
    assert!(first.get("line").is_some());
    assert_eq!(first["kind"], "canonical");
}

/// Positive coverage for the `query_fact` / `locate_fact` path. Uses the
/// markers-crate fixture which has `// locus: fact hot-path` annotations
/// promoted to `AirFact` entries by the markers loader. Without this
/// test, every fact-derived kind is only covered indirectly by the
/// kebab→FactKind mapping unit test.
#[test]
fn query_hot_path_finds_marker_fact() {
    let bin = env!("CARGO_BIN_EXE_locus");
    let assert = Command::new(bin)
        .arg("query")
        .arg("hot-path")
        .arg("--workspace")
        .arg(markers_fixture())
        .assert()
        .success();
    let out = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    assert!(out.contains("hot-path"), "header missing; out: {out}");
    assert!(
        out.contains("markers_crate::frame_step"),
        "expected the marker-annotated symbol; out: {out}"
    );
    assert!(
        !out.contains("no matches detected"),
        "expected at least one row; out: {out}"
    );
}

/// JSON-mode coverage of the fact-derived path: the row must carry
/// `source` (loader name) and `evidence` fields, distinguishing it from
/// hint-derived rows.
#[test]
fn query_hot_path_json_includes_source_field() {
    let bin = env!("CARGO_BIN_EXE_locus");
    let assert = Command::new(bin)
        .arg("query")
        .arg("hot-path")
        .arg("--workspace")
        .arg(markers_fixture())
        .arg("--json")
        .assert()
        .success();
    let out = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    let parsed: serde_json::Value =
        serde_json::from_str(&out).unwrap_or_else(|e| panic!("not valid JSON: {e}\nout: {out}"));
    let arr = parsed.as_array().expect("expected array");
    assert!(!arr.is_empty(), "expected ≥1 row; got: {out}");
    let first = &arr[0];
    assert_eq!(first["kind"], "hot-path");
    assert!(
        first.get("source").is_some(),
        "fact rows must carry a `source` field"
    );
}
