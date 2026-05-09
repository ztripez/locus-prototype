//! Shape of the FL section inside `locus.lock`.
//!
//! Rules family FL (Failure Lineage Ownership): the lockfile records two
//! lists of patterns that together describe where transport-level failures
//! must not leak.
//!
//! - `domain_paths` — module patterns marking files whose function signatures
//!   must speak the domain's error vocabulary.
//! - `boundary_error_patterns` — patterns matching error type names that are
//!   transport / boundary level (e.g. `reqwest::Error`, `sqlx::Error`,
//!   `std::io::Error`). Encountering one of these as the `E` of a
//!   `Result<T, E>` returned from a domain-path function is a structural
//!   failure-lineage violation: the boundary error escaped without being
//!   wrapped in a domain error type.
//!
//! Both lists default to empty and FL001 stays silent until the user has
//! onboarded their codebase — same UX shape as DG / UT lockfile-driven rules.

// locus: ot canonical

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlSection {
    /// Module patterns matching `AirFile.module_path` for files declared as
    /// "domain" — i.e. files whose function signatures must not leak boundary
    /// error types. Pattern syntax mirrors UT/DG: simple suffix wildcards.
    #[serde(default)]
    pub domain_paths: Vec<String>,

    /// Patterns matching the `E` in a function's `Result<T, E>` return type
    /// when E is a transport / boundary error and therefore must not appear
    /// in a domain function signature. Pattern syntax mirrors `domain_paths`.
    #[serde(default)]
    pub boundary_error_patterns: Vec<String>,

    /// Callee names considered "panic-shaped" — i.e. they mask a missing
    /// invariant rather than propagate a structured error. FL002 matches
    /// each `AirItem::CallSite.callee` (last `::` segment for path-qualified
    /// macros) against these patterns. Default covers the standard
    /// agent-introduced "make it compile" family: `unwrap`, `expect`,
    /// `unwrap_or_default`, `panic`, `todo`, `unimplemented`. The user can
    /// tighten or loosen via the lockfile.
    #[serde(default = "default_forbidden_callees")]
    pub forbidden_callees: Vec<String>,

    /// Module patterns matching `AirFile.module_path` for files where the
    /// panic-shaped callees above are legitimate — typically supervisors,
    /// startup-asserting bin entry points, or test-support modules that
    /// own the invariant being asserted. Shared by FL002 (panic-shaped
    /// callees) and FL003 (silent-discard callees) — both rules stay
    /// silent until this list is populated, mirroring every other
    /// lockfile-driven rule.
    ///
    /// The spec (`docs/PARADIGMS.md` line 811: "panics/unwraps outside
    /// invariant owners *or tests*") expects test paths to be carved out.
    /// We can't auto-detect `#[cfg(test)]` from AIR — `AirFunction` /
    /// `AirFile` don't carry attribute state — so test-path patterns are a
    /// user lockfile decision. Recommended starter set when populating:
    /// `["*::tests::*", "*::test::*", "tests::*", "tests::*::*"]` plus any
    /// project-specific invariant-owner modules. We deliberately don't seed
    /// these defaults here because a non-empty seed would flip FL002/FL003
    /// from "silent until configured" to "fires on every codebase" — a
    /// posture the rest of Locus avoids.
    #[serde(default)]
    pub invariant_owner_paths: Vec<String>,

    /// Method-call callees considered "silent error discard" — they convert
    /// a `Result` into a value-or-default *without* propagating the error.
    /// FL003 matches each `AirItem::CallSite` with `kind == Method` and
    /// `callee == <name>` against this list. Default covers the agent's
    /// classic silent-swallow pattern: `.ok()` on a `Result` (drops the
    /// error, returns `Option`), `.err()` (drops the success), and
    /// `.unwrap_or_else` when paired with a closure that ignores its
    /// argument (we can't see the closure body, so the conservative call
    /// is to flag it and let the user accept it via `// locus: allow FL003`).
    ///
    /// Note: bare-name matching means we'll see `.ok()` on `Option`-shaped
    /// receivers too. Most std types have no `.ok()` method — `Option`
    /// itself doesn't — so the false-positive surface is small. The
    /// receiver-type would be needed to be precise; receiver resolution is
    /// out of AIR's scope today, so the rule is intentionally conservative.
    #[serde(default = "default_silent_discard_callees")]
    pub silent_discard_callees: Vec<String>,

    /// Callees recorded on `AirItem::SilentDiscard` (`let _ = ...`) that are
    /// **legitimate** silent discards — FL004 skips a discard when its
    /// callee matches any pattern here. Default covers the canonical
    /// fire-and-forget patterns: `lock` (intentional drop guard), `send`
    /// (closed-channel error is recoverable), `drop` (explicit value
    /// drop), `set_logger` / `subscribe` (idempotent registrations that
    /// fail when called twice).
    ///
    /// Pattern syntax mirrors the other lists — exact match or trailing
    /// `::*` wildcard.
    #[serde(default = "default_silent_discard_allowed_callees")]
    pub silent_discard_allowed_callees: Vec<String>,

    /// Module patterns matching `AirFile.module_path` (or a function
    /// symbol's containing module) for files that legitimately implement
    /// retry policies — modules that own backoff, max-attempts, jitter,
    /// or other declared retry semantics. FL012 stays silent until this
    /// list is populated; once populated, every retry-shaped loop
    /// (`AirItem::RetryLoop` with `propagates: true` and `has_break:
    /// true`) outside the listed paths is flagged as an ad-hoc retry
    /// without an accepted policy.
    ///
    /// Pattern syntax mirrors `invariant_owner_paths`. Recommended
    /// starter when populating: `["*::retry::*", "*::backoff::*"]` plus
    /// any project-specific retry-policy modules.
    #[serde(default)]
    pub retry_policy_owner_paths: Vec<String>,
}

