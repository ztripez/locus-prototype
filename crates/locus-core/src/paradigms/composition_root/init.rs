//! `locus init` suggestions for the CR paradigm.

use locus_air::AirWorkspace;

use super::CR_PREFIX;
use super::lockfile_schema::CrSection;
use crate::init::{CommandOption, Suggestion, SuggestionCategory, detect_layers, percentile};
use crate::lockfile::Lockfile;

pub fn suggest(air: &AirWorkspace, lockfile: &Lockfile) -> Vec<Suggestion> {
    let mut out = layer_suggestions(air, lockfile);
    out.extend(suggest_wiring_density(air, lockfile));
    out
}

fn layer_suggestions(air: &AirWorkspace, lockfile: &Lockfile) -> Vec<Suggestion> {
    let section: CrSection = lockfile.paradigm_section(CR_PREFIX).unwrap_or_default();
    if !section.composition_root_paths.is_empty() {
        return Vec::new();
    }
    if lockfile.is_acknowledged_empty(CR_PREFIX) {
        return Vec::new();
    }
    let layers = detect_layers(air);
    if layers.composition.is_empty() {
        return Vec::new();
    }
    let commands: Vec<String> = layers
        .composition
        .iter()
        .map(|g| format!("locus cr add-composition-root \"{g}\""))
        .collect();
    vec![Suggestion {
        category: SuggestionCategory::Layer,
        headline: "composition root candidates detected".into(),
        why: vec![format!("globs: {}", layers.composition.join(", "))],
        options: vec![
            CommandOption {
                label: "specify".into(),
                commands,
            },
            CommandOption {
                label: "or skip".into(),
                commands: vec![format!("locus init --acknowledge-empty {CR_PREFIX}")],
            },
        ],
        prefixes: vec![CR_PREFIX.into()],
    }]
}

fn suggest_wiring_density(air: &AirWorkspace, lockfile: &Lockfile) -> Vec<Suggestion> {
    use locus_air::{ActionKind, AirItem};
    use std::collections::BTreeMap;

    let section: CrSection = lockfile.paradigm_section(CR_PREFIX).unwrap_or_default();
    if lockfile.is_acknowledged_empty(CR_PREFIX) {
        return Vec::new();
    }
    // Group `Construct`-action counts by enclosing function symbol. Entries
    // without a `function` (free-floating Construct facts) contribute nothing
    // — wiring density is a per-function shape.
    let mut counts: BTreeMap<String, u32> = BTreeMap::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                if let AirItem::TruthAction(a) = item
                    && a.action == ActionKind::Construct
                    && let Some(fn_sym) = a.function.as_deref()
                {
                    *counts.entry(fn_sym.to_string()).or_insert(0) += 1;
                }
            }
        }
    }
    let values: Vec<u32> = counts.values().copied().collect();
    let Some(p95) = percentile(&values, 0.95) else {
        return Vec::new();
    };
    if (p95 as f32) <= section.wiring_density_threshold as f32 * 1.5 {
        return Vec::new();
    }
    let suggested = ((p95 as f32) * 1.1).ceil() as u32;
    vec![Suggestion {
        category: SuggestionCategory::Threshold,
        headline: format!(
            "CR002 wiring-density p95 = {p95}; current cap = {}",
            section.wiring_density_threshold
        ),
        why: vec!["p95 above current cap by >1.5×".into()],
        options: vec![CommandOption {
            label: "raise the cap".into(),
            commands: vec![format!("locus cr set-wiring-density-threshold {suggested}")],
        }],
        prefixes: vec![CR_PREFIX.into()],
    }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use locus_air::{AirFile, AirPackage, AirWorkspace};

    fn ws_with(module: &str) -> AirWorkspace {
        AirWorkspace::new(vec![AirPackage {
            name: "x".into(),
            version: "0.0.1".into(),
            root_dir: "/tmp/x".into(),
            files: vec![AirFile {
                path: format!("src/{}.rs", module.replace("::", "/")),
                module_path: Some(module.into()),
                items: Vec::new(),
                hints: Vec::new(),
                parse_error: None,
                line_count: 1,
            }],
        }])
    }

    #[test]
    fn suggests_composition_root_when_bin_module_present() {
        let air = ws_with("x::bin::main");
        let lf = Lockfile::empty();
        let s = suggest(&air, &lf);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].category, SuggestionCategory::Layer);
        assert!(
            s[0].options[0]
                .commands
                .iter()
                .any(|c| c.contains("locus cr add-composition-root \"x::bin::*\""))
        );
    }

    #[test]
    fn no_suggestion_when_section_already_populated() {
        let air = ws_with("x::bin::main");
        let mut lf = Lockfile::empty();
        lf.paradigms.insert(
            CR_PREFIX.into(),
            serde_json::json!({"composition_root_paths": ["x::bin::*"]}),
        );
        assert!(suggest(&air, &lf).is_empty());
    }

    #[test]
    fn no_suggestion_when_acknowledged_empty() {
        let air = ws_with("x::bin::main");
        let mut lf = Lockfile::empty();
        lf.acknowledged_empty.push(CR_PREFIX.into());
        assert!(suggest(&air, &lf).is_empty());
    }

    #[test]
    fn no_threshold_suggestion_on_empty_workspace() {
        // Default wiring_density_threshold is 12; with no Construct actions,
        // we shouldn't fire a threshold suggestion.
        let air = AirWorkspace::new(Vec::new());
        let lf = Lockfile::empty();
        let s = suggest(&air, &lf);
        assert!(
            s.iter()
                .all(|s| s.category != SuggestionCategory::Threshold),
            "no Construct actions should mean no threshold suggestion"
        );
    }
}
