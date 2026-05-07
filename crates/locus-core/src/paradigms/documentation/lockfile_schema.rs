//! Lockfile section shape for DC (Documentation / Comment Ownership).
//!
//! DC001 fires on public types and functions that have no doc comment.
//! Because "public API must be documented" is a project-wide policy choice,
//! the rule is gated on an explicit opt-in: `require_public_docs` defaults
//! to `false`, so DC is silent until the user turns it on. `exempt_paths`
//! lets the user carve out regions where the rule shouldn't apply
//! (test modules, generated code, FFI shims) without disabling the rule
//! entirely.

// ot: canonical

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DcSection {
    /// Top-level switch. Default `false` keeps DC001 silent until the user
    /// opts in — "public API must be documented" is a project policy, not
    /// a universal default.
    #[serde(default)]
    pub require_public_docs: bool,

    /// Module patterns matching `AirFile.module_path` whose contents skip
    /// the doc requirement. Typical entries: `*::tests::*`,
    /// `*::generated::*`, `*::ffi::*`. Pattern syntax mirrors UT/DG: simple
    /// suffix wildcards.
    #[serde(default)]
    pub exempt_paths: Vec<String>,

    /// Phrases that, when found (case-insensitive substring) in a public
    /// item's doc comment, fire DC002. Defaults to a high-signal seed list
    /// of LLM-transcript residue and stale planning markers (see
    /// [`default_forbidden_doc_phrases`]). Clearing the list opts out of
    /// DC002 entirely — DC002 stays silent when this is empty.
    #[serde(default = "default_forbidden_doc_phrases")]
    pub forbidden_doc_phrases: Vec<ForbiddenPhrase>,
}

impl Default for DcSection {
    fn default() -> Self {
        Self {
            require_public_docs: false,
            exempt_paths: Vec::new(),
            forbidden_doc_phrases: default_forbidden_doc_phrases(),
        }
    }
}

/// One entry in the DC002 forbidden-phrase list. Matched case-insensitively
/// as a substring of an item's doc text. `confidence` drives
/// [`crate::diagnostics::Severity::from_confidence`] — values below `0.50`
/// suppress the diagnostic entirely (intentional, so users can demote a
/// phrase without removing it).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ForbiddenPhrase {
    pub phrase: String,
    /// 0.0–1.0; drives `Severity::from_confidence` mapping.
    #[serde(default = "default_phrase_confidence")]
    pub confidence: f32,
}

fn default_phrase_confidence() -> f32 {
    0.75
}

// ot: allow DC002 reason="documentation deliberately quotes the residue phrases it filters on" expires="2099-01-01"
/// Seeded forbidden-phrase list — high-signal LLM-transcript residue and
/// stale planning markers. Confidences chosen per
/// `docs/PARADIGMS.md` §"Paradigm 17" so that the strongest signals
/// (the `the prompt` / `per the prompt` family) fire as `Fatal` regardless
/// of `--agent-strict`, mid-tier signals (the `as discussed` /
/// `from the previous version` family) fire `Fatal` at 0.90/0.85, and the
/// remaining markers (the `for now` / TODO family) sit in the 0.70 Warning
/// band that elevates to `Fatal` under agent-strict.
pub fn default_forbidden_doc_phrases() -> Vec<ForbiddenPhrase> {
    vec![
        ForbiddenPhrase {
            phrase: "as discussed".into(),
            confidence: 0.90,
        },
        ForbiddenPhrase {
            phrase: "as we discussed".into(),
            confidence: 0.90,
        },
        ForbiddenPhrase {
            phrase: "the prompt".into(),
            confidence: 0.95,
        },
        ForbiddenPhrase {
            phrase: "per the prompt".into(),
            confidence: 0.95,
        },
        ForbiddenPhrase {
            phrase: "previously".into(),
            confidence: 0.85,
        },
        ForbiddenPhrase {
            phrase: "mentioned earlier".into(),
            confidence: 0.85,
        },
        ForbiddenPhrase {
            phrase: "edge case above".into(),
            confidence: 0.80,
        },
        ForbiddenPhrase {
            phrase: "the user wanted".into(),
            confidence: 0.85,
        },
        ForbiddenPhrase {
            phrase: "this should fix".into(),
            confidence: 0.80,
        },
        ForbiddenPhrase {
            phrase: "because of the issue".into(),
            confidence: 0.75,
        },
        ForbiddenPhrase {
            phrase: "new approach".into(),
            confidence: 0.70,
        },
        ForbiddenPhrase {
            phrase: "old approach".into(),
            confidence: 0.75,
        },
        ForbiddenPhrase {
            phrase: "from the previous version".into(),
            confidence: 0.85,
        },
        ForbiddenPhrase {
            phrase: "for now".into(),
            confidence: 0.75,
        },
        ForbiddenPhrase {
            phrase: "later".into(),
            confidence: 0.65,
        },
        ForbiddenPhrase {
            phrase: "temporary".into(),
            confidence: 0.75,
        },
        ForbiddenPhrase {
            phrase: "clean this up".into(),
            confidence: 0.75,
        },
        ForbiddenPhrase {
            phrase: "TODO".into(),
            confidence: 0.70,
        },
        ForbiddenPhrase {
            phrase: "FIXME".into(),
            confidence: 0.80,
        },
        ForbiddenPhrase {
            phrase: "HACK".into(),
            confidence: 0.85,
        },
    ]
}

/// Pattern syntax: simple suffix wildcard, mirroring UT/DG.
/// - `foo::bar` — exact match
/// - `foo::*` — `foo` itself or any descendant (`foo::bar`, `foo::bar::baz`)
/// - `*` — anything
///
/// Duplicated locally rather than shared with UT to keep paradigm slices
/// independent — each paradigm owns its lockfile shape and helpers.
pub fn matches_pattern(pattern: &str, path: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix("::*") {
        return path == prefix || path.starts_with(&format!("{prefix}::"));
    }
    pattern == path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_match() {
        assert!(matches_pattern("foo::bar", "foo::bar"));
        assert!(!matches_pattern("foo::bar", "foo::bar::baz"));
        assert!(!matches_pattern("foo::bar", "foo"));
    }

    #[test]
    fn suffix_wildcard_includes_the_prefix_and_descendants() {
        assert!(matches_pattern("foo::*", "foo"));
        assert!(matches_pattern("foo::*", "foo::bar"));
        assert!(matches_pattern("foo::*", "foo::bar::baz"));
        assert!(!matches_pattern("foo::*", "foobar"));
        assert!(!matches_pattern("foo::*", "bar"));
    }

    #[test]
    fn star_matches_anything() {
        assert!(matches_pattern("*", ""));
        assert!(matches_pattern("*", "anything"));
        assert!(matches_pattern("*", "anything::nested"));
    }
}
