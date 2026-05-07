//! `locus.lock` — accepted architecture decisions.
//!
//! One file shared by all paradigms; each paradigm owns its own section under
//! `paradigms.<prefix>`. Phase 2 only writes the OT section; the namespace is
//! reserved now so adding DG/CF/etc. later doesn't require a migration.
//!
//! The lockfile is generated and updated by CLI commands — humans review it
//! in PRs but don't hand-edit in normal use.

// ot: canonical

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use thiserror::Error;

pub const LOCKFILE_VERSION: u32 = 1;
pub const LOCKFILE_NAME: &str = "locus.lock";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Lockfile {
    pub version: u32,
    /// Paradigm-namespaced sections. Keyed by rule prefix (`"OT"`, `"DG"`, …).
    /// Each section is opaque JSON owned by the paradigm; `locus-core` doesn't
    /// interpret it.
    #[serde(default)]
    pub paradigms: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub exceptions: Vec<Exception>,
}

impl Lockfile {
    pub fn empty() -> Self {
        Self {
            version: LOCKFILE_VERSION,
            paradigms: BTreeMap::new(),
            exceptions: Vec::new(),
        }
    }

    /// Load `locus.lock` from a workspace root. Returns `Lockfile::empty()` if
    /// the file does not exist (so paradigms can run without a lockfile yet).
    pub fn load_or_empty(workspace_root: &Path) -> Result<Self, LockfileError> {
        let path = workspace_root.join(LOCKFILE_NAME);
        if !path.exists() {
            return Ok(Self::empty());
        }
        let bytes = std::fs::read(&path).map_err(|source| LockfileError::Io {
            path: path.clone(),
            source,
        })?;
        let lf: Lockfile = serde_json::from_slice(&bytes)
            .map_err(|source| LockfileError::Parse { path, source })?;
        Ok(lf)
    }

    /// Pull a paradigm's section, deserialized into `T`. Returns `T::default()`
    /// if the paradigm has no section yet.
    pub fn paradigm_section<T>(&self, prefix: &str) -> Result<T, serde_json::Error>
    where
        T: for<'de> Deserialize<'de> + Default,
    {
        match self.paradigms.get(prefix) {
            Some(v) => serde_json::from_value(v.clone()),
            None => Ok(T::default()),
        }
    }

    /// Persist `locus.lock` at the workspace root, pretty-printed for review.
    /// Overwrites the existing file.
    pub fn save(&self, workspace_root: &Path) -> Result<std::path::PathBuf, LockfileError> {
        let path = workspace_root.join(LOCKFILE_NAME);
        let bytes = serde_json::to_vec_pretty(self).map_err(|source| LockfileError::Parse {
            path: path.clone(),
            source,
        })?;
        std::fs::write(&path, bytes).map_err(|source| LockfileError::Io {
            path: path.clone(),
            source,
        })?;
        Ok(path)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Exception {
    pub rule: String,
    pub target: String,
    pub reason: String,
    pub expires: String,
}

#[derive(Debug, Error)]
pub enum LockfileError {
    #[error("io error reading {path}: {source}")]
    Io {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("parse error in {path}: {source}")]
    Parse {
        path: std::path::PathBuf,
        #[source]
        source: serde_json::Error,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;

    #[test]
    fn empty_round_trips() {
        let lf = Lockfile::empty();
        let s = serde_json::to_string(&lf).unwrap();
        let lf2: Lockfile = serde_json::from_str(&s).unwrap();
        assert_eq!(lf2.version, LOCKFILE_VERSION);
        assert!(lf2.paradigms.is_empty());
    }

    #[test]
    fn paradigm_section_default_when_missing() {
        let lf = Lockfile::empty();
        let v: serde_json::Value = lf.paradigm_section("OT").unwrap();
        assert_eq!(v, serde_json::Value::Null);
    }

    #[test]
    fn parse_realistic_shape() {
        let s = indoc! {r#"
            {
              "version": 1,
              "paradigms": {
                "OT": {
                  "concepts": {
                    "identity.user": {
                      "canonical": "crate::domain::User",
                      "boundaries": ["crate::api::UserDto"]
                    }
                  }
                }
              },
              "exceptions": [
                {"rule": "OT002", "target": "src/legacy.rs:12", "reason": "import shim", "expires": "2026-12-01"}
              ]
            }
        "#};
        let lf: Lockfile = serde_json::from_str(s).unwrap();
        assert_eq!(lf.version, 1);
        assert!(lf.paradigms.contains_key("OT"));
        assert_eq!(lf.exceptions.len(), 1);
        assert_eq!(lf.exceptions[0].rule, "OT002");
    }
}
