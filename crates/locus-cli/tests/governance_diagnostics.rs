//! Snapshot test: LOCUS003 advisories appear for un-migrated legacy rules.
//!
//! Uses the dg-public-api fixture, which triggers DG003 (still a legacy
//! rule). LOCUS003 for DG003 appears here. The compat snapshots deliberately
//! exclude this output (they cover pass-through only).

use assert_cmd::Command;

#[test]
fn check_dg_public_api_shows_locus003_for_dg003() {
    let bin = env!("CARGO_BIN_EXE_locus");
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/dg-public-api")
        .canonicalize()
        .expect("dg-public-api fixture resolves");

    let assert = Command::new(bin)
        .arg("check")
        .arg("--workspace")
        .arg(&src)
        .assert();
    let output = assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    let mut settings = insta::Settings::clone_current();
    settings.add_filter(r"/[A-Za-z0-9_./\-]*dg-public-api", "<DG_FIXTURE>");
    settings.bind(|| {
        insta::assert_snapshot!(
            "check_dg_public_api",
            format!("STDOUT:\n{stdout}\nSTDERR:\n{stderr}")
        )
    });
}
