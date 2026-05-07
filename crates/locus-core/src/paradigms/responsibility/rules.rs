//! RM rule implementations.
//!
//! Implemented:
//! - [`rm001`]: function performs too many distinct kinds of work.
//!
//! Lockfile-driven: stays silent until the user opts in by setting
//! `paradigms.RM.default_max_action_kinds`. This mirrors the DG/UT pattern —
//! pre-onboarding we don't have the data (or the user's intent) to call any
//! particular density "wrong."

use std::collections::BTreeMap;

use locus_air::{ActionKind, AirItem, AirSpan, AirTruthAction, AirWorkspace};

use super::lockfile_schema::{RmSection, matches_pattern};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

/// RM001 — function performs too many distinct kinds of work.
///
/// Walks every `AirTruthAction` whose `function` is recorded, groups by the
/// enclosing function symbol, and counts the distinct `ActionKind` values for
/// each. Fires once per function whose distinct-kind count exceeds
/// [`RmSection::effective_default`]. The diagnostic pins to the function's
/// `AirItem::Function` span when one is available, otherwise falls back to the
/// first action's span.
///
/// Severity: Warning by default; Fatal under `--agent-strict`.
pub fn rm001(air: &AirWorkspace, section: &RmSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.default_max_action_kinds.is_none() {
        return Vec::new();
    }
    let cap = section.effective_default();

    // Index functions by symbol: (span, file_path).
    let mut function_index: BTreeMap<String, (AirSpan, String)> = BTreeMap::new();
    // Index file paths to their module_path so we can match exempt patterns.
    let mut module_path_for_file: BTreeMap<String, String> = BTreeMap::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            if let Some(mp) = file.module_path.as_deref() {
                module_path_for_file.insert(file.path.clone(), mp.to_string());
            }
            for item in &file.items {
                if let AirItem::Function(f) = item {
                    function_index
                        .entry(f.symbol.clone())
                        .or_insert_with(|| (f.span.clone(), file.path.clone()));
                }
            }
        }
    }

    // Group actions by enclosing function symbol, preserving order of first
    // appearance for the diagnostic's `why` payload. `ActionKind` is `Copy`
    // but not `Hash`/`Ord`, so distinct-kinds is tracked as a small `Vec`
    // with a manual membership check (only five variants exist).
    #[derive(Default)]
    struct Group<'a> {
        kinds: Vec<ActionKind>,
        actions: Vec<&'a AirTruthAction>,
        first_file: Option<String>,
    }
    let mut groups: BTreeMap<String, Group<'_>> = BTreeMap::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                let AirItem::TruthAction(a) = item else {
                    continue;
                };
                let Some(fn_sym) = a.function.as_deref() else {
                    continue;
                };
                let g = groups.entry(fn_sym.to_string()).or_default();
                if !g.kinds.contains(&a.action) {
                    g.kinds.push(a.action);
                }
                g.actions.push(a);
                if g.first_file.is_none() {
                    g.first_file = Some(file.path.clone());
                }
            }
        }
    }

    let mut out = Vec::new();
    for (fn_sym, group) in groups {
        if (group.kinds.len() as u32) <= cap {
            continue;
        }
        // Resolve span + file path.
        let (span, file_path) = match function_index.get(&fn_sym) {
            Some((span, fp)) => (span.clone(), fp.clone()),
            None => {
                let first = group
                    .actions
                    .first()
                    .expect("group has at least one action");
                (
                    first.span.clone(),
                    group
                        .first_file
                        .clone()
                        .unwrap_or_else(|| first.span.file.clone()),
                )
            }
        };

        // Exempt-paths check: if the function's containing file's module_path
        // matches any exempt pattern, skip.
        if let Some(module_path) = module_path_for_file.get(&file_path)
            && section
                .exempt_paths
                .iter()
                .any(|pat| matches_pattern(pat, module_path))
        {
            continue;
        }

        // Build the diagnostic. Sort kinds by their stable string label so
        // the message and `why` are deterministic across runs.
        let mut kinds_sorted: Vec<ActionKind> = group.kinds.clone();
        kinds_sorted.sort_by_key(format_kind);
        let kinds_label = kinds_sorted
            .iter()
            .map(format_kind)
            .collect::<Vec<_>>()
            .join(", ");
        let mut why = vec![format!(
            "{} distinct ActionKind values present: {}",
            kinds_sorted.len(),
            kinds_label
        )];
        for action in group.actions.iter().take(5) {
            why.push(format!(
                "{} `{}` at {}:{}",
                format_kind(&action.action),
                action.target,
                action.span.file,
                action.span.line_start
            ));
        }
        if group.actions.len() > 5 {
            why.push(format!(
                "(+ {} more action(s) elided)",
                group.actions.len() - 5
            ));
        }
        let function_was_anchored = function_index.contains_key(&fn_sym);
        if !function_was_anchored {
            why.push(
                "no top-level `AirItem::Function` matched this enclosing symbol; \
                 span pinned to the first action"
                    .into(),
            );
        }

        out.push(Diagnostic {
            rule_id: "RM001".to_string(),
            severity: mode.elevate(Severity::Warning),
            span,
            concept: None,
            message: format!(
                "function `{fn_sym}` performs {} distinct kinds of work: {kinds_label}",
                kinds_sorted.len()
            ),
            why,
            suggested_fix: Some(format!(
                "split `{fn_sym}` along ownership lines: extract validation, construction, and \
                 side-effect orchestration into separate single-responsibility functions. If this \
                 density is intentional (e.g. a generated handler), add the file's module path to \
                 `paradigms.RM.exempt_paths` in `locus.lock`."
            )),
        });
    }
    out
}

