//! `.locus/lock.json` — accepted architecture decisions.
//!
//! One file shared by all paradigms; each paradigm owns its own section under
//! `paradigms.<prefix>`. Phase 2 only writes the OT section; the namespace is
//! reserved now so adding DG/CF/etc. later doesn't require a migration.
//!
//! The lockfile is generated and updated by CLI commands — humans review it
//! in PRs but don't hand-edit in normal use.

// locus: ot canonical

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use thiserror::Error;

pub const LOCKFILE_VERSION: u32 = 1;
/// Directory under the workspace root that holds all Locus configuration.
/// Mirrors `.git` / `.vscode` / `.github` conventions — a single hidden folder
/// for the tool's state. Future config files (architecture policy YAML, policy
/// declaration files, etc.) live alongside the lockfile here.
pub const LOCUS_DIR: &str = ".locus";
/// Path of the lockfile relative to the workspace root.
/// The single source of truth for "where the lockfile lives on disk".
pub const LOCKFILE_RELATIVE_PATH: &str = ".locus/lock.json";

/// Full struct form of an `acknowledged_empty` entry. Carries optional debt
/// metadata: `expires`, `reason`, `owner`, `debt_id`, and `introduced_by`.
/// All metadata fields are optional so the struct-form entry is still valid
/// without them (though `PG009` will fire for new entries lacking
/// `reason`/`expires`/`owner`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AcknowledgedEmpty {
    /// The paradigm prefix being acknowledged (e.g. `"BO"`, `"PA"`, …).
    pub prefix: String,
    /// Debt metadata — `YYYY-MM-DD` expiry date. Required by `PG009` on
    /// new entries added after the schema upgrade.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires: Option<String>,
    /// Debt metadata — human-readable explanation of why this paradigm is
    /// intentionally left empty. Required by `PG009` on new entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Debt metadata — owner team / individual / role. Required by
    /// `PG009` on new entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// Optional stable cross-reference identifier for the debt record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub debt_id: Option<String>,
    /// Optional PR / issue reference describing this acknowledgement's origin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced_by: Option<String>,
}

/// Serde-transparent enum that accepts both the legacy plain-string form
/// (`"BO"`) and the new struct form (`{"prefix": "BO", "reason": "…", …}`)
/// for entries in `Lockfile::acknowledged_empty`.
///
/// Backwards compatibility: the current `.locus/lock.json` uses `Vec<String>`;
/// those entries parse as `AcknowledgedEmptyEntry::Legacy` and are
/// surfaced in `locus debt` as `LegacyNoMetadata`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum AcknowledgedEmptyEntry {
    /// Legacy form: a bare paradigm prefix string. Deserializes from `"BO"`.
    Legacy(String),
    /// New form: a struct with `prefix` plus optional debt metadata.
    Full(AcknowledgedEmpty),
}

impl AcknowledgedEmptyEntry {
    /// Borrow the paradigm prefix regardless of which variant this entry is.
    pub fn prefix(&self) -> &str {
        match self {
            AcknowledgedEmptyEntry::Legacy(s) => s.as_str(),
            AcknowledgedEmptyEntry::Full(meta) => meta.prefix.as_str(),
        }
    }
}

impl From<String> for AcknowledgedEmptyEntry {
    fn from(s: String) -> Self {
        AcknowledgedEmptyEntry::Legacy(s)
    }
}

impl From<&str> for AcknowledgedEmptyEntry {
    fn from(s: &str) -> Self {
        AcknowledgedEmptyEntry::Legacy(s.to_string())
    }
}

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
    /// Paradigms the user has explicitly acknowledged as having no definitions.
    /// Vacant-by-definition paradigms (BO/PA/CR/RW/DA/UT/ER/FL/DG/CF/…) emit
    /// `LOCUS002` when their declaration lists are empty unless the prefix
    /// appears in this list. Empty list = full nag mode; populated list =
    /// "I really don't intend to use these paradigms."
    ///
    /// Entries may be plain strings (legacy form) or `AcknowledgedEmptyEntry`
    /// structs carrying debt metadata (`expires`, `reason`, `owner`,
    /// `debt_id`, `introduced_by`). Legacy strings are accepted via
    /// `#[serde(untagged)]` and surfaced in `locus debt` as legacy-no-metadata.
    #[serde(default)]
    pub acknowledged_empty: Vec<AcknowledgedEmptyEntry>,
}

