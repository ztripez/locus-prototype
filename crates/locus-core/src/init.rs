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
pub fn cross_paradigm_suggestions(air: &AirWorkspace, lockfile: &Lockfile) -> Vec<Suggestion> {
    let layers = detect_layers(air);
    let mut out = Vec::new();
    if !layers.domain.is_empty() && !any_domain_paths_set(lockfile) {
        out.push(domain_layer_suggestion(&layers.domain));
    }
    out
}

fn any_domain_paths_set(lockfile: &Lockfile) -> bool {
    let bo: serde_json::Value = lockfile
        .paradigm_section("BO")
        .unwrap_or(serde_json::Value::Null);
    let er: serde_json::Value = lockfile
        .paradigm_section("ER")
        .unwrap_or(serde_json::Value::Null);
    let fl: serde_json::Value = lockfile
        .paradigm_section("FL")
        .unwrap_or(serde_json::Value::Null);
    let rm: serde_json::Value = lockfile
        .paradigm_section("RM")
        .unwrap_or(serde_json::Value::Null);
    has_nonempty_array(&bo, "domain_paths")
        || has_nonempty_array(&er, "domain_paths")
        || has_nonempty_array(&fl, "domain_paths")
        || has_nonempty_array(&rm, "domain_paths_rm")
}

fn has_nonempty_array(v: &serde_json::Value, key: &str) -> bool {
    v.get(key)
        .and_then(|a| a.as_array())
        .is_some_and(|a| !a.is_empty())
}

fn domain_layer_suggestion(globs: &[String]) -> Suggestion {
    let mut commands: Vec<String> = Vec::new();
    for g in globs {
        commands.push(format!("locus bo add-domain-path \"{g}\""));
        commands.push(format!("locus fl add-domain-path \"{g}\""));
        commands.push(format!("locus er add-domain-path \"{g}\""));
        commands.push(format!("locus rm add-domain-path \"{g}\""));
    }
    Suggestion {
        category: SuggestionCategory::Layer,
        headline: "domain layer detected, but no paradigms onboarded".into(),
        why: vec![
            "required by BO, ER, FL, RM".into(),
            format!("globs: {}", globs.join(", ")),
        ],
        options: vec![
            CommandOption {
                label: "specify (run for each paradigm you want to onboard)".into(),
                commands,
            },
            CommandOption {
                label: "or skip these paradigms".into(),
                commands: vec!["locus init --acknowledge-empty BO,ER,FL,RM".into()],
            },
        ],
        prefixes: vec!["BO".into(), "ER".into(), "FL".into(), "RM".into()],
    }
}

/// A set of module-path globs grouped by detected architectural layer. The
/// globs are returned in `<module>::*` form so paradigm setters can use
/// them verbatim.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DetectedLayers {
    pub domain: Vec<String>,
    pub api_or_boundary: Vec<String>,
    pub application: Vec<String>,
    pub composition: Vec<String>,
    pub tests: Vec<String>,
    pub utilities: Vec<String>,
    pub config: Vec<String>,
}

pub fn detect_layers(air: &AirWorkspace) -> DetectedLayers {
    use std::collections::BTreeSet;

    let mut domain: BTreeSet<String> = BTreeSet::new();
    let mut api: BTreeSet<String> = BTreeSet::new();
    let mut application: BTreeSet<String> = BTreeSet::new();
    let mut composition: BTreeSet<String> = BTreeSet::new();
    let mut tests: BTreeSet<String> = BTreeSet::new();
    let mut utilities: BTreeSet<String> = BTreeSet::new();
    let mut config: BTreeSet<String> = BTreeSet::new();

    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module) = file.module_path.as_deref() else {
                continue;
            };
            for seg in module.split("::") {
                match seg {
                    "domain" | "core" | "model" | "models" => {
                        domain.insert(layer_glob(module, seg));
                    }
                    "api" | "dto" | "dtos" | "transport" => {
                        api.insert(layer_glob(module, seg));
                    }
                    "application" | "usecases" | "handlers" | "service" | "services" => {
                        application.insert(layer_glob(module, seg));
                    }
                    "composition" | "wiring" | "bin" | "main" => {
                        composition.insert(layer_glob(module, seg));
                    }
                    "tests" | "test_support" | "fixtures" => {
                        tests.insert(layer_glob(module, seg));
                    }
                    "util" | "utils" | "common" | "helpers" => {
                        utilities.insert(layer_glob(module, seg));
                    }
                    "config" | "settings" => {
                        config.insert(layer_glob(module, seg));
                    }
                    _ => {}
                }
            }
        }
    }

    DetectedLayers {
        domain: domain.into_iter().collect(),
        api_or_boundary: api.into_iter().collect(),
        application: application.into_iter().collect(),
        composition: composition.into_iter().collect(),
        tests: tests.into_iter().collect(),
        utilities: utilities.into_iter().collect(),
        config: config.into_iter().collect(),
    }
}

