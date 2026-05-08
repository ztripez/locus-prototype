use assert_cmd::Command;

#[test]
fn init_against_sample_crate_emits_expected_checklist() {
    let bin = env!("CARGO_BIN_EXE_locus");
    let workspace_dir = tempfile::tempdir().unwrap();
    let src =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/sample-crate");
    copy_dir_all(&src, workspace_dir.path()).unwrap();

    let assert = Command::new(bin)
        .arg("init")
        .arg("--workspace")
        .arg(workspace_dir.path())
        .assert();
    let output = assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();

    let mut settings = insta::Settings::clone_current();
    settings.add_filter(r"wrote .*locus\.lock", "wrote <PATH>/locus.lock");
    settings.add_filter(r"updated .*locus\.lock", "updated <PATH>/locus.lock");
    settings.bind(|| insta::assert_snapshot!("init_sample_crate", stdout));
}

/// Snapshot the `locus init` output for a workspace that contains a
/// concept-shaped cluster (`User` + `UserResponse` + a `From` impl) but no
/// `// ot:` hints. The snapshot is the regression baseline; we also assert
/// the layer-suggestion block carries the cluster-crate domain glob.
///
/// The current `init` flow auto-promotes hints into the OT section before
/// calling `suggest()`, and `suggest()` requires at least one hinted
/// `Canonical` member to fire. A hint-less fixture therefore emits no
/// `[concept]` block — the snapshot reflects this. Relaxing that
/// requirement (e.g. picking a heuristic canonical when the cluster has a
/// converter) is tracked separately.
#[test]
fn init_against_cluster_crate_snapshots_checklist() {
    let bin = env!("CARGO_BIN_EXE_locus");
    let workspace_dir = tempfile::tempdir().unwrap();
    let src =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/cluster-crate");
    copy_dir_all(&src, workspace_dir.path()).unwrap();

    let assert = Command::new(bin)
        .arg("init")
        .arg("--workspace")
        .arg(workspace_dir.path())
        .assert();
    let output = assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();

    let mut settings = insta::Settings::clone_current();
    settings.add_filter(r"wrote .*locus\.lock", "wrote <PATH>/locus.lock");
    settings.add_filter(r"updated .*locus\.lock", "updated <PATH>/locus.lock");
    settings.bind(|| insta::assert_snapshot!("init_cluster_crate", stdout));

    assert!(
        stdout.contains("cluster_crate::domain::*"),
        "expected layer suggestion to mention the cluster-crate domain glob; got:\n{stdout}"
    );
}

/// Round-trip the OT acceptance flow: the cluster-crate fixture starts
/// without a `user` concept in its lockfile, `accept canonical` +
/// `accept boundary` populate it, and a second `init` run preserves the
/// accepted entries (re-running `paradigm.init` against a hint-less
/// workspace returns an empty section, which is then *not* clobbered into
/// the existing lockfile).
#[test]
fn cluster_round_trip_persists_accepted_concept() {
    let bin = env!("CARGO_BIN_EXE_locus");
    let workspace_dir = tempfile::tempdir().unwrap();
    let src =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/cluster-crate");
    copy_dir_all(&src, workspace_dir.path()).unwrap();

    // First run: lockfile gets created with empty OT section. `init` exits
    // non-zero when unresolved suggestions remain, so we use `.output()`
    // (no exit-code enforcement) instead of `.assert()`.
    let _ = Command::new(bin)
        .arg("init")
        .arg("--workspace")
        .arg(workspace_dir.path())
        .output()
        .unwrap();
    let lockfile_path = workspace_dir.path().join("locus.lock");
    let initial = std::fs::read_to_string(&lockfile_path).unwrap();
    assert!(
        !initial.contains("\"user\""),
        "fresh init should not pre-populate the user concept; got:\n{initial}"
    );

    // Apply the accept commands a `[concept]`-style suggestion would propose.
    Command::new(bin)
        .args([
            "accept",
            "canonical",
            "cluster_crate::domain::User",
            "--concept",
            "user",
        ])
        .arg("--workspace")
        .arg(workspace_dir.path())
        .assert()
        .success();
    Command::new(bin)
        .args([
            "accept",
            "boundary",
            "cluster_crate::api::UserResponse",
            "--concept",
            "user",
        ])
        .arg("--workspace")
        .arg(workspace_dir.path())
        .assert()
        .success();

    let after_accept = std::fs::read_to_string(&lockfile_path).unwrap();
    assert!(
        after_accept.contains("cluster_crate::domain::User"),
        "accept canonical should have written the User symbol; got:\n{after_accept}"
    );
    assert!(
        after_accept.contains("cluster_crate::api::UserResponse"),
        "accept boundary should have written the UserResponse symbol; got:\n{after_accept}"
    );

    // Second run: re-running init must preserve the accepted entries.
    let _ = Command::new(bin)
        .arg("init")
        .arg("--workspace")
        .arg(workspace_dir.path())
        .output()
        .unwrap();
    let after_second_init = std::fs::read_to_string(&lockfile_path).unwrap();
    assert!(
        after_second_init.contains("cluster_crate::domain::User"),
        "second init must preserve accepted canonical; got:\n{after_second_init}"
    );
    assert!(
        after_second_init.contains("cluster_crate::api::UserResponse"),
        "second init must preserve accepted boundary; got:\n{after_second_init}"
    );
}

fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let dst_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &dst_path)?;
        } else {
            std::fs::copy(entry.path(), &dst_path)?;
        }
    }
    Ok(())
}