impl Default for FlSection {
    fn default() -> Self {
        Self {
            domain_paths: Vec::new(),
            boundary_error_patterns: Vec::new(),
            forbidden_callees: default_forbidden_callees(),
            invariant_owner_paths: Vec::new(),
            silent_discard_callees: default_silent_discard_callees(),
            silent_discard_allowed_callees: default_silent_discard_allowed_callees(),
            retry_policy_owner_paths: Vec::new(),
        }
    }
}

impl FlSection {
    /// True when none of the user-declarative lists are populated. FL002
    /// (panic-shaped) / FL003 (silent discard) / FL004 (`let _ =`) /
    /// FL005-007 / FL011 / FL013 all fire on seeded callee patterns but
    /// rely on `invariant_owner_paths` to carve out tests and other
    /// invariant-owner modules — without it, every test's `unwrap()`
    /// trips FL002. FL001 needs `domain_paths` + `boundary_error_patterns`,
    /// FL012 needs `retry_policy_owner_paths`. The vacancy diagnostic
    /// nudges users to populate at least the `invariant_owner_paths`
    /// list so the noise becomes targeted.
    pub fn is_vacant(&self) -> bool {
        self.domain_paths.is_empty()
            && self.boundary_error_patterns.is_empty()
            && self.invariant_owner_paths.is_empty()
            && self.retry_policy_owner_paths.is_empty()
    }
}

/// Default forbidden callees for FL002: the standard agent-introduced
/// "make it compile by unwrapping" family. Matched against
/// `AirCallSite.callee` (last `::` segment for path-qualified macros), so
/// these are bare names — no `std::` prefix.
pub fn default_forbidden_callees() -> Vec<String> {
    vec![
        "unwrap".to_string(),
        "expect".to_string(),
        "unwrap_or_default".to_string(),
        "panic".to_string(),
        "todo".to_string(),
        "unimplemented".to_string(),
    ]
}

