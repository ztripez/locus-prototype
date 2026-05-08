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

    /// Markers that, when present in a public item's doc text without an
    /// immediate parenthesised owner reference (e.g. `TODO(alice):` or
    /// `FIXME(#123):`), fire DC004. Defaults to a high-signal seed list of
    /// the four canonical "needs follow-up" markers (`TODO`, `FIXME`,
    /// `HACK`, `XXX`). Clearing the list opts out of DC004 entirely —
    /// DC004 stays silent when this is empty.
    #[serde(default = "default_unowned_marker_patterns")]
    pub unowned_marker_patterns: Vec<String>,
}

impl Default for DcSection {
    fn default() -> Self {
        Self {
            require_public_docs: false,
            exempt_paths: Vec::new(),
            forbidden_doc_phrases: default_forbidden_doc_phrases(),
            unowned_marker_patterns: default_unowned_marker_patterns(),
        }
    }
}

// ot: allow DC002 reason="doc deliberately quotes the marker patterns it filters on" expires="2099-01-01"
/// Seeded marker list for DC004 — the canonical "needs follow-up" markers
/// that should always carry a parenthesised owner reference (e.g.
/// `mark(alice):`, `mark(#123):` — see DC004 for the actual marker text).
/// An owner-less marker is a stale reminder with no path to resolution.
pub fn default_unowned_marker_patterns() -> Vec<String> {
    vec!["TODO".into(), "FIXME".into(), "HACK".into(), "XXX".into()]
}

// ot: allow DC002 reason="documentation deliberately quotes residue phrases for the alias-matching example" expires="2099-01-01"
/// One entry in the DC002 forbidden-phrase list. Matched case-insensitively
/// as a substring of an item's doc text. `confidence` drives
/// [`crate::diagnostics::Severity::from_confidence`] — values below `0.50`
/// suppress the diagnostic entirely (intentional, so users can demote a
/// phrase without removing it).
///
/// **Paraphrase coverage via `aliases`.** The same residue intent often
/// surfaces under multiple phrasings (`as discussed` / `as we discussed`
/// / `as I mentioned` / `we agreed`). Rather than introduce an embedding
/// model — which would break the no-LLM determinism rule — DC002 takes a
/// **deterministic alias list** approach: each `ForbiddenPhrase` carries
/// hand-curated equivalent phrasings that all fire under the same
/// confidence and concept. The matcher tries each alias the same way as
/// the primary phrase (case-insensitive substring); when an alias hits,
/// it's surfaced in the diagnostic message so the user sees exactly
/// which text was matched. No stemming, no regex — just a curated list,
/// auditable and reproducible.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ForbiddenPhrase {
    pub phrase: String,
    /// 0.0–1.0; drives `Severity::from_confidence` mapping.
    #[serde(default = "default_phrase_confidence")]
    pub confidence: f32,
    /// Equivalent phrasings — when any matches, the diagnostic surfaces
    /// the matched alias. Empty by default; the seeded high-value
    /// phrases ship with curated alias sets.
    #[serde(default)]
    pub aliases: Vec<String>,
}

fn default_phrase_confidence() -> f32 {
    0.75
}

