//! RM rule implementations.
//!
//! Implemented:
//! - [`rm001`]: function performs too many distinct kinds of work.
//! - [`rm002`]: converter performs a side-effect fact.
//!
//! Lockfile-driven: stays silent until the user opts in by setting
//! `paradigms.RM.default_max_action_kinds` (RM001) or by populating
//! `paradigms.RM.converter_paths` (RM002). This mirrors the DG/UT pattern —
//! pre-onboarding we don't have the data (or the user's intent) to call any
//! particular density "wrong."

use std::collections::BTreeMap;

use locus_air::{
    ActionKind, AirFact, AirItem, AirSpan, AirTruthAction, AirWorkspace, FactKind, FactTarget,
};

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

/// RM002 — converter performs a side-effect fact.
///
/// For every `AirFact` whose `kind` is one of the side-effect-shaped kinds
/// (`SpawnedWork`, `Logging`, `ConfigRead`) and whose `target` is
/// `FactTarget::Function { symbol }`, look up the targeted function's file
/// and fire when the file's `module_path` matches any pattern in
/// `converter_paths`. Converters are supposed to be pure mapping; mixing in
/// any side effect collapses the boundary that justifies a converter layer
/// in the first place.
///
/// Severity: Warning by default; Fatal under `--agent-strict`. Deterministic
/// — driven entirely by lockfile patterns and loader-emitted facts.
///
/// Silent when `converter_paths` is empty: same opt-in UX as the rest of the
/// paradigm.
pub fn rm002(air: &AirWorkspace, section: &RmSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.converter_paths.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    for fact in &air.facts {
        if !is_side_effect_fact_kind(fact.kind) {
            continue;
        }
        let FactTarget::Function { symbol } = &fact.target else {
            continue;
        };
        let Some((module_path, fn_span)) = lookup_function(air, symbol) else {
            continue;
        };
        let matched_pattern = section
            .converter_paths
            .iter()
            .find(|pat| matches_pattern(pat, module_path));
        let Some(matched_pattern) = matched_pattern else {
            continue;
        };
        out.push(rm002_diagnostic(
            fact,
            symbol,
            module_path,
            fn_span,
            matched_pattern,
            mode,
        ));
    }
    out
}

fn is_side_effect_fact_kind(kind: FactKind) -> bool {
    matches!(
        kind,
        FactKind::SpawnedWork | FactKind::Logging | FactKind::ConfigRead
    )
}

fn format_fact_kind(kind: FactKind) -> &'static str {
    match kind {
        FactKind::SpawnedWork => "spawned-work",
        FactKind::Logging => "logging",
        FactKind::ConfigRead => "config-read",
        FactKind::ExternalIo => "external-io",
        FactKind::PersistenceWrite => "persistence-write",
        FactKind::BlockingCall => "blocking-call",
        FactKind::HotPath => "hot-path",
        FactKind::RequestContext => "request-context",
        FactKind::BoundaryEntry => "boundary-entry",
        FactKind::RuntimeStateOwner => "runtime-state-owner",
        FactKind::BackgroundWorker => "background-worker",
    }
}

/// Resolve `symbol` against AIR. Returns the enclosing file's module_path
/// and the function's span. Mirrors `runtime_work::rules::lookup_function`.
fn lookup_function<'a>(air: &'a AirWorkspace, symbol: &str) -> Option<(&'a str, AirSpan)> {
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                if let AirItem::Function(f) = item
                    && f.symbol == symbol
                {
                    let module = file.module_path.as_deref()?;
                    return Some((module, f.span.clone()));
                }
            }
        }
    }
    None
}