/// Default silent-discard callees for FL003: the agent's "make the error
/// go away without propagating it" family. Matched against
/// `AirCallSite.callee` for method calls only (`CallKind::Method`).
/// Spec: `docs/PARADIGMS.md` line 804–807 (".ok() / unwrap_or_default
/// masking, etc.").
pub fn default_silent_discard_callees() -> Vec<String> {
    vec![
        "ok".to_string(),
        "err".to_string(),
        "unwrap_or_else".to_string(),
    ]
}

/// Default callee allowlist for FL004 (`let _ = expr;` silent-discard
/// bindings). These are the canonical idiomatic fire-and-forget patterns
/// where `let _ = …` is the *correct* shape:
///
/// - `lock` — `let _ = mutex.lock();` keeps the guard alive for the
///   surrounding block.
/// - `send` — closed-channel errors are recoverable on senders.
/// - `drop` — explicit value drop.
/// - `set_logger` / `subscribe` / `try_init` — idempotent registrations
///   whose "already initialised" error is benign.
///
/// FL004 matches the discarded callee against this list and skips the
/// diagnostic. Users who want a tighter posture can clear the list in
/// their lockfile.
pub fn default_silent_discard_allowed_callees() -> Vec<String> {
    vec![
        "lock".to_string(),
        "send".to_string(),
        "drop".to_string(),
        "set_logger".to_string(),
        "subscribe".to_string(),
        "try_init".to_string(),
    ]
}

/// Pattern syntax: segment-aligned wildcards.
/// - `foo::bar` — exact match
/// - `foo::*` — `foo` itself or any descendant (`foo::bar`, `foo::bar::baz`)
/// - `*::foo` — `foo` itself or anywhere ending with `::foo` (`a::foo`,
///   `a::b::foo`)
/// - `*::foo::*` — `foo` appearing as any whole segment in the path
///   (`foo`, `a::foo`, `a::foo::b`, `a::b::foo::c`)
/// - `*` — anything
///
/// FL's matcher is intentionally richer than the rest of the paradigm
/// matchers (which only handle the trailing `::*` shape) because FL's
/// `invariant_owner_paths` are typically test-module patterns
/// (`*::tests::*`) and inline `mod tests {}` blocks land on a wide
/// variety of module paths across a workspace. Other paradigms can lift
/// this implementation when they hit the same need.
///
/// Duplicated locally rather than imported from a sibling paradigm so
/// each paradigm's matcher can evolve independently.
pub fn matches_pattern(pattern: &str, path: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    let leading_wild = pattern.starts_with("*::");
    let trailing_wild = pattern.ends_with("::*");
    let stripped = match (leading_wild, trailing_wild) {
        (true, true) => &pattern[3..pattern.len() - 3],
        (true, false) => &pattern[3..],
        (false, true) => &pattern[..pattern.len() - 3],
        (false, false) => pattern,
    };
    if stripped.is_empty() {
        // Pattern was just `*::` or `::*` — treat as a malformed
        // wildcard rather than matching anything; callers configuring
        // these would have meant `*`.
        return false;
    }
    match (leading_wild, trailing_wild) {
        (true, true) => {
            let mid = format!("::{stripped}::");
            let starts = format!("{stripped}::");
            let ends = format!("::{stripped}");
            path == stripped
                || path.contains(&mid)
                || path.starts_with(&starts)
                || path.ends_with(&ends)
        }
        (true, false) => path == stripped || path.ends_with(&format!("::{stripped}")),
        (false, true) => path == stripped || path.starts_with(&format!("{stripped}::")),
        (false, false) => pattern == path,
    }
}