// ot: allow DC002 reason="documentation deliberately quotes the residue phrases it filters on" expires="2099-01-01"
// ot: allow DC004 reason="docstring deliberately quotes the bare marker family DC004 fires on" expires="2099-01-01"
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
            aliases: vec![
                "as we discussed".into(),
                "as we've discussed".into(),
                "as i discussed".into(),
                "as we mentioned".into(),
                "as i mentioned".into(),
                "we discussed".into(),
                "we've discussed".into(),
                "we agreed".into(),
                "as we agreed".into(),
            ],
        },
        ForbiddenPhrase {
            phrase: "the prompt".into(),
            confidence: 0.95,
            aliases: vec![
                "per the prompt".into(),
                "in the prompt".into(),
                "from the prompt".into(),
                "the original prompt".into(),
                "your prompt".into(),
            ],
        },
        ForbiddenPhrase {
            phrase: "previously".into(),
            confidence: 0.85,
            aliases: vec![
                "previously discussed".into(),
                "previously mentioned".into(),
                "as previously".into(),
            ],
        },
        ForbiddenPhrase {
            phrase: "mentioned earlier".into(),
            confidence: 0.85,
            aliases: vec![
                "mentioned above".into(),
                "noted earlier".into(),
                "noted above".into(),
                "discussed earlier".into(),
                "discussed above".into(),
                "as mentioned".into(),
                "as noted".into(),
            ],
        },
        ForbiddenPhrase {
            phrase: "edge case above".into(),
            confidence: 0.80,
            aliases: vec![
                "edge case mentioned".into(),
                "edge case noted earlier".into(),
            ],
        },
        ForbiddenPhrase {
            phrase: "the user wanted".into(),
            confidence: 0.85,
            aliases: vec![
                "the user requested".into(),
                "you wanted".into(),
                "you requested".into(),
                "you asked for".into(),
                "as the user said".into(),
            ],
        },
        ForbiddenPhrase {
            phrase: "this should fix".into(),
            confidence: 0.80,
            aliases: vec![
                "this should resolve".into(),
                "this should address".into(),
                "this should handle".into(),
                "this fixes".into(),
            ],
        },
        ForbiddenPhrase {
            phrase: "because of the issue".into(),
            confidence: 0.75,
            aliases: vec![
                "due to the issue".into(),
                "to handle the issue".into(),
                "to fix the issue".into(),
                "to address the issue".into(),
                "because of this issue".into(),
            ],
        },
        ForbiddenPhrase {
            phrase: "new approach".into(),
            confidence: 0.70,
            aliases: vec![
                "my new approach".into(),
                "the new approach".into(),
                "the new way".into(),
                "this new approach".into(),
            ],
        },
        ForbiddenPhrase {
            phrase: "old approach".into(),
            confidence: 0.75,
            aliases: vec![
                "the old approach".into(),
                "the old way".into(),
                "the previous approach".into(),
                "old code".into(),
                "legacy approach".into(),
            ],
        },
        ForbiddenPhrase {
            phrase: "from the previous version".into(),
            confidence: 0.85,
            aliases: vec![
                "from the prior version".into(),
                "from before".into(),
                "from the earlier version".into(),
                "before the refactor".into(),
            ],
        },
        ForbiddenPhrase {
            phrase: "for now".into(),
            confidence: 0.75,
            aliases: vec![
                "for the time being".into(),
                "for the moment".into(),
                "until later".into(),
                "as a stopgap".into(),
            ],
        },
        ForbiddenPhrase {
            phrase: "later".into(),
            confidence: 0.65,
            aliases: vec!["in a later iteration".into(), "in a later pass".into()],
        },
        ForbiddenPhrase {
            phrase: "temporary".into(),
            confidence: 0.75,
            aliases: vec![
                "temp solution".into(),
                "stopgap".into(),
                "placeholder".into(),
            ],
        },
        ForbiddenPhrase {
            phrase: "clean this up".into(),
            confidence: 0.75,
            aliases: vec![
                "clean up later".into(),
                "cleanup needed".into(),
                "needs cleanup".into(),
                "needs a cleanup".into(),
                "should be cleaned up".into(),
            ],
        },
        ForbiddenPhrase {
            phrase: "TODO".into(),
            confidence: 0.70,
            aliases: vec![],
        },
        ForbiddenPhrase {
            phrase: "FIXME".into(),
            confidence: 0.80,
            aliases: vec![],
        },
        ForbiddenPhrase {
            phrase: "HACK".into(),
            confidence: 0.85,
            aliases: vec![],
        },
        ForbiddenPhrase {
            phrase: "XXX".into(),
            confidence: 0.80,
            aliases: vec![],
        },
        ForbiddenPhrase {
            phrase: "let me know".into(),
            confidence: 0.85,
            aliases: vec![
                "tell me if".into(),
                "let me know if".into(),
                "if you want".into(),
            ],
        },
        ForbiddenPhrase {
            phrase: "i think".into(),
            confidence: 0.65,
            aliases: vec![
                "i believe".into(),
                "i suspect".into(),
                "in my opinion".into(),
                "imho".into(),
            ],
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