fn format_kind(k: &ActionKind) -> String {
    match k {
        ActionKind::Construct => "Construct".to_string(),
        ActionKind::EnumMatch => "EnumMatch".to_string(),
        ActionKind::StringCompare => "StringCompare".to_string(),
        ActionKind::Validate => "Validate".to_string(),
        ActionKind::Normalize => "Normalize".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use locus_air::{
        AIR_SCHEMA_VERSION, AirFile, AirFunction, AirPackage, AirSpan, AirTruthAction,
        AirWorkspace, Visibility,
    };

    fn action(kind: ActionKind, target: &str, function: &str, file: &str, line: u32) -> AirItem {
        AirItem::TruthAction(AirTruthAction {
            action: kind,
            target: target.into(),
            function: Some(function.into()),
            span: AirSpan::new(file, line, line),
            confidence: 0.9,
            reasons: Vec::new(),
        })
    }

    fn func(symbol: &str, file: &str, line: u32) -> AirItem {
        AirItem::Function(AirFunction {
            name: symbol.rsplit("::").next().unwrap_or(symbol).into(),
            symbol: symbol.into(),
            visibility: Visibility::Public,
            params: Vec::new(),
            return_type: None,
            span: AirSpan::new(file, line, line + 10),
            line_count: 11,
            doc: None,
        })
    }

    fn air_with(files: Vec<(&str, Option<&str>, Vec<AirItem>)>) -> AirWorkspace {
        AirWorkspace {
            schema_version: AIR_SCHEMA_VERSION,
            packages: vec![AirPackage {
                name: "x".into(),
                version: "0".into(),
                root_dir: "/".into(),
                files: files
                    .into_iter()
                    .map(|(path, module, items)| AirFile {
                        path: path.into(),
                        module_path: module.map(|s| s.to_string()),
                        items,
                        hints: Vec::new(),
                        parse_error: None,
                        line_count: 50,
                    })
                    .collect(),
            }],
        }
    }

    fn enabled_section(cap: u32) -> RmSection {
        RmSection {
            default_max_action_kinds: Some(cap),
            exempt_paths: Vec::new(),
        }
    }

    #[test]
    fn rm001_fires_on_three_distinct_action_kinds() {
        let air = air_with(vec![(
            "src/handler.rs",
            Some("crate::handler"),
            vec![
                func("crate::handler::do_it", "src/handler.rs", 10),
                action(
                    ActionKind::Construct,
                    "User",
                    "crate::handler::do_it",
                    "src/handler.rs",
                    11,
                ),
                action(
                    ActionKind::Validate,
                    "email",
                    "crate::handler::do_it",
                    "src/handler.rs",
                    12,
                ),
                action(
                    ActionKind::Normalize,
                    "name",
                    "crate::handler::do_it",
                    "src/handler.rs",
                    13,
                ),
            ],
        )]);
        let diags = rm001(&air, &enabled_section(2), CheckMode::Human);
        assert_eq!(diags.len(), 1);
        let d = &diags[0];
        assert_eq!(d.rule_id, "RM001");
        assert_eq!(d.severity, Severity::Warning);
        assert!(d.message.contains("crate::handler::do_it"));
        assert!(d.message.contains('3'));
        // Span pinned to the function, not the action's line.
        assert_eq!(d.span.line_start, 10);
    }

    #[test]
    fn rm001_quiet_at_or_below_cap() {
        let air = air_with(vec![(
            "src/handler.rs",
            Some("crate::handler"),
            vec![
                func("crate::handler::ok", "src/handler.rs", 10),
                action(
                    ActionKind::Construct,
                    "User",
                    "crate::handler::ok",
                    "src/handler.rs",
                    11,
                ),
                action(
                    ActionKind::Validate,
                    "email",
                    "crate::handler::ok",
                    "src/handler.rs",
                    12,
                ),
            ],
        )]);
        assert!(rm001(&air, &enabled_section(2), CheckMode::Human).is_empty());
    }

    #[test]
    fn rm001_quiet_when_module_path_is_exempt() {
        let air = air_with(vec![(
            "src/handler.rs",
            Some("crate::handler::tests"),
            vec![
                func("crate::handler::tests::it_works", "src/handler.rs", 10),
                action(
                    ActionKind::Construct,
                    "User",
                    "crate::handler::tests::it_works",
                    "src/handler.rs",
                    11,
                ),
                action(
                    ActionKind::Validate,
                    "email",
                    "crate::handler::tests::it_works",
                    "src/handler.rs",
                    12,
                ),
                action(
                    ActionKind::Normalize,
                    "name",
                    "crate::handler::tests::it_works",
                    "src/handler.rs",
                    13,
                ),
            ],
        )]);
        let section = RmSection {
            default_max_action_kinds: Some(2),
            exempt_paths: vec!["crate::handler::tests::*".into()],
        };
        assert!(rm001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn rm001_silent_when_default_max_action_kinds_is_none() {
        let air = air_with(vec![(
            "src/handler.rs",
            Some("crate::handler"),
            vec![
                func("crate::handler::do_it", "src/handler.rs", 10),
                action(
                    ActionKind::Construct,
                    "User",
                    "crate::handler::do_it",
                    "src/handler.rs",
                    11,
                ),
                action(
                    ActionKind::Validate,
                    "email",
                    "crate::handler::do_it",
                    "src/handler.rs",
                    12,
                ),
                action(
                    ActionKind::Normalize,
                    "name",
                    "crate::handler::do_it",
                    "src/handler.rs",
                    13,
                ),
                action(
                    ActionKind::EnumMatch,
                    "Status",
                    "crate::handler::do_it",
                    "src/handler.rs",
                    14,
                ),
            ],
        )]);
        // Even though exempt_paths is empty AND there are 4 distinct kinds,
        // an unset default_max_action_kinds means the rule is fully silent.
        let section = RmSection::default();
        assert!(rm001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn rm001_one_diagnostic_per_function_regardless_of_action_count() {
        // Five Construct actions + one Validate + one Normalize = 3 distinct
        // kinds. Should fire exactly once for the function, not per action.
        let mut items = vec![func("crate::handler::do_it", "src/handler.rs", 10)];
        for i in 0..5 {
            items.push(action(
                ActionKind::Construct,
                "User",
                "crate::handler::do_it",
                "src/handler.rs",
                11 + i,
            ));
        }
        items.push(action(
            ActionKind::Validate,
            "email",
            "crate::handler::do_it",
            "src/handler.rs",
            20,
        ));
        items.push(action(
            ActionKind::Normalize,
            "name",
            "crate::handler::do_it",
            "src/handler.rs",
            21,
        ));
        let air = air_with(vec![("src/handler.rs", Some("crate::handler"), items)]);
        let diags = rm001(&air, &enabled_section(2), CheckMode::Human);
        assert_eq!(diags.len(), 1, "one diagnostic per function symbol");
    }

    #[test]
    fn rm001_agent_strict_elevates_to_fatal() {
        let air = air_with(vec![(
            "src/handler.rs",
            Some("crate::handler"),
            vec![
                func("crate::handler::do_it", "src/handler.rs", 10),
                action(
                    ActionKind::Construct,
                    "User",
                    "crate::handler::do_it",
                    "src/handler.rs",
                    11,
                ),
                action(
                    ActionKind::Validate,
                    "email",
                    "crate::handler::do_it",
                    "src/handler.rs",
                    12,
                ),
                action(
                    ActionKind::Normalize,
                    "name",
                    "crate::handler::do_it",
                    "src/handler.rs",
                    13,
                ),
            ],
        )]);
        let diags = rm001(&air, &enabled_section(2), CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    #[test]
    fn rm001_falls_back_to_first_action_span_when_function_not_in_air() {
        // No `AirItem::Function` for `crate::handler::do_it` — simulates an
        // enclosing function that isn't a top-level fn. Diagnostic should
        // still fire and pin to the first action's span.
        let air = air_with(vec![(
            "src/handler.rs",
            Some("crate::handler"),
            vec![
                action(
                    ActionKind::Construct,
                    "User",
                    "crate::handler::do_it",
                    "src/handler.rs",
                    11,
                ),
                action(
                    ActionKind::Validate,
                    "email",
                    "crate::handler::do_it",
                    "src/handler.rs",
                    12,
                ),
                action(
                    ActionKind::Normalize,
                    "name",
                    "crate::handler::do_it",
                    "src/handler.rs",
                    13,
                ),
            ],
        )]);
        let diags = rm001(&air, &enabled_section(2), CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].span.line_start, 11);
        assert!(
            diags[0]
                .why
                .iter()
                .any(|w| w.contains("no top-level `AirItem::Function`")),
            "why should explain the fallback; got {:?}",
            diags[0].why
        );
    }

    #[test]
    fn rm001_skips_actions_without_function() {
        // Actions with `function: None` are simply skipped; they shouldn't
        // fold into any group.
        let air = air_with(vec![(
            "src/handler.rs",
            Some("crate::handler"),
            vec![
                AirItem::TruthAction(AirTruthAction {
                    action: ActionKind::Construct,
                    target: "User".into(),
                    function: None,
                    span: AirSpan::new("src/handler.rs", 11, 11),
                    confidence: 0.9,
                    reasons: Vec::new(),
                }),
                AirItem::TruthAction(AirTruthAction {
                    action: ActionKind::Validate,
                    target: "email".into(),
                    function: None,
                    span: AirSpan::new("src/handler.rs", 12, 12),
                    confidence: 0.9,
                    reasons: Vec::new(),
                }),
                AirItem::TruthAction(AirTruthAction {
                    action: ActionKind::Normalize,
                    target: "name".into(),
                    function: None,
                    span: AirSpan::new("src/handler.rs", 13, 13),
                    confidence: 0.9,
                    reasons: Vec::new(),
                }),
            ],
        )]);
        assert!(rm001(&air, &enabled_section(2), CheckMode::Human).is_empty());
    }
}
