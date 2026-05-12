//! Snapshot test: DG003 (registered rule in P4) fires on the dg-public-api
//! fixture without LOCUS003 overhead. DG003 is now a `RuleDefinition` — it
//! emits findings directly through the governance pipeline. The compat
//! snapshots cover pass-through only; this fixture isolates DG3 behaviour.

use assert_cmd::Command;

#[test]
fn check_dg_public_api_dg003_fires_as_registered_rule() {
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
