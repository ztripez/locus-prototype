//! Init-time onboarding suggestions for DG.
//!
//! When DG features are declared but one or more carry an empty
//! `public_api`, DG003 fires per cross-feature import — a single missing
//! authority declaration becomes N diagnostics. This module collapses that
//! into one [`Suggestion`] per affected target feature: "feature X has N
//! cross-feature reaches; declare its public_api to allow the legitimate
//! ones". The cross-paradigm "no features defined at all" case is handled
//! separately by [`crate::init::cross_paradigm_suggestions`].
//!
//! Threshold for collapsing: **3+ reaches**. Below that, the per-import
//! DG003 diagnostics are concise enough on their own.

// locus: ot canonical

use crate::init::{CommandOption, Suggestion, SuggestionCategory};
use crate::lockfile::Lockfile;
use locus_air::{AirItem, AirWorkspace};
use std::collections::BTreeMap;

use super::DG_PREFIX;
use super::lockfile_schema::{DgSection, FeatureDefinition, matches_pattern};

/// Minimum cross-feature reach count before we collapse per-import DG003
/// diagnostics into one onboarding nudge.
const COLLAPSE_THRESHOLD: usize = 3;

pub fn suggest(air: &AirWorkspace, lockfile: &Lockfile) -> Vec<Suggestion> {
    let section: DgSection = lockfile.paradigm_section(DG_PREFIX).unwrap_or_default();
    if section.features.len() < 2 {
        // No grouped story to tell: cross-paradigm `feature_partition_suggestion`
        // handles the "no features at all" case; with one feature there is no
        // cross-feature edge to summarise.
        return Vec::new();
    }

    // Count cross-feature reaches per target feature whose `public_api` is
    // empty. We don't second-guess features that already declared a public
    // surface — a real DG003 there is a real internals reach.
    let mut reaches: BTreeMap<&str, usize> = BTreeMap::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            let Some(importer_feature) = owning_feature_by_module(&section.features, module_path)
            else {
                continue;
            };
            for item in &file.items {
                let AirItem::Import(imp) = item else {
                    continue;
                };
                let Some(target_feature) = owning_feature_by_module(&section.features, &imp.path)
                else {
                    continue;
                };
                if std::ptr::eq(importer_feature, target_feature) {
                    continue;
                }
                if !target_feature.public_api.is_empty() {
                    continue;
                }
                *reaches.entry(target_feature.name.as_str()).or_insert(0) += 1;
            }
        }
    }

    reaches
        .into_iter()
        .filter(|(_, count)| *count >= COLLAPSE_THRESHOLD)
        .map(|(target_name, count)| grouped_suggestion(&section, target_name, count))
        .collect()
}

fn grouped_suggestion(section: &DgSection, target_name: &str, count: usize) -> Suggestion {
    let target_feature = section
        .features
        .iter()
        .find(|f| f.name == target_name)
        .expect("target_name came from section.features iteration");
    let module_pattern = &target_feature.module;
    let api_seed = api_seed_from_module(module_pattern);
    let define_cmd = format!(
        "locus dg define-feature --name {target_name} --module \"{module_pattern}\" --public-api \"{api_seed}\" --force"
    );
    Suggestion {
        category: SuggestionCategory::Feature,
        headline: format!(
            "feature `{target_name}` has {count} cross-feature reaches but no `public_api`",
        ),
        why: vec![
            format!(
                "every cross-feature import targeting `{target_name}` becomes a DG003 internals \
                 reach until its public_api is declared",
            ),
            "this is one missing authority declaration, not N architecture bugs".into(),
        ],
        options: vec![
            CommandOption {
                label: "declare public_api (start narrow; widen as needed)".into(),
                commands: vec![define_cmd],
            },
            CommandOption {
                label: "or keep all internals open by widening the API".into(),
                commands: vec![format!(
                    "locus dg define-feature --name {target_name} --module \"{module_pattern}\" --public-api \"{module_pattern}\" --force",
                )],
            },
        ],
        prefixes: vec!["DG".into()],
    }
}

/// Best-effort starter `public_api` glob from a feature's `module` pattern.
/// `feature_one::*` → `feature_one::api::*`. If the pattern doesn't end in
/// `::*`, fall back to the pattern itself.
fn api_seed_from_module(module: &str) -> String {
    if let Some(stem) = module.strip_suffix("::*") {
        format!("{stem}::api::*")
    } else {
        module.to_string()
    }
}

