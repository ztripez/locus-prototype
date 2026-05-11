//! Compatibility golden snapshot — P1 of epic #71.
//!
//! Asserts the new governance pipeline produces byte-identical output to
//! the legacy `paradigm.check()` loop on `tests/fixtures/sample-crate`.
//! If this test fails, either:
//!   - a non-pass-through policy was added (intentional — update the
//!     governance-only snapshot in P3, not this one), or
//!   - the legacy adapter changed how it synthesizes findings
//!     (unintentional — investigate).

use assert_cmd::Command;

#[test]
fn check_sample_crate_output_is_stable() {
    let bin = env!("CARGO_BIN_EXE_locus");
    let src =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/sample-crate");

    let assert = Command::new(bin)
        .arg("check")
        .arg("--workspace")
        .arg(&src)
        .assert();
    let output = assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    let mut settings = insta::Settings::clone_current();
    settings.add_filter(r"/[A-Za-z0-9_./\-]*sample-crate", "<FIXTURE>");
    settings.bind(|| {
        insta::assert_snapshot!(
            "check_sample_crate",
            format!("STDOUT:\n{stdout}\nSTDERR:\n{stderr}")
        )
    });
}

#[test]
fn check_sample_crate_agent_strict_output_is_stable() {
    let bin = env!("CARGO_BIN_EXE_locus");
    let src =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/sample-crate");

    let assert = Command::new(bin)
        .arg("check")
        .arg("--workspace")
        .arg(&src)
        .arg("--agent-strict")
        .assert();
    let output = assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    let mut settings = insta::Settings::clone_current();
    settings.add_filter(r"/[A-Za-z0-9_./\-]*sample-crate", "<FIXTURE>");
    settings.bind(|| {
        insta::assert_snapshot!(
            "check_sample_crate_agent_strict",
            format!("STDOUT:\n{stdout}\nSTDERR:\n{stderr}")
        )
    });
}
