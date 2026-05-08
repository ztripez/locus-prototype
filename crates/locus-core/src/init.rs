//! Init-time onboarding suggestions.
//!
//! Each paradigm's `init.rs` emits zero or more [`Suggestion`]s; cross-
//! paradigm helpers in this module emit `Suggestion`s for shared questions
//! (layer detection, feature partitioning). The CLI's `init` handler
//! aggregates both lists, sorts and de-duplicates them, and prints the
//! result as a checklist.
//!
//! Suggestions are *not* fired as `Diagnostic`s — they are init-only and
//! never affect the rule engine's pass/fail.

// ot: canonical

use crate::lockfile::Lockfile;
use locus_air::AirWorkspace;
use std::cmp::Ordering;

/// Cross-paradigm suggestions (layer detection, feature partitioning, …).
/// Phase 1 returns no suggestions; phases 2 and 4 populate it.
pub fn cross_paradigm_suggestions(_air: &AirWorkspace, _lockfile: &Lockfile) -> Vec<Suggestion> {
    Vec::new()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub category: SuggestionCategory,
    pub headline: String,
    pub why: Vec<String>,
    pub options: Vec<CommandOption>,
    /// Paradigm prefixes this suggestion is associated with. Used by the
    /// aggregator to merge `why` lines when two paradigms emit the same
    /// suggestion shape.
    pub prefixes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SuggestionCategory {
    Concept,
    Layer,
    Feature,
    Threshold,
    Switch,
    ParadigmVacant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOption {
    pub label: String,
    pub commands: Vec<String>,
}

impl Suggestion {
    /// Render this suggestion as a human-readable block (no leading or
    /// trailing newline).
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("[{}] {}", self.category.tag(), self.headline));
        for w in &self.why {
            out.push('\n');
            out.push_str("  ");
            out.push_str(w);
        }
        for opt in &self.options {
            out.push('\n');
            out.push_str("  ");
            out.push_str(&opt.label);
            out.push(':');
            for cmd in &opt.commands {
                out.push('\n');
                out.push_str("    ");
                out.push_str(cmd);
            }
        }
        out
    }
}

impl SuggestionCategory {
    pub fn tag(self) -> &'static str {
        match self {
            SuggestionCategory::Concept => "concept",
            SuggestionCategory::Layer => "layer",
            SuggestionCategory::Feature => "feature",
            SuggestionCategory::Threshold => "threshold",
            SuggestionCategory::Switch => "switch",
            SuggestionCategory::ParadigmVacant => "paradigm-vacant",
        }
    }
}

/// Collect suggestions from many sources, sort them into a stable order,
/// and merge duplicates (suggestions with byte-identical option-command
/// lists). Merging combines `prefixes` and `why` lines.
pub fn aggregate(mut suggestions: Vec<Suggestion>) -> Vec<Suggestion> {
    suggestions.sort_by(suggestion_order);
    let mut out: Vec<Suggestion> = Vec::with_capacity(suggestions.len());
    for s in suggestions {
        let key = command_signature(&s);
        if let Some(existing) = out.iter_mut().find(|e| command_signature(e) == key) {
            for p in s.prefixes {
                if !existing.prefixes.iter().any(|q| q == &p) {
                    existing.prefixes.push(p);
                }
            }
            for w in s.why {
                if !existing.why.iter().any(|q| q == &w) {
                    existing.why.push(w);
                }
            }
        } else {
            out.push(s);
        }
    }
    out
}

fn suggestion_order(a: &Suggestion, b: &Suggestion) -> Ordering {
    a.category
        .cmp(&b.category)
        .then_with(|| a.headline.cmp(&b.headline))
}

fn command_signature(s: &Suggestion) -> Vec<Vec<String>> {
    s.options.iter().map(|o| o.commands.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_layer_suggestion() {
        let s = Suggestion {
            category: SuggestionCategory::Layer,
            headline: "no domain layer detected".into(),
            why: vec!["required by BO, FL".into()],
            options: vec![
                CommandOption {
                    label: "specify".into(),
                    commands: vec![
                        "locus bo add-domain-path \"crate::domain::*\"".into(),
                        "locus fl add-domain-path \"crate::domain::*\"".into(),
                    ],
                },
                CommandOption {
                    label: "or skip".into(),
                    commands: vec!["locus init --acknowledge-empty BO,FL".into()],
                },
            ],
            prefixes: vec!["BO".into(), "FL".into()],
        };
        let expected = "\
[layer] no domain layer detected
  required by BO, FL
  specify:
    locus bo add-domain-path \"crate::domain::*\"
    locus fl add-domain-path \"crate::domain::*\"
  or skip:
    locus init --acknowledge-empty BO,FL";
        assert_eq!(s.render(), expected);
    }
}

#[cfg(test)]
mod aggregate_tests {
    use super::*;

    fn mk(category: SuggestionCategory, headline: &str, prefix: &str, cmds: &[&str]) -> Suggestion {
        Suggestion {
            category,
            headline: headline.into(),
            why: vec![format!("from {prefix}")],
            options: vec![CommandOption {
                label: "specify".into(),
                commands: cmds.iter().map(|c| (*c).to_string()).collect(),
            }],
            prefixes: vec![prefix.into()],
        }
    }

    #[test]
    fn aggregate_merges_identical_command_sets() {
        let a = mk(
            SuggestionCategory::Layer,
            "no domain",
            "BO",
            &["locus xx add"],
        );
        let b = mk(
            SuggestionCategory::Layer,
            "no domain",
            "FL",
            &["locus xx add"],
        );
        let out = aggregate(vec![a, b]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].prefixes, vec!["BO", "FL"]);
        assert_eq!(out[0].why, vec!["from BO", "from FL"]);
    }

    #[test]
    fn aggregate_keeps_distinct_command_sets() {
        let a = mk(
            SuggestionCategory::Layer,
            "no domain",
            "BO",
            &["locus bo add"],
        );
        let b = mk(
            SuggestionCategory::Layer,
            "no domain",
            "FL",
            &["locus fl add"],
        );
        let out = aggregate(vec![a, b]);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn aggregate_sorts_by_category_then_headline() {
        let a = mk(SuggestionCategory::ParadigmVacant, "RW empty", "RW", &["a"]);
        let b = mk(SuggestionCategory::Layer, "no domain", "BO", &["b"]);
        let c = mk(SuggestionCategory::Concept, "user cluster", "OT", &["c"]);
        let out = aggregate(vec![a, b, c]);
        assert_eq!(out[0].category, SuggestionCategory::Concept);
        assert_eq!(out[1].category, SuggestionCategory::Layer);
        assert_eq!(out[2].category, SuggestionCategory::ParadigmVacant);
    }
}
