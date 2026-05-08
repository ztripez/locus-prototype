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