/// Return the containing-module path for a function symbol — i.e. the
/// symbol with its last `::`-segment dropped. `a::b::tests::f` →
/// `a::b::tests`. Bare names with no `::` return the original string.
///
/// Used by FL002–FL005 alongside the file's `module_path` to evaluate
/// the function's enclosing context: a function inside an inline
/// `mod tests {}` block has a file `module_path` that doesn't include
/// `::tests::`, but its symbol does. Without this, `*::tests::*`
/// patterns silently miss inline test modules.
pub fn containing_module_of(function_symbol: &str) -> &str {
    function_symbol
        .rsplit_once("::")
        .map(|(prefix, _)| prefix)
        .unwrap_or(function_symbol)
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

    #[test]
    fn leading_wildcard_matches_any_ending() {
        assert!(matches_pattern("*::tests", "a::b::tests"));
        assert!(matches_pattern("*::tests", "tests"));
        assert!(matches_pattern("*::tests", "a::tests"));
        assert!(!matches_pattern("*::tests", "a::tests::b"));
        assert!(!matches_pattern("*::tests", "tester")); // not segment-aligned
    }

    #[test]
    fn segment_anywhere_wildcard_matches_inline_test_modules() {
        // The headline use case: `*::tests::*` should fire on any
        // function symbol or containing-module path that has `tests`
        // as a segment somewhere in the middle.
        assert!(matches_pattern("*::tests::*", "tests"));
        assert!(matches_pattern("*::tests::*", "a::tests"));
        assert!(matches_pattern("*::tests::*", "tests::nested"));
        assert!(matches_pattern("*::tests::*", "a::b::tests"));
        assert!(matches_pattern("*::tests::*", "a::b::tests::f"));
        assert!(matches_pattern("*::tests::*", "a::tests::b::c"));
        assert!(!matches_pattern("*::tests::*", "tester::hat"));
        assert!(!matches_pattern("*::tests::*", "a::testimony"));
    }

    #[test]
    fn malformed_bare_wildcard_does_not_match_anything() {
        // `*::` and `::*` alone with no body shouldn't quietly match
        // every path — that's what `*` is for.
        assert!(!matches_pattern("*::", "anything"));
        assert!(!matches_pattern("::*", "anything"));
    }

    #[test]
    fn containing_module_drops_last_segment() {
        assert_eq!(
            containing_module_of("a::b::tests::sigmoid_extreme"),
            "a::b::tests"
        );
        assert_eq!(containing_module_of("crate::User"), "crate");
        // Bare name without `::` is its own containing module — the
        // caller should still match it against patterns directly.
        assert_eq!(containing_module_of("standalone"), "standalone");
    }

    #[test]
    fn default_section_seeds_forbidden_callees_and_keeps_owner_paths_empty() {
        let s = FlSection::default();
        assert!(s.domain_paths.is_empty());
        assert!(s.boundary_error_patterns.is_empty());
        assert!(s.invariant_owner_paths.is_empty());
        for expected in [
            "unwrap",
            "expect",
            "unwrap_or_default",
            "panic",
            "todo",
            "unimplemented",
        ] {
            assert!(
                s.forbidden_callees.iter().any(|c| c == expected),
                "default forbidden callees missing `{expected}`: {:?}",
                s.forbidden_callees,
            );
        }
        for expected in ["ok", "err", "unwrap_or_else"] {
            assert!(
                s.silent_discard_callees.iter().any(|c| c == expected),
                "default silent-discard callees missing `{expected}`: {:?}",
                s.silent_discard_callees,
            );
        }
        for expected in [
            "lock",
            "send",
            "drop",
            "set_logger",
            "subscribe",
            "try_init",
        ] {
            assert!(
                s.silent_discard_allowed_callees
                    .iter()
                    .any(|c| c == expected),
                "default allowed-discard callees missing `{expected}`: {:?}",
                s.silent_discard_allowed_callees,
            );
        }
    }

    #[test]
    fn boundary_error_patterns_can_target_type_paths() {
        // Pattern syntax is path-shaped, not Rust-namespaced — the rule will
        // run it against the rendered error type string from
        // `AirFunction.return_type`.
        assert!(matches_pattern("reqwest::Error", "reqwest::Error"));
        assert!(matches_pattern("reqwest::*", "reqwest::Error"));
        assert!(matches_pattern(
            "reqwest::*",
            "reqwest::header::InvalidHeader"
        ));
        assert!(!matches_pattern("reqwest::*", "sqlx::Error"));
    }
}
