//! Cargo / rustdoc invocation helpers for [`RustdocJsonBackend`].
//!
//! Owns the process-IO boundary: shelling out to `cargo metadata` for
//! the workspace member list, then `cargo rustdoc --output-format
//! json` per crate, then reading + parsing the resulting JSON. The
//! rest of the backend (struct, trait impl) lives in
//! [`super::backend`]; the rustdoc-types → AIR translation lives in
//! [`super::walk`].

// locus: ot canonical

use std::path::{Path, PathBuf};
use std::process::Command;

use rustdoc_types::Crate;

use crate::AdapterError;

/// JSON format version this backend was built against. The
/// `rustdoc-types` crate version pinned in `Cargo.toml` (`0.57.x`)
/// matches rustdoc's emitted `format_version = 57`. Format-version
/// mismatches between the toolchain and the parser are caught up-front
/// and surfaced as [`AdapterError::BackendUnavailable`] rather than
/// producing partially-decoded junk.
pub(super) const SUPPORTED_FORMAT_VERSION: u32 = rustdoc_types::FORMAT_VERSION;

/// Bare-minimum subset of `cargo metadata` we need: the list of
/// workspace-member package names. Shelled out via
/// `cargo +<toolchain> metadata --no-deps --format-version 1`.
pub(super) fn list_workspace_packages(
    toolchain: &str,
    workspace_root: &Path,
) -> Result<Vec<String>, AdapterError> {
    let plus = format!("+{toolchain}");
    let output = Command::new("cargo")
        .arg(&plus)
        .arg("metadata")
        .arg("--no-deps")
        .arg("--format-version")
        .arg("1")
        .arg("--manifest-path")
        .arg(workspace_root.join("Cargo.toml"))
        .output()
        .map_err(|e| AdapterError::BackendUnavailable(format!("cargo invocation failed: {e}")))?;
    if !output.status.success() {
        return Err(AdapterError::WorkspaceFailed {
            message: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| AdapterError::BackendUnavailable(format!("cargo metadata json: {e}")))?;
    let members = parsed
        .get("workspace_members")
        .and_then(|m| m.as_array())
        .ok_or_else(|| {
            AdapterError::BackendUnavailable("no workspace_members in metadata".into())
        })?;
    let member_ids: std::collections::HashSet<&str> =
        members.iter().filter_map(|m| m.as_str()).collect();
    let packages = parsed
        .get("packages")
        .and_then(|p| p.as_array())
        .ok_or_else(|| AdapterError::BackendUnavailable("no packages in metadata".into()))?;
    let mut names = Vec::new();
    for pkg in packages {
        let id = pkg.get("id").and_then(|i| i.as_str()).unwrap_or("");
        if !member_ids.contains(id) {
            continue;
        }
        if let Some(name) = pkg.get("name").and_then(|n| n.as_str()) {
            names.push(name.to_string());
        }
    }
    Ok(names)
}

/// Invoke `cargo +<toolchain> rustdoc -p <package> -- -Zunstable-options
/// --output-format json` and return the resulting JSON file's path.
///
/// Passes an explicit `--target-dir <workspace_root>/target` so the
/// output location is deterministic regardless of CI configuration
/// (e.g. `CARGO_TARGET_DIR`, `.cargo/config.toml` redirects, build
/// cache mounts, or workspace-nesting heuristics). Without this CI
/// can land the JSON somewhere we don't look.
pub(super) fn run_rustdoc_for(
    toolchain: &str,
    workspace_root: &Path,
    package: &str,
) -> Result<PathBuf, AdapterError> {
    let target_dir = workspace_root.join("target");
    let output = Command::new("cargo")
        .arg(format!("+{toolchain}"))
        .arg("rustdoc")
        .arg("-p")
        .arg(package)
        .arg("--lib")
        .arg("--manifest-path")
        .arg(workspace_root.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(&target_dir)
        .arg("--")
        .arg("-Zunstable-options")
        .arg("--output-format")
        .arg("json")
        .output()
        .map_err(|e| AdapterError::BackendUnavailable(format!("cargo rustdoc failed: {e}")))?;
    if !output.status.success() {
        return Err(AdapterError::WorkspaceFailed {
            message: format!(
                "cargo rustdoc -p {package}: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        });
    }
    locate_rustdoc_json(&target_dir, package)
}

/// `cargo rustdoc --output-format json --target-dir <dir>` writes to
/// `<dir>/doc/<crate>.json`, where `<crate>` is the package name with
/// `-` replaced by `_`. Returns an error pointing at the expected path
/// when rustdoc claimed success but the JSON isn't there.
fn locate_rustdoc_json(target_dir: &Path, package: &str) -> Result<PathBuf, AdapterError> {
    let crate_name = package.replace('-', "_");
    let json_path = target_dir.join("doc").join(format!("{crate_name}.json"));
    if !json_path.exists() {
        return Err(AdapterError::WorkspaceFailed {
            message: format!(
                "rustdoc JSON not produced: expected {}",
                json_path.display()
            ),
        });
    }
    Ok(json_path)
}

pub(super) fn parse_rustdoc_json(json_path: &Path) -> Result<Crate, AdapterError> {
    let bytes = std::fs::read(json_path).map_err(|e| AdapterError::WorkspaceFailed {
        message: format!("read {}: {e}", json_path.display()),
    })?;
    let krate: Crate =
        serde_json::from_slice(&bytes).map_err(|e| AdapterError::WorkspaceFailed {
            message: format!("parse {}: {e}", json_path.display()),
        })?;
    if krate.format_version != SUPPORTED_FORMAT_VERSION {
        return Err(AdapterError::BackendUnavailable(format!(
            "rustdoc JSON format_version {} (this backend supports {})",
            krate.format_version, SUPPORTED_FORMAT_VERSION
        )));
    }
    Ok(krate)
}
