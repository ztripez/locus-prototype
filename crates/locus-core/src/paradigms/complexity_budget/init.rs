//! `locus init` suggestions for the CX paradigm.

use locus_air::{AirItem, AirWorkspace};

use super::CX_PREFIX;
use super::lockfile_schema::CxSection;
use crate::init::{CommandOption, Suggestion, SuggestionCategory, percentile};
use crate::lockfile::Lockfile;

const SPEC_DEFAULT_FUNCTION_LINES: u32 = 50;
const TOLERANCE: f32 = 1.5;

pub fn suggest(air: &AirWorkspace, lockfile: &Lockfile) -> Vec<Suggestion> {
    let section: CxSection = lockfile.paradigm_section(CX_PREFIX).unwrap_or_default();
    if section.default_max_function_lines.is_some() {
        return Vec::new();
    }
    if lockfile.is_acknowledged_empty(CX_PREFIX) {
        return Vec::new();
    }
    let function_lines: Vec<u32> = air
        .packages
        .iter()
        .flat_map(|p| p.files.iter())
        .flat_map(|f| {
            f.items.iter().filter_map(|item| match item {
                AirItem::Function(fun) => {
                    Some(fun.span.line_end.saturating_sub(fun.span.line_start) + 1)
                }
                _ => None,
            })
        })
        .collect();
    let Some(p95) = percentile(&function_lines, 0.95) else {
        return Vec::new();
    };
    let default_f = SPEC_DEFAULT_FUNCTION_LINES as f32;
    let p95_f = p95 as f32;
    if p95_f > default_f * TOLERANCE || p95_f * TOLERANCE < default_f {
        let suggested = (p95_f * 1.1).ceil() as u32;
        vec![Suggestion {
            category: SuggestionCategory::Threshold,
            headline: format!(
                "CX001 function-line p95 = {p95}; default = {SPEC_DEFAULT_FUNCTION_LINES}"
            ),
            why: vec!["p95 differs from default by >1.5×; consider explicit cap".into()],
            options: vec![CommandOption {
                label: "set explicit cap".into(),
                commands: vec![format!("locus cx set-default --max-lines {suggested}")],
            }],
            prefixes: vec![CX_PREFIX.into()],
        }]
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use locus_air::{AirFile, AirFunction, AirItem, AirPackage, AirSpan, AirWorkspace, Visibility};

    fn fn_item(start: u32, end: u32) -> AirItem {
        let line_count = end.saturating_sub(start) + 1;
        AirItem::Function(AirFunction {
            name: "f".into(),
            symbol: format!("x::f@{start}"),
            symbol_segments: vec!["x".into(), "f".into()],
            visibility: Visibility::Public,
            params: Vec::new(),
            return_type: None,
            span: AirSpan::new("t.rs", start, end),
            line_count,
            doc: None,
            decorators: Vec::new(),
        })
    }

    fn ws_with_fns(spans: &[(u32, u32)]) -> AirWorkspace {
        AirWorkspace::new(vec![AirPackage {
            name: "x".into(),
            version: "0.0.1".into(),
            root_dir: "/tmp/x".into(),
            files: vec![AirFile {
                path: "src/lib.rs".into(),
                module_path: Some("x".into()),
                items: spans.iter().map(|(s, e)| fn_item(*s, *e)).collect(),
                hints: Vec::new(),
                parse_error: None,
                line_count: 200,
            }],
        }])
    }

    #[test]
    fn no_suggestion_when_p95_within_tolerance() {
        // line_count = 41 (i + 40 - i + 1) — within [default/1.5, default*1.5]
        // = [~33, 75] for default = 50.
        let spans: Vec<(u32, u32)> = (1..=20).map(|i| (i, i + 40)).collect();
        let air = ws_with_fns(&spans);
        let lf = Lockfile::empty();
        assert!(suggest(&air, &lf).is_empty());
    }

    #[test]
    fn suggestion_when_p95_far_above_default() {
        let spans: Vec<(u32, u32)> = (1..=20).map(|i| (i, i + 200)).collect();
        let air = ws_with_fns(&spans);
        let lf = Lockfile::empty();
        let s = suggest(&air, &lf);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].category, SuggestionCategory::Threshold);
    }

    #[test]
    fn no_suggestion_when_acknowledged_empty() {
        let spans: Vec<(u32, u32)> = (1..=5).map(|i| (i, i + 200)).collect();
        let air = ws_with_fns(&spans);
        let mut lf = Lockfile::empty();
        lf.acknowledged_empty.push(CX_PREFIX.into());
        assert!(suggest(&air, &lf).is_empty());
    }
}