fn owning_feature_by_module<'a>(
    features: &'a [FeatureDefinition],
    path: &str,
) -> Option<&'a FeatureDefinition> {
    features.iter().find(|f| matches_pattern(&f.module, path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use locus_air::{AirFile, AirImport, AirPackage, AirSpan, Visibility};

    fn import(path: &str) -> AirItem {
        AirItem::Import(AirImport {
            path: path.into(),
            path_segments: path.split("::").map(|s| s.to_string()).collect(),
            visibility: Visibility::Module,
            span: AirSpan::new("src/x.rs", 1, 1),
        })
    }

    fn file_with_imports(module: &str, imports: &[&str]) -> AirFile {
        AirFile {
            path: format!("src/{}.rs", module.replace("::", "/")),
            module_path: Some(module.into()),
            items: imports.iter().map(|p| import(p)).collect(),
            hints: Vec::new(),
            parse_error: None,
            line_count: 1,
        }
    }

    fn ws(files: Vec<AirFile>) -> AirWorkspace {
        AirWorkspace::new(vec![AirPackage {
            name: "x".into(),
            version: "0.0.1".into(),
            root_dir: "/tmp/x".into(),
            files,
        }])
    }

    fn dg_lockfile(features: serde_json::Value) -> Lockfile {
        let mut lf = Lockfile::empty();
        lf.paradigms.insert(
            DG_PREFIX.into(),
            serde_json::json!({ "features": features }),
        );
        lf
    }

    #[test]
    fn collapses_three_or_more_reaches_into_a_feature_with_empty_public_api() {
        let air = ws(vec![file_with_imports(
            "x::feature_two::handler",
            &[
                "x::feature_one::internals::a",
                "x::feature_one::internals::b",
                "x::feature_one::internals::c",
            ],
        )]);
        let lf = dg_lockfile(serde_json::json!([
            {"name": "feature_one", "module": "x::feature_one::*", "public_api": []},
            {"name": "feature_two", "module": "x::feature_two::*", "public_api": []},
        ]));
        let s = suggest(&air, &lf);
        // feature_two has 0 reaches; feature_one has 3 — only one suggestion.
        assert_eq!(s.len(), 1, "expected one grouped suggestion, got {s:#?}");
        assert!(s[0].headline.contains("feature_one"));
        assert!(s[0].headline.contains("3"));
        let cmd = s[0].options[0].commands[0].as_str();
        assert!(
            cmd.contains("--name feature_one")
                && cmd.contains("--public-api")
                && cmd.contains("--force"),
            "expected define-feature --force suggestion, got `{cmd}`"
        );
    }

    #[test]
    fn does_not_collapse_below_threshold() {
        let air = ws(vec![file_with_imports(
            "x::feature_two::handler",
            &[
                "x::feature_one::internals::a",
                "x::feature_one::internals::b",
            ],
        )]);
        let lf = dg_lockfile(serde_json::json!([
            {"name": "feature_one", "module": "x::feature_one::*", "public_api": []},
            {"name": "feature_two", "module": "x::feature_two::*", "public_api": []},
        ]));
        let s = suggest(&air, &lf);
        assert!(
            s.is_empty(),
            "2 reaches is below threshold; per-import DG003 should speak instead, got {s:#?}",
        );
    }

    #[test]
    fn skips_features_with_declared_public_api() {
        let air = ws(vec![file_with_imports(
            "x::feature_two::handler",
            &[
                "x::feature_one::internals::a",
                "x::feature_one::internals::b",
                "x::feature_one::internals::c",
            ],
        )]);
        let lf = dg_lockfile(serde_json::json!([
            {
                "name": "feature_one",
                "module": "x::feature_one::*",
                "public_api": ["x::feature_one::api::*"],
            },
            {"name": "feature_two", "module": "x::feature_two::*", "public_api": []},
        ]));
        let s = suggest(&air, &lf);
        assert!(
            s.is_empty(),
            "feature_one has a declared public_api — DG003 per-import is the right voice, \
             not a grouped onboarding nudge. got {s:#?}",
        );
    }

    #[test]
    fn ignores_intra_feature_imports() {
        // Three imports inside feature_one → these are intra-feature, not
        // cross-feature reaches.
        let air = ws(vec![file_with_imports(
            "x::feature_one::handler",
            &[
                "x::feature_one::internals::a",
                "x::feature_one::internals::b",
                "x::feature_one::internals::c",
            ],
        )]);
        let lf = dg_lockfile(serde_json::json!([
            {"name": "feature_one", "module": "x::feature_one::*", "public_api": []},
            {"name": "feature_two", "module": "x::feature_two::*", "public_api": []},
        ]));
        assert!(suggest(&air, &lf).is_empty());
    }

    #[test]
    fn empty_when_fewer_than_two_features_declared() {
        let air = ws(vec![file_with_imports(
            "x::feature_one::handler",
            &["x::feature_one::other"],
        )]);
        let lf = dg_lockfile(serde_json::json!([
            {"name": "feature_one", "module": "x::feature_one::*", "public_api": []},
        ]));
        assert!(suggest(&air, &lf).is_empty());
    }
}
