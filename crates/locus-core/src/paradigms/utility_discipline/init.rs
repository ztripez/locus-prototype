//! `locus init` suggestions for the UT paradigm.

use locus_air::AirWorkspace;

use super::UT_PREFIX;
use super::lockfile_schema::UtSection;
use crate::init::{CommandOption, Suggestion, SuggestionCategory, detect_layers};
use crate::lockfile::Lockfile;

pub fn suggest(air: &AirWorkspace, lockfile: &Lockfile) -> Vec<Suggestion> {
    let section: UtSection = lockfile.paradigm_section(UT_PREFIX).unwrap_or_default();
    if !section.utility_paths.is_empty() {
        return Vec::new();
    }
    if lockfile.is_acknowledged_empty(UT_PREFIX) {
        return Vec::new();
    }
    let layers = detect_layers(air);
    if layers.utilities.is_empty() {
        return Vec::new();
    }
    let commands: Vec<String> = layers
        .utilities
        .iter()
        .map(|g| format!("locus ut add-utility-path \"{g}\""))
        .collect();
    vec![Suggestion {
        category: SuggestionCategory::Layer,
        headline: "utility module candidates detected".into(),
        why: vec![format!("globs: {}", layers.utilities.join(", "))],
        options: vec![
            CommandOption {
                label: "specify".into(),
                commands,
            },
            CommandOption {
                label: "or skip".into(),
                commands: vec![format!("locus init --acknowledge-empty {UT_PREFIX}")],
            },
        ],
        prefixes: vec![UT_PREFIX.into()],
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
    fn suggests_utility_path_when_common_module_present() {
        let air = ws_with("x::common::helpers");
        let lf = Lockfile::empty();
        let s = suggest(&air, &lf);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].category, SuggestionCategory::Layer);
        assert!(
            s[0].options[0]
                .commands
                .iter()
                .any(|c| c.contains("locus ut add-utility-path \"x::common::*\""))
        );
    }

    #[test]
    fn no_suggestion_when_section_already_populated() {
        let air = ws_with("x::common::helpers");
        let mut lf = Lockfile::empty();
        lf.paradigms.insert(
            UT_PREFIX.into(),
            serde_json::json!({"utility_paths": ["x::common::*"]}),
        );
        assert!(suggest(&air, &lf).is_empty());
    }

    #[test]
    fn no_suggestion_when_acknowledged_empty() {
        let air = ws_with("x::common::helpers");
        let mut lf = Lockfile::empty();
        lf.acknowledged_empty.push(UT_PREFIX.into());
        assert!(suggest(&air, &lf).is_empty());
    }
}
