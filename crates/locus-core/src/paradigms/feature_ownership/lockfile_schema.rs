//! Shape of the FO section inside `locus.lock`.
//!
//! Rules family FO (Feature Ownership): records the named feature regions of
//! the workspace so FO rules can detect domain concepts being defined inside
//! more than one feature. The shape mirrors DG's `FeatureDefinition` minus
//! `public_api` — FO only cares about *where* a feature lives, not what it
//! exposes. The duplicate is intentional: paradigms shouldn't depend on each
//! other's lockfile schemas.

// ot: canonical

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct FoSection {
    /// Named feature regions of the workspace. FO001 fires when the same
    /// public type name is defined in two different features. An empty list
    /// keeps every FO rule silent.
    #[serde(default)]
    pub features: Vec<FoFeature>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FoFeature {
    /// Human-readable feature name (`"billing"`, `"identity"`, …).
    pub name: String,
    /// Module pattern matching every path that *belongs* to this feature.
    /// e.g. `"lore_engine_billing::*"` or `"crate::billing::*"`.
    pub module: String,
}

/// Pattern syntax: simple suffix wildcard.
/// - `foo::bar` — exact match
/// - `foo::*` — `foo` itself or any descendant (`foo::bar`, `foo::bar::baz`)
/// - `*` — anything
///
/// Duplicated from DG's `matches_pattern` on purpose: paradigm crates own
/// their lockfile schemas and shouldn't reach into one another. If a third
/// paradigm needs the same helper, lift it into a shared utility then; not
/// before.
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