/// Produce the `<prefix>::<seg>::*` glob from a module path that contains
/// `<seg>` as one of its segments.
fn layer_glob(module: &str, seg: &str) -> String {
    let mut out = String::new();
    for (i, s) in module.split("::").enumerate() {
        if i > 0 {
            out.push_str("::");
        }
        out.push_str(s);
        if s == seg {
            out.push_str("::*");
            return out;
        }
    }
    format!("{module}::*")
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

#[cfg(test)]
mod layer_detection_tests {
    use super::*;
    use locus_air::{AirFile, AirPackage, AirWorkspace};

    fn pkg(name: &str, files: &[(&str, Option<&str>)]) -> AirPackage {
        AirPackage {
            name: name.into(),
            version: "0.0.1".into(),
            root_dir: format!("/tmp/{name}"),
            files: files
                .iter()
                .map(|(p, m)| AirFile {
                    path: (*p).into(),
                    module_path: m.map(|s| s.to_string()),
                    items: Vec::new(),
                    hints: Vec::new(),
                    parse_error: None,
                    line_count: 1,
                })
                .collect(),
        }
    }

    #[test]
    fn detects_domain_modules_by_segment() {
        let air = AirWorkspace::new(vec![pkg(
            "x",
            &[
                ("src/user/domain.rs", Some("x::user::domain")),
                ("src/user/api.rs", Some("x::user::api")),
            ],
        )]);
        let layers = detect_layers(&air);
        assert!(layers.domain.iter().any(|p| p == "x::user::domain::*"));
        assert!(
            layers
                .api_or_boundary
                .iter()
                .any(|p| p == "x::user::api::*")
        );
    }

    #[test]
    fn returns_empty_when_no_conventions_match() {
        let air = AirWorkspace::new(vec![pkg("x", &[("src/lib.rs", Some("x"))])]);
        let layers = detect_layers(&air);
        assert!(layers.domain.is_empty());
        assert!(layers.api_or_boundary.is_empty());
        assert!(layers.application.is_empty());
        assert!(layers.tests.is_empty());
        assert!(layers.utilities.is_empty());
        assert!(layers.config.is_empty());
        assert!(layers.composition.is_empty());
    }
}

#[cfg(test)]
mod cross_paradigm_layer_tests {
    use super::*;
    use crate::lockfile::Lockfile;
    use locus_air::{AirFile, AirPackage, AirWorkspace};

    fn workspace_with(modules: &[&str]) -> AirWorkspace {
        AirWorkspace::new(vec![AirPackage {
            name: "x".into(),
            version: "0.0.1".into(),
            root_dir: "/tmp/x".into(),
            files: modules
                .iter()
                .map(|m| AirFile {
                    path: format!("src/{}.rs", m.replace("::", "/")),
                    module_path: Some((*m).into()),
                    items: Vec::new(),
                    hints: Vec::new(),
                    parse_error: None,
                    line_count: 1,
                })
                .collect(),
        }])
    }

    #[test]
    fn emits_domain_suggestion_when_domain_modules_seen() {
        let air = workspace_with(&["x::user::domain", "x::user::api"]);
        let lf = Lockfile::empty();
        let suggestions = cross_paradigm_suggestions(&air, &lf);
        let domain = suggestions.iter().find(|s| {
            s.headline
                .contains("domain layer detected, but no paradigms onboarded")
        });
        assert!(domain.is_some(), "expected a domain-layer suggestion");
        let s = domain.unwrap();
        assert_eq!(s.category, SuggestionCategory::Layer);
        let cmds = s.options[0].commands.join("\n");
        assert!(cmds.contains("locus bo add-domain-path \"x::user::domain::*\""));
        assert!(cmds.contains("locus fl add-domain-path \"x::user::domain::*\""));
        assert!(cmds.contains("locus er add-domain-path \"x::user::domain::*\""));
        assert!(cmds.contains("locus rm add-domain-path \"x::user::domain::*\""));
    }

    #[test]
    fn omits_domain_suggestion_when_bo_already_has_a_path() {
        use serde_json::json;
        let air = workspace_with(&["x::user::domain"]);
        let mut lf = Lockfile::empty();
        lf.paradigms
            .insert("BO".into(), json!({"domain_paths": ["x::user::domain::*"]}));
        let suggestions = cross_paradigm_suggestions(&air, &lf);
        assert!(
            !suggestions
                .iter()
                .any(|s| s.headline.contains("domain layer detected")),
            "domain suggestion should suppress once BO has the path"
        );
    }
}

/// Distinct second-segment module names across the workspace
/// (`x::user::domain` → `"user"`). Excludes single-segment files (the
/// crate root). Returned alphabetically.
pub fn top_level_modules(air: &AirWorkspace) -> Vec<String> {
    use std::collections::BTreeSet;
    let mut names: BTreeSet<String> = BTreeSet::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module) = file.module_path.as_deref() else {
                continue;
            };
            let mut segs = module.split("::");
            let _root = segs.next();
            if let Some(second) = segs.next() {
                names.insert(second.to_string());
            }
        }
    }
    names.into_iter().collect()
}

#[cfg(test)]
mod top_level_module_tests {
    use super::*;
    use locus_air::{AirFile, AirPackage, AirWorkspace};

    fn ws(modules: &[&str]) -> AirWorkspace {
        AirWorkspace::new(vec![AirPackage {
            name: "x".into(),
            version: "0.0.1".into(),
            root_dir: "/tmp/x".into(),
            files: modules
                .iter()
                .map(|m| AirFile {
                    path: format!("src/{}.rs", m.replace("::", "/")),
                    module_path: Some((*m).into()),
                    items: Vec::new(),
                    hints: Vec::new(),
                    parse_error: None,
                    line_count: 1,
                })
                .collect(),
        }])
    }

    #[test]
    fn enumerates_distinct_first_segment_after_crate_root() {
        let air = ws(&[
            "x::user::domain",
            "x::user::api",
            "x::order::domain",
            "x::billing",
        ]);
        let modules = top_level_modules(&air);
        assert_eq!(
            modules,
            vec![
                "billing".to_string(),
                "order".to_string(),
                "user".to_string()
            ]
        );
    }

    #[test]
    fn ignores_crate_root_only_files() {
        let air = ws(&["x"]);
        assert!(top_level_modules(&air).is_empty());
    }
}
