//! `locus init` suggestions for the CF paradigm.

use locus_air::AirWorkspace;

use super::CF_PREFIX;
use super::lockfile_schema::CfSection;
use crate::init::{CommandOption, Suggestion, SuggestionCategory, detect_layers};
use crate::lockfile::Lockfile;

pub fn suggest(air: &AirWorkspace, lockfile: &Lockfile) -> Vec<Suggestion> {
    let section: CfSection = lockfile.paradigm_section(CF_PREFIX).unwrap_or_default();
    if !section.config_paths.is_empty() {
        return Vec::new();
    }
    if lockfile.is_acknowledged_empty(CF_PREFIX) {
        return Vec::new();
    }
    let layers = detect_layers(air);
    if layers.config.is_empty() {
        return Vec::new();
    }
    let commands: Vec<String> = layers
        .config
        .iter()
        .map(|g| format!("locus cf add-config-path \"{g}\""))
        .collect();
    vec![Suggestion {
        category: SuggestionCategory::Layer,
        headline: "config layer candidates detected".into(),
        why: vec![format!("globs: {}", layers.config.join(", "))],
        options: vec![
            CommandOption {
                label: "specify".into(),
                commands,
            },
            CommandOption {
                label: "or skip".into(),
                commands: vec![format!("locus init --acknowledge-empty {CF_PREFIX}")],
            },
        ],
        prefixes: vec![CF_PREFIX.into()],
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
    fn suggests_config_path_when_settings_module_present() {
        let air = ws_with("x::settings::server");
        let lf = Lockfile::empty();
        let s = suggest(&air, &lf);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].category, SuggestionCategory::Layer);
        assert!(
            s[0].options[0]
                .commands
                .iter()
                .any(|c| c.contains("locus cf add-config-path \"x::settings::*\""))
        );
    }

    #[test]
    fn no_suggestion_when_section_already_populated() {
        let air = ws_with("x::settings::server");
        let mut lf = Lockfile::empty();
        lf.paradigms.insert(
            CF_PREFIX.into(),
            serde_json::json!({"config_paths": ["x::settings::*"]}),
        );
        assert!(suggest(&air, &lf).is_empty());
    }

    #[test]
    fn no_suggestion_when_acknowledged_empty() {
        let air = ws_with("x::settings::server");
        let mut lf = Lockfile::empty();
        lf.acknowledged_empty.push(CF_PREFIX.into());
        assert!(suggest(&air, &lf).is_empty());
    }
}
