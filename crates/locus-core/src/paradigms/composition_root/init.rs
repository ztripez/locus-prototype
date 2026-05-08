//! `locus init` suggestions for the CR paradigm.

use locus_air::AirWorkspace;

use super::CR_PREFIX;
use super::lockfile_schema::CrSection;
use crate::init::{CommandOption, Suggestion, SuggestionCategory, detect_layers};
use crate::lockfile::Lockfile;

pub fn suggest(air: &AirWorkspace, lockfile: &Lockfile) -> Vec<Suggestion> {
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
}
