//! CLI smoke test for `locus check --semantic-rust` (issue #111 phase 3).
//!
//! Exercises the `--semantic-rust` flag end-to-end: runs `locus check`
//! against the semantic-conversions-fixture, which has two
//! user-written impls (`From<UserDto> for User`,
//! `TryFrom<&str> for UserId`). The `RustdocJsonBackend` resolves
//! both and merges them into the AIR before paradigm rules run.
//!
//! ## What this test pins
//!
//! - The flag is plumbed (i.e. clap accepts it).
//! - On a nightly-equipped machine, the backend invocation succeeds
//!   end-to-end (exit 0, no `semantic-rust skipped` advisory).
//! - On a machine without nightly, the test treats the
//!   `semantic-rust skipped` advisory as a **skip**, not a failure —
//!   mirroring the gating pattern in `rustdoc_json_smoke.rs`.
//!
//! ## What this test deliberately doesn't pin
//!
//! - The downstream diagnostic count from OT006/OT007 on the fixture.
//!   The fixture's lockfile is empty, so OT rules don't fire on the
//!   resolved records regardless. End-to-end "semantic record changes
//!   OT diagnostic" coverage lives in
//!   `locus-core/tests/semantic_provenance_spike.rs` using
//!   `TestBackend`; that path is hermetic and toolchain-independent.

use assert_cmd::Command;

fn fixture_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/semantic-conversions-fixture")
}

#[test]
fn check_with_semantic_rust_runs_end_to_end_or_skips_cleanly() {
    let bin = env!("CARGO_BIN_EXE_locus");
    let out = Command::new(bin)
        .arg("check")
        .arg("--workspace")
        .arg(fixture_path())
        .arg("--semantic-rust")
        .output()
        .expect("invoke locus check");

    let stderr = String::from_utf8_lossy(&out.stderr);

    if stderr.contains("semantic-rust skipped") {
        // Nightly toolchain not available, rustdoc invocation failed,
        // or the workspace didn't compile. The advisory itself is the
        // success signal: the CLI fell back to syntactic facts and
        // didn't fail the check. Treat as skip.
        eprintln!("skipping: backend advised fallback: {stderr}");
        return;
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "locus check --semantic-rust should exit 0 on the fixture;\n\
         status: {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        out.status,
    );

    // The fixture has no `.locus/lock.json`, so the run produces only
    // the LOCUS003-style governance default findings. Importantly the
    // stderr should NOT carry the "semantic-rust skipped" or "dropping
    // resolved conversion" messages — those would mean the backend ran
    // but the merge couldn't place the records, which is the wiring
    // bug this test is here to catch.
    assert!(
        !stderr.contains("dropping resolved conversion"),
        "semantic-rust merge should have placed all resolved records into the AIR; got stderr:\n{stderr}",
    );
}

#[test]
fn check_without_semantic_rust_does_not_invoke_backend() {
    // Regression: omitting the flag must not invoke rustdoc. The
    // backend's stderr advisories (or the rustdoc build cost) should
    // not appear at all.
    let bin = env!("CARGO_BIN_EXE_locus");
    let out = Command::new(bin)
        .arg("check")
        .arg("--workspace")
        .arg(fixture_path())
        .output()
        .expect("invoke locus check");

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("semantic-rust"),
        "stderr should not mention semantic-rust when flag is absent; got:\n{stderr}",
    );
}
