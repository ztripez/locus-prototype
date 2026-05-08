//! `locus init` suggestions for the RM paradigm.

use std::collections::{BTreeMap, BTreeSet};

use locus_air::{ActionKind, AirItem, AirWorkspace};

use super::RM_PREFIX;
use super::lockfile_schema::RmSection;
use crate::init::{CommandOption, Suggestion, SuggestionCategory, percentile};
use crate::lockfile::Lockfile;

pub fn suggest(air: &AirWorkspace, lockfile: &Lockfile) -> Vec<Suggestion> {
    let section: RmSection = lockfile.paradigm_section(RM_PREFIX).unwrap_or_default();
    if section.default_max_action_kinds.is_some() {
        return Vec::new();
    }
    if lockfile.is_acknowledged_empty(RM_PREFIX) {
        return Vec::new();
    }
    // Group distinct `ActionKind` values per enclosing function symbol.
    // Free-floating `TruthAction` items (no `function`) contribute nothing —
    // RM001 cares about per-function mixing, not workspace-wide spread.
    let mut by_fn: BTreeMap<String, BTreeSet<ActionKind>> = BTreeMap::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                if let AirItem::TruthAction(a) = item
                    && let Some(fn_sym) = a.function.as_deref()
                {
                    by_fn
                        .entry(fn_sym.to_string())
                        .or_default()
                        .insert(a.action);
                }
            }
        }
    }
    let kinds_per_fn: Vec<u32> = by_fn.values().map(|set| set.len() as u32).collect();
    let Some(p95) = percentile(&kinds_per_fn, 0.95) else {
        return Vec::new();
    };
    if p95 <= 3 {
        return Vec::new();
    }
    let suggested = ((p95 as f32) * 1.1).ceil() as u32;
    vec![Suggestion {
        category: SuggestionCategory::Threshold,
        headline: format!("RM001 action-kinds-per-fn p95 = {p95}; no cap set"),
        why: vec!["consider an explicit cap so RM001 fires meaningfully".into()],
        options: vec![CommandOption {
            label: "set explicit cap".into(),
            commands: vec![format!("locus rm set-default --max-kinds {suggested}")],
        }],
        prefixes: vec![RM_PREFIX.into()],
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_threshold_suggestion_on_empty_workspace() {
        let air = AirWorkspace::new(Vec::new());
        let lf = Lockfile::empty();
        assert!(suggest(&air, &lf).is_empty());
    }

    #[test]
    fn no_suggestion_when_acknowledged_empty() {
        let air = AirWorkspace::new(Vec::new());
        let mut lf = Lockfile::empty();
        lf.acknowledged_empty.push(RM_PREFIX.into());
        assert!(suggest(&air, &lf).is_empty());
    }

    #[test]
    fn no_suggestion_when_default_already_set() {
        let air = AirWorkspace::new(Vec::new());
        let mut lf = Lockfile::empty();
        lf.paradigms.insert(
            RM_PREFIX.into(),
            serde_json::json!({"default_max_action_kinds": 3}),
        );
        assert!(suggest(&air, &lf).is_empty());
    }
}
