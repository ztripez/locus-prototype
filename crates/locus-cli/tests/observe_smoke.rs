//! Integration test: `locus observe` MVP on the sample-crate fixture.
//!
//! Asserts:
//! - exit code is 0 (always)
//! - output contains the three section headers
//! - survey runs before advisory pressure
//! - declarations come last
//! - no "error" or "warning" output (everything is advisory)
//! - snapshot of full output for regression baseline

use assert_cmd::Command;

#[test]
fn observe_sample_crate_succeeds() {
    let bin = env!("CARGO_BIN_EXE_locus");
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/sample-crate");

    let assert = Command::new(bin)
        .arg("observe")
        .arg("--workspace")
        .arg(&src)
        .assert()
        .success();

    let output = assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();

    // Section ordering check
    let survey_idx = stdout
        .find("Architecture survey")
        .expect("survey section missing");
    let pressure_idx = stdout
        .find("Advisory pressure")
        .expect("pressure section missing");
    let decls_idx = stdout
        .find("Next declarations")
        .expect("declarations section missing");
    assert!(
        survey_idx < pressure_idx,
        "survey must come before advisory pressure"
    );
    assert!(
        pressure_idx < decls_idx,
        "advisory pressure must come before next declarations"
    );

    // No enforcement vocabulary
    assert!(
        !stdout.contains("error["),
        "observe must not render error diagnostics"
    );
    assert!(
        !stdout.contains("warning["),
        "observe must not render warning diagnostics"
    );

    // Snapshot the redacted output for regression baseline
    let mut settings = insta::Settings::clone_current();
    settings.add_filter(r"/[A-Za-z0-9_./\-]*sample-crate", "<FIXTURE>");
    settings.add_filter(r"\d+ lines", "<N> lines");
    settings.add_filter(r"\d+ findings", "<N> findings");
    settings.add_filter(r"\d+ finding\b", "<N> finding");
    settings.bind(|| insta::assert_snapshot!("observe_sample_crate", stdout));
}
