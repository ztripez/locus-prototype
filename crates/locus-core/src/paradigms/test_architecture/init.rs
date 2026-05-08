//! `locus init` suggestions for the TA paradigm.

use locus_air::AirWorkspace;

use super::TA_PREFIX;
use super::lockfile_schema::TaSection;
use crate::init::{CommandOption, Suggestion, SuggestionCategory, detect_layers};
use crate::lockfile::Lockfile;

pub fn suggest(air: &AirWorkspace, lockfile: &Lockfile) -> Vec<Suggestion> {
    let section: TaSection = lockfile.paradigm_section(TA_PREFIX).unwrap_or_default();
    if !section.test_paths.is_empty() {
        return Vec::new();
    }
    if lockfile.is_acknowledged_empty(TA_PREFIX) {
        return Vec::new();
    }
    let layers = detect_layers(air);
    if layers.tests.is_empty() {
        return Vec::new();
    }
    let commands: Vec<String> = layers
        .tests
        .iter()
        .map(|g| format!("locus ta add-test-path \"{g}\""))
        .collect();
    vec![Suggestion {
        category: SuggestionCategory::Layer,
        headline: "test paths detected".into(),
        why: vec![format!("globs: {}", layers.tests.join(", "))],
        options: vec![
            CommandOption {
                label: "specify".into(),
                commands,
            },
            CommandOption {
                label: "or skip".into(),
                commands: vec![format!("locus init --acknowledge-empty {TA_PREFIX}")],
            },
        ],
        prefixes: vec![TA_PREFIX.into()],
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
    fn suggests_test_path_when_tests_module_present() {
        let air = ws_with("x::user::tests");
        let lf = Lockfile::empty();
        let s = suggest(&air, &lf);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].category, SuggestionCategory::Layer);
        assert!(
            s[0].options[0]
                .commands
                .iter()
                .any(|c| c.contains("locus ta add-test-path \"x::user::tests::*\""))
        );
    }

    #[test]
    fn no_suggestion_when_section_already_populated() {
        let air = ws_with("x::user::tests");
        let mut lf = Lockfile::empty();
        lf.paradigms.insert(
            TA_PREFIX.into(),
            serde_json::json!({"test_paths": ["x::user::tests::*"]}),
        );
        assert!(suggest(&air, &lf).is_empty());
    }

    #[test]
    fn no_suggestion_when_acknowledged_empty() {
        let air = ws_with("x::user::tests");
        let mut lf = Lockfile::empty();
        lf.acknowledged_empty.push(TA_PREFIX.into());
        assert!(suggest(&air, &lf).is_empty());
    }
}