impl Lockfile {
    pub fn empty() -> Self {
        Self {
            version: LOCKFILE_VERSION,
            paradigms: BTreeMap::new(),
            exceptions: Vec::new(),
            acknowledged_empty: Vec::new(),
        }
    }

    /// True if the user has explicitly acknowledged the named paradigm as
    /// having no definitions. Used by vacant-by-definition paradigms to
    /// suppress the LOCUS002 "missing definitions" diagnostic.
    pub fn is_acknowledged_empty(&self, prefix: &str) -> bool {
        self.acknowledged_empty.iter().any(|e| e.prefix() == prefix)
    }

    /// Load `.locus/lock.json` from a workspace root. Returns
    /// `Lockfile::empty()` if the file does not exist (so paradigms can run
    /// without a lockfile yet).
    pub fn load_or_empty(workspace_root: &Path) -> Result<Self, LockfileError> {
        let path = workspace_root.join(LOCKFILE_RELATIVE_PATH);
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

    /// Like [`Lockfile::paradigm_section`], but returns `None` when the section
    /// is not explicitly present in the lockfile. Use this when defaults must
    /// be distinguishable from explicit user configuration (e.g. Policy Guard
    /// auditing).
    ///
    /// `Some(Ok(T))` — the section was present and parsed cleanly.
    /// `Some(Err(_))` — the section was present but malformed for `T`.
    /// `None` — the section was absent; caller should treat this as
    /// "no explicit user policy" rather than silently injecting defaults.
    pub fn paradigm_section_explicit<T>(&self, prefix: &str) -> Option<Result<T, serde_json::Error>>
    where
        T: for<'de> Deserialize<'de>,
    {
        self.paradigms
            .get(prefix)
            .map(|v| serde_json::from_value(v.clone()))
    }

    /// Persist `.locus/lock.json` under the workspace root, pretty-printed for
    /// review. Creates the `.locus/` directory if it does not already exist.
    /// Overwrites the existing file.
    pub fn save(&self, workspace_root: &Path) -> Result<std::path::PathBuf, LockfileError> {
        let path = workspace_root.join(LOCKFILE_RELATIVE_PATH);
        let bytes = serde_json::to_vec_pretty(self).map_err(|source| LockfileError::Parse {
            path: path.clone(),
            source,
        })?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| LockfileError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
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
    fn paradigm_section_explicit_returns_none_when_missing() {
        let lf = Lockfile::empty();
        let res: Option<Result<serde_json::Value, _>> = lf.paradigm_section_explicit("OT");
        assert!(
            res.is_none(),
            "absent section must return None so callers can distinguish \
             user-set-empty from defaulted",
        );
    }

    #[test]
    fn paradigm_section_explicit_returns_some_ok_when_present() {
        let mut lf = Lockfile::empty();
        lf.paradigms
            .insert("OT".to_string(), serde_json::json!({"concepts": {}}));
        let res: Option<Result<serde_json::Value, _>> = lf.paradigm_section_explicit("OT");
        let parsed = res
            .expect("section is explicitly present; expected Some(_)")
            .expect("section parses as serde_json::Value");
        assert_eq!(parsed, serde_json::json!({"concepts": {}}));
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

    // ---- AcknowledgedEmptyEntry schema tests -------------------------

    #[test]
    fn legacy_string_parses_as_acknowledged_empty_entry() {
        let json = r#""BO""#;
        let entry: AcknowledgedEmptyEntry = serde_json::from_str(json).unwrap();
        match &entry {
            AcknowledgedEmptyEntry::Legacy(s) => assert_eq!(s, "BO"),
            AcknowledgedEmptyEntry::Full(_) => panic!("expected Legacy variant"),
        }
        assert_eq!(entry.prefix(), "BO");
    }

    #[test]
    fn struct_form_parses_as_acknowledged_empty_entry() {
        let json = r#"{"prefix":"PA","expires":"2027-05-09","reason":"no ports yet","owner":"@core","debt_id":"ack-empty-PA","introduced_by":"PR #49"}"#;
        let entry: AcknowledgedEmptyEntry = serde_json::from_str(json).unwrap();
        match &entry {
            AcknowledgedEmptyEntry::Full(meta) => {
                assert_eq!(meta.prefix, "PA");
                assert_eq!(meta.expires.as_deref(), Some("2027-05-09"));
                assert_eq!(meta.reason.as_deref(), Some("no ports yet"));
                assert_eq!(meta.owner.as_deref(), Some("@core"));
                assert_eq!(meta.debt_id.as_deref(), Some("ack-empty-PA"));
                assert_eq!(meta.introduced_by.as_deref(), Some("PR #49"));
            }
            AcknowledgedEmptyEntry::Legacy(_) => panic!("expected Full variant"),
        }
        assert_eq!(entry.prefix(), "PA");
    }

    #[test]
    fn legacy_lockfile_vec_string_parses() {
        // Backwards-compatibility: the current .locus/lock.json has
        // "acknowledged_empty": ["BO", "CF", "CR", ...]
        let json = r#"{"version":1,"acknowledged_empty":["BO","CF","CR","DA"]}"#;
        let lf: Lockfile = serde_json::from_str(json).unwrap();
        assert_eq!(lf.acknowledged_empty.len(), 4);
        assert!(matches!(
            &lf.acknowledged_empty[0],
            AcknowledgedEmptyEntry::Legacy(s) if s == "BO"
        ));
        assert_eq!(lf.acknowledged_empty[0].prefix(), "BO");
        assert!(lf.is_acknowledged_empty("CF"));
        assert!(!lf.is_acknowledged_empty("PA"));
    }

    #[test]
    fn mixed_legacy_and_struct_forms_parse() {
        let json = r#"{"version":1,"acknowledged_empty":[
            "BO",
            {"prefix":"PA","expires":"2027-05-09","reason":"no ports","owner":"@core"}
        ]}"#;
        let lf: Lockfile = serde_json::from_str(json).unwrap();
        assert_eq!(lf.acknowledged_empty.len(), 2);
        assert_eq!(lf.acknowledged_empty[0].prefix(), "BO");
        assert_eq!(lf.acknowledged_empty[1].prefix(), "PA");
        assert!(lf.is_acknowledged_empty("BO"));
        assert!(lf.is_acknowledged_empty("PA"));
        assert!(!lf.is_acknowledged_empty("CR"));
    }

    #[test]
    fn struct_form_round_trips() {
        let entry = AcknowledgedEmptyEntry::Full(AcknowledgedEmpty {
            prefix: "RW".to_string(),
            expires: Some("2027-05-09".to_string()),
            reason: Some("no runtime owners yet".to_string()),
            owner: Some("@core".to_string()),
            debt_id: Some("ack-empty-RW".to_string()),
            introduced_by: Some("PR #49".to_string()),
        });
        let json = serde_json::to_string(&entry).unwrap();
        let back: AcknowledgedEmptyEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(entry, back);
        assert_eq!(back.prefix(), "RW");
    }

    #[test]
    fn legacy_entry_prefix_accessor() {
        let entry = AcknowledgedEmptyEntry::Legacy("FL".to_string());
        assert_eq!(entry.prefix(), "FL");
    }

    #[test]
    fn full_entry_prefix_accessor() {
        let entry = AcknowledgedEmptyEntry::Full(AcknowledgedEmpty {
            prefix: "UT".to_string(),
            ..Default::default()
        });
        assert_eq!(entry.prefix(), "UT");
    }
}
