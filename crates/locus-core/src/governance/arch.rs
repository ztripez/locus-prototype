//! `.locus/arch.json` loader for the Locus architecture declaration.
//!
//! MVP shape: a flat list of expected governance policies. Loaded by
//! `governance::run`; the optional declaration is consumed by
//! `RegistryCoherencePolicy` to surface drift (declared-but-not-registered,
//! registered-but-not-declared, missing file).

// locus: ot boundary cli.arch_declaration arch_declaration

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Relative path of the architecture declaration from the workspace root.
pub const ARCH_RELATIVE_PATH: &str = ".locus/arch.json";

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct ArchDeclaration {
    /// Names of governance policies the workspace expects to be active.
    /// Match `PolicyId` literals (e.g. `"registry-integrity"`).
    #[serde(default)]
    pub policies: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchLoadOutcome {
    /// File found and parsed successfully.
    Present(ArchDeclaration),
    /// `.locus/arch.json` doesn't exist.
    Missing,
    /// File exists but failed to parse (returns the raw error message for diagnostics).
    Invalid(String),
}

impl ArchDeclaration {
    /// Load from `<workspace>/.locus/arch.json`. Never panics.
    pub fn load(workspace_root: &Path) -> ArchLoadOutcome {
        let path = workspace_root.join(ARCH_RELATIVE_PATH);
        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<ArchDeclaration>(&text) {
                Ok(decl) => ArchLoadOutcome::Present(decl),
                Err(e) => ArchLoadOutcome::Invalid(e.to_string()),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => ArchLoadOutcome::Missing,
            Err(e) => {
                ArchLoadOutcome::Invalid(format!("io error reading {ARCH_RELATIVE_PATH}: {e}"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_arch(dir: &Path, contents: &str) {
        let locus_dir = dir.join(".locus");
        fs::create_dir_all(&locus_dir).unwrap();
        fs::write(locus_dir.join("arch.json"), contents).unwrap();
    }

    #[test]
    fn round_trips_minimal_declaration() {
        let tmp = tempfile::tempdir().unwrap();
        write_arch(
            tmp.path(),
            r#"{"policies": ["registry-integrity", "registry-coherence"]}"#,
        );
        let outcome = ArchDeclaration::load(tmp.path());
        match outcome {
            ArchLoadOutcome::Present(decl) => {
                assert_eq!(
                    decl.policies,
                    vec![
                        "registry-integrity".to_string(),
                        "registry-coherence".to_string()
                    ]
                );
            }
            other => panic!("expected Present; got {other:?}"),
        }
    }

    #[test]
    fn defaults_policies_to_empty_when_omitted() {
        let tmp = tempfile::tempdir().unwrap();
        write_arch(tmp.path(), "{}");
        match ArchDeclaration::load(tmp.path()) {
            ArchLoadOutcome::Present(decl) => assert!(decl.policies.is_empty()),
            other => panic!("expected Present with empty policies; got {other:?}"),
        }
    }

    #[test]
    fn missing_for_unknown_path() {
        let tmp = tempfile::tempdir().unwrap();
        let outcome = ArchDeclaration::load(tmp.path());
        assert_eq!(outcome, ArchLoadOutcome::Missing);
    }

    #[test]
    fn invalid_for_malformed_json() {
        let tmp = tempfile::tempdir().unwrap();
        write_arch(tmp.path(), "{not valid json");
        match ArchDeclaration::load(tmp.path()) {
            ArchLoadOutcome::Invalid(msg) => {
                assert!(
                    !msg.is_empty(),
                    "Invalid variant should carry a non-empty error string"
                );
            }
            other => panic!("expected Invalid; got {other:?}"),
        }
    }

    #[test]
    fn arch_relative_path_is_dot_locus_arch_json() {
        assert_eq!(ARCH_RELATIVE_PATH, ".locus/arch.json");
    }
}