fn rm002_diagnostic(
    fact: &AirFact,
    symbol: &str,
    module_path: &str,
    fn_span: AirSpan,
    matched_pattern: &str,
    mode: CheckMode,
) -> Diagnostic {
    let span = match &fact.target {
        FactTarget::Span(s) => s.clone(),
        FactTarget::Function { .. } | FactTarget::File { .. } => fn_span,
    };
    let kind_label = format_fact_kind(fact.kind);
    let evidence = fact.evidence.as_deref().unwrap_or("?");
    let mut why = vec![
        format!("module `{module_path}` matches converter-path pattern `{matched_pattern}`"),
        format!("fact kind: {kind_label}"),
        format!("evidence: `{evidence}`"),
        format!("enclosing function: `{symbol}`"),
    ];
    for r in &fact.reasons {
        why.push(r.clone());
    }
    Diagnostic {
        rule_id: "RM002".to_string(),
        severity: mode.elevate(Severity::Warning),
        span,
        concept: None,
        message: format!(
            "converter `{symbol}` in module `{module_path}` performs a {kind_label} side effect"
        ),
        why,
        suggested_fix: Some(format!(
            "move the {kind_label} side effect out of `{symbol}` and into a caller \
             (an orchestrator or use-case). Keep the converter pure mapping. If this \
             module is *not* actually a converter, remove its pattern from \
             `paradigms.RM.converter_paths` in `locus.lock`."
        )),
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
            facts: Vec::new(),
        }
    }

    fn enabled_section(cap: u32) -> RmSection {
        RmSection {
            default_max_action_kinds: Some(cap),
            exempt_paths: Vec::new(),
            converter_paths: Vec::new(),
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
            converter_paths: Vec::new(),
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

    // ---------- RM002 ----------

    fn fact(kind: FactKind, symbol: &str, evidence: &str, reason: &str) -> AirFact {
        AirFact {
            kind,
            target: FactTarget::Function {
                symbol: symbol.into(),
            },
            source: "test".into(),
            confidence: 1.0,
            reasons: vec![reason.into()],
            evidence: Some(evidence.into()),
        }
    }

    fn air_with_facts(
        files: Vec<(&str, Option<&str>, Vec<AirItem>)>,
        facts: Vec<AirFact>,
    ) -> AirWorkspace {
        let mut air = air_with(files);
        air.facts = facts;
        air
    }

    fn converter_section(patterns: Vec<&str>) -> RmSection {
        RmSection {
            default_max_action_kinds: None,
            exempt_paths: Vec::new(),
            converter_paths: patterns.into_iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn rm002_fires_on_logging_in_converter_module() {
        let air = air_with_facts(
            vec![(
                "src/mapping/user.rs",
                Some("crate::mapping::user"),
                vec![func(
                    "crate::mapping::user::to_dto",
                    "src/mapping/user.rs",
                    7,
                )],
            )],
            vec![fact(
                FactKind::Logging,
                "crate::mapping::user::to_dto",
                "tracing::info!",
                "`tracing::info!` is a logging primitive",
            )],
        );
        let section = converter_section(vec!["crate::mapping::*"]);
        let diags = rm002(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        let d = &diags[0];
        assert_eq!(d.rule_id, "RM002");
        assert_eq!(d.severity, Severity::Warning);
        assert_eq!(d.span.line_start, 7);
        assert!(d.message.contains("crate::mapping::user::to_dto"));
        assert!(d.message.contains("logging"));
        assert!(
            d.why.iter().any(|w| w.contains("crate::mapping::*")),
            "expected matched pattern in why; got {:?}",
            d.why
        );
        assert!(
            d.why.iter().any(|w| w.contains("tracing::info!")),
            "expected evidence in why; got {:?}",
            d.why
        );
        assert!(
            d.why.iter().any(|w| w.contains("logging")),
            "expected fact-kind label in why; got {:?}",
            d.why
        );
        assert!(
            d.why.iter().any(|w| w.contains("to_dto")),
            "expected enclosing function in why; got {:?}",
            d.why
        );
        assert!(
            d.why.iter().any(|w| w.contains("logging primitive")),
            "expected loader reason propagated; got {:?}",
            d.why
        );
    }

    #[test]
    fn rm002_fires_on_spawned_work_in_converter_module() {
        let air = air_with_facts(
            vec![(
                "src/mapping/user.rs",
                Some("crate::mapping::user"),
                vec![func(
                    "crate::mapping::user::to_dto",
                    "src/mapping/user.rs",
                    9,
                )],
            )],
            vec![fact(
                FactKind::SpawnedWork,
                "crate::mapping::user::to_dto",
                "tokio::spawn",
                "spawn-shaped call",
            )],
        );
        let section = converter_section(vec!["crate::mapping::*"]);
        let diags = rm002(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "RM002");
        assert!(diags[0].message.contains("spawned-work"));
    }

    #[test]
    fn rm002_fires_on_config_read_in_converter_module() {
        let air = air_with_facts(
            vec![(
                "src/mapping/user.rs",
                Some("crate::mapping::user"),
                vec![func(
                    "crate::mapping::user::to_dto",
                    "src/mapping/user.rs",
                    11,
                )],
            )],
            vec![fact(
                FactKind::ConfigRead,
                "crate::mapping::user::to_dto",
                "std::env::var",
                "env-var read",
            )],
        );
        let section = converter_section(vec!["crate::mapping::*"]);
        let diags = rm002(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "RM002");
        assert!(diags[0].message.contains("config-read"));
    }

    #[test]
    fn rm002_quiet_on_non_side_effect_facts_and_non_converter_paths() {
        let air = air_with_facts(
            vec![
                (
                    "src/mapping/user.rs",
                    Some("crate::mapping::user"),
                    vec![func(
                        "crate::mapping::user::to_dto",
                        "src/mapping/user.rs",
                        7,
                    )],
                ),
                (
                    "src/handler.rs",
                    Some("crate::handler"),
                    vec![func("crate::handler::create_user", "src/handler.rs", 12)],
                ),
            ],
            vec![
                // Non-side-effect kind targeting a converter — must not fire.
                AirFact {
                    kind: FactKind::BlockingCall,
                    target: FactTarget::Function {
                        symbol: "crate::mapping::user::to_dto".into(),
                    },
                    source: "test".into(),
                    confidence: 1.0,
                    reasons: Vec::new(),
                    evidence: Some("std::thread::sleep".into()),
                },
                AirFact {
                    kind: FactKind::ExternalIo,
                    target: FactTarget::Function {
                        symbol: "crate::mapping::user::to_dto".into(),
                    },
                    source: "test".into(),
                    confidence: 1.0,
                    reasons: Vec::new(),
                    evidence: Some("reqwest::get".into()),
                },
                // Side-effect kind targeting a non-converter — must not fire.
                fact(
                    FactKind::Logging,
                    "crate::handler::create_user",
                    "tracing::info!",
                    "logging primitive",
                ),
            ],
        );
        let section = converter_section(vec!["crate::mapping::*"]);
        assert!(rm002(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn rm002_silent_when_converter_paths_empty() {
        let air = air_with_facts(
            vec![(
                "src/mapping/user.rs",
                Some("crate::mapping::user"),
                vec![func(
                    "crate::mapping::user::to_dto",
                    "src/mapping/user.rs",
                    7,
                )],
            )],
            vec![fact(
                FactKind::Logging,
                "crate::mapping::user::to_dto",
                "tracing::info!",
                "logging primitive",
            )],
        );
        // Default RmSection has empty converter_paths; rule must be silent
        // even when a side-effect fact is present.
        let section = RmSection::default();
        assert!(rm002(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn rm002_agent_strict_elevates_to_fatal() {
        let air = air_with_facts(
            vec![(
                "src/mapping/user.rs",
                Some("crate::mapping::user"),
                vec![func(
                    "crate::mapping::user::to_dto",
                    "src/mapping/user.rs",
                    7,
                )],
            )],
            vec![fact(
                FactKind::Logging,
                "crate::mapping::user::to_dto",
                "tracing::info!",
                "logging primitive",
            )],
        );
        let section = converter_section(vec!["crate::mapping::*"]);
        let diags = rm002(&air, &section, CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }
}
