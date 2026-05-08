//! `locus init` suggestions for the MO paradigm.

use locus_air::{AirItem, AirWorkspace, Visibility};

use super::MO_PREFIX;
use super::lockfile_schema::MoSection;
use crate::init::{CommandOption, Suggestion, SuggestionCategory, percentile};
use crate::lockfile::Lockfile;

const SPEC_DEFAULT_PUBLIC_TYPES: u32 = 5;
const TOLERANCE: f32 = 1.5;

pub fn suggest(air: &AirWorkspace, lockfile: &Lockfile) -> Vec<Suggestion> {
    let section: MoSection = lockfile.paradigm_section(MO_PREFIX).unwrap_or_default();
    if section.default_max_public_types.is_some() {
        return Vec::new();
    }
    if lockfile.is_acknowledged_empty(MO_PREFIX) {
        return Vec::new();
    }
    let counts: Vec<u32> = air
        .packages
        .iter()
        .flat_map(|p| p.files.iter())
        .map(|f| {
            f.items
                .iter()
                .filter(
                    |i| matches!(i, AirItem::Type(t) if matches!(t.visibility, Visibility::Public)),
                )
                .count() as u32
        })
        .collect();
    let Some(p95) = percentile(&counts, 0.95) else {
        return Vec::new();
    };
    let d = SPEC_DEFAULT_PUBLIC_TYPES as f32;
    if (p95 as f32) > d * TOLERANCE {
        let suggested = ((p95 as f32) * 1.1).ceil() as u32;
        vec![Suggestion {
            category: SuggestionCategory::Threshold,
            headline: format!(
                "MO001 public-types-per-file p95 = {p95}; default = {SPEC_DEFAULT_PUBLIC_TYPES}"
            ),
            why: vec!["p95 above default by >1.5×".into()],
            options: vec![CommandOption {
                label: "set explicit cap".into(),
                commands: vec![format!("locus mo set-default-max-public-types {suggested}")],
            }],
            prefixes: vec![MO_PREFIX.into()],
        }]
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use locus_air::{
        AirFile, AirItem, AirPackage, AirSpan, AirType, AirWorkspace, TypeKind, Visibility,
    };

    fn ty_item(name: &str) -> AirItem {
        AirItem::Type(AirType {
            kind: TypeKind::Struct,
            name: name.into(),
            symbol: format!("x::{name}"),
            symbol_segments: vec!["x".into(), name.into()],
            visibility: Visibility::Public,
            fields: Vec::new(),
            variants: Vec::new(),
            decorators: Vec::new(),
            span: AirSpan::new("t.rs", 1, 1),
            doc: None,
        })
    }

    fn ws_with_files(public_per_file: &[u32]) -> AirWorkspace {
        AirWorkspace::new(vec![AirPackage {
            name: "x".into(),
            version: "0.0.1".into(),
            root_dir: "/tmp/x".into(),
            files: public_per_file
                .iter()
                .enumerate()
                .map(|(i, n)| AirFile {
                    path: format!("src/f{i}.rs"),
                    module_path: Some(format!("x::f{i}")),
                    items: (0..*n).map(|j| ty_item(&format!("T{i}_{j}"))).collect(),
                    hints: Vec::new(),
                    parse_error: None,
                    line_count: 100,
                })
                .collect(),
        }])
    }

    #[test]
    fn no_suggestion_when_p95_within_tolerance() {
        // Files with 1–2 public types each — p95 = 2, default = 5; 2 <= 5*1.5
        // so no suggestion.
        let counts: Vec<u32> = (0..20).map(|i| if i % 2 == 0 { 1 } else { 2 }).collect();
        let air = ws_with_files(&counts);
        let lf = Lockfile::empty();
        assert!(suggest(&air, &lf).is_empty());
    }

    #[test]
    fn suggestion_when_p95_far_above_default() {
        // 18 files with 1 public type, 2 files with 20. p95 of 20 entries =
        // ceil(20*0.95) = 19, index 18 (0-based), so the 19th value in the
        // sorted slice = 20. 20 > 5*1.5, so a threshold suggestion fires.
        let mut counts: Vec<u32> = vec![1; 18];
        counts.push(20);
        counts.push(20);
        let air = ws_with_files(&counts);
        let lf = Lockfile::empty();
        let s = suggest(&air, &lf);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].category, SuggestionCategory::Threshold);
    }
}
