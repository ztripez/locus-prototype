//! RM rule implementations.
//!
//! Implemented:
//! - [`rm001`]: function performs too many distinct kinds of work.
//! - [`rm002`]: converter performs a side-effect fact.
//! - [`rm003`]: handler module containing branch-rich domain policy.
//! - [`rm004`]: repository module containing branch-rich domain logic.
//! - [`rm005`]: validator function performing IO (external or persistence).
//! - [`rm006`]: domain type method performing persistence-write.
//!
//! Lockfile-driven: stays silent until the user opts in by setting
//! `paradigms.RM.default_max_action_kinds` (RM001), populating
//! `paradigms.RM.converter_paths` (RM002), `paradigms.RM.handler_paths`
//! (RM003), `paradigms.RM.repository_paths` (RM004),
//! `paradigms.RM.validator_paths` (RM005), or
//! `paradigms.RM.domain_paths_rm` (RM006). This mirrors the DG/UT pattern
//! — pre-onboarding we don't have the data (or the user's intent) to call
//! any particular density "wrong."

use std::collections::BTreeMap;

use locus_air::{
    ActionKind, AirFact, AirItem, AirSpan, AirTruthAction, AirWorkspace, FactKind, FactTarget,
};

use super::lockfile_schema::{RmSection, matches_pattern};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

/// Per-function action accumulator for RM001.
#[derive(Default)]
struct Rm001Group<'a> {
    kinds: Vec<ActionKind>,
    actions: Vec<&'a AirTruthAction>,
    first_file: Option<String>,
}

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
    let (function_index, module_path_for_file) = build_workspace_indexes(air);
    let groups = build_rm001_groups(air);
    emit_rm001_diagnostics(
        groups,
        &function_index,
        &module_path_for_file,
        section,
        cap,
        mode,
    )
}

fn build_workspace_indexes(
    air: &AirWorkspace,
) -> (
    BTreeMap<String, (AirSpan, String)>,
    BTreeMap<String, String>,
) {
    let mut function_index: BTreeMap<String, (AirSpan, String)> = BTreeMap::new();
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
    (function_index, module_path_for_file)
}

fn build_rm001_groups(air: &AirWorkspace) -> BTreeMap<String, Rm001Group<'_>> {
    let mut groups: BTreeMap<String, Rm001Group<'_>> = BTreeMap::new();
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
    groups
}

fn emit_rm001_diagnostics(
    groups: BTreeMap<String, Rm001Group<'_>>,
    function_index: &BTreeMap<String, (AirSpan, String)>,
    module_path_for_file: &BTreeMap<String, String>,
    section: &RmSection,
    cap: u32,
    mode: CheckMode,
) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (fn_sym, group) in groups {
        if (group.kinds.len() as u32) <= cap {
            continue;
        }
        let anchored = function_index.contains_key(&fn_sym);
        let (span, file_path) = resolve_rm001_span(&fn_sym, &group, function_index);
        if let Some(mp) = module_path_for_file.get(&file_path)
            && section
                .exempt_paths
                .iter()
                .any(|pat| matches_pattern(pat, mp))
        {
            continue;
        }
        let mut kinds_sorted: Vec<ActionKind> = group.kinds.clone();
        kinds_sorted.sort_by_key(format_kind);
        let kinds_label = kinds_sorted
            .iter()
            .map(format_kind)
            .collect::<Vec<_>>()
            .join(", ");
        let why = build_rm001_why(&kinds_sorted, &kinds_label, &group.actions, anchored);
        out.push(rm001_diagnostic(
            &fn_sym,
            &kinds_sorted,
            &kinds_label,
            why,
            span,
            mode,
        ));
    }
    out
}

fn resolve_rm001_span<'a>(
    fn_sym: &str,
    group: &Rm001Group<'a>,
    function_index: &BTreeMap<String, (AirSpan, String)>,
) -> (AirSpan, String) {
    match function_index.get(fn_sym) {
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
    }
}

fn build_rm001_why(
    kinds_sorted: &[ActionKind],
    kinds_label: &str,
    actions: &[&AirTruthAction],
    anchored: bool,
) -> Vec<String> {
    let mut why = vec![format!(
        "{} distinct ActionKind values present: {kinds_label}",
        kinds_sorted.len()
    )];
    for action in actions.iter().take(5) {
        why.push(format!(
            "{} `{}` at {}:{}",
            format_kind(&action.action),
            action.target,
            action.span.file,
            action.span.line_start
        ));
    }
    if actions.len() > 5 {
        why.push(format!("(+ {} more action(s) elided)", actions.len() - 5));
    }
    if !anchored {
        why.push(
            "no top-level `AirItem::Function` matched this enclosing symbol; \
             span pinned to the first action"
                .into(),
        );
    }
    why
}

fn rm001_diagnostic(
    fn_sym: &str,
    kinds_sorted: &[ActionKind],
    kinds_label: &str,
    why: Vec<String>,
    span: AirSpan,
    mode: CheckMode,
) -> Diagnostic {
    Diagnostic {
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
             `paradigms.RM.exempt_paths` in `.locus/lock.json`."
        )),
    }
}

fn format_kind(k: &ActionKind) -> String {
    match k {
        ActionKind::Construct => "Construct".to_string(),
        ActionKind::DiscriminatedMatch => "EnumMatch".to_string(),
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
             `paradigms.RM.converter_paths` in `.locus/lock.json`."
        )),
    }
}

/// Which density rule fired — used to drive shared diagnostic plumbing.
#[derive(Clone, Copy)]
enum DensityRole {
    Handler,
    Repository,
}

impl DensityRole {
    fn rule_id(self) -> &'static str {
        match self {
            DensityRole::Handler => "RM003",
            DensityRole::Repository => "RM004",
        }
    }

    fn role_label(self) -> &'static str {
        match self {
            DensityRole::Handler => "handler",
            DensityRole::Repository => "repository",
        }
    }

    fn lockfile_paths_field(self) -> &'static str {
        match self {
            DensityRole::Handler => "paradigms.RM.handler_paths",
            DensityRole::Repository => "paradigms.RM.repository_paths",
        }
    }

    fn lockfile_cap_field(self) -> &'static str {
        match self {
            DensityRole::Handler => "paradigms.RM.max_handler_decisions",
            DensityRole::Repository => "paradigms.RM.max_repository_decisions",
        }
    }

    fn suggested_fix(self, fn_sym: &str, module_path: &str, count: u32) -> String {
        match self {
            DensityRole::Handler => format!(
                "the handler `{fn_sym}` in `{module_path}` is making {count} branch-style \
                 decisions ({{StringCompare, EnumMatch}}). Push the policy down into a domain \
                 module the handler delegates to — handlers should orchestrate, not branch. \
                 If this density is intentional, raise `paradigms.RM.max_handler_decisions` \
                 in `.locus/lock.json` or remove the module from `paradigms.RM.handler_paths`."
            ),
            DensityRole::Repository => format!(
                "the repository function `{fn_sym}` in `{module_path}` is making {count} \
                 branch-style decisions ({{StringCompare, EnumMatch}}). Repositories should \
                 stay close to persistence; lift the branching into a domain function the \
                 repository feeds. If this density is intentional, raise \
                 `paradigms.RM.max_repository_decisions` in `.locus/lock.json` or remove the \
                 module from `paradigms.RM.repository_paths`."
            ),
        }
    }
}

/// Shared core for RM003 / RM004. Walks every `AirItem::Function`, looks up
/// the enclosing file's `module_path`, and counts the function's
/// `StringCompare` + `EnumMatch` `AirTruthAction`s. Fires when the count
/// exceeds `cap` and the file's `module_path` matches one of `paths`.
fn density_rule(
    air: &AirWorkspace,
    paths: &[String],
    cap: u32,
    role: DensityRole,
    mode: CheckMode,
) -> Vec<Diagnostic> {
    if paths.is_empty() {
        return Vec::new();
    }
    let (function_index, module_path_for_file) = build_workspace_indexes(air);
    let mut groups: BTreeMap<String, Vec<&AirTruthAction>> = BTreeMap::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                let AirItem::TruthAction(a) = item else {
                    continue;
                };
                if !matches!(
                    a.action,
                    ActionKind::StringCompare | ActionKind::DiscriminatedMatch
                ) {
                    continue;
                }
                let Some(fn_sym) = a.function.as_deref() else {
                    continue;
                };
                groups.entry(fn_sym.to_string()).or_default().push(a);
            }
        }
    }
    emit_density_diagnostics(
        groups,
        &function_index,
        &module_path_for_file,
        paths,
        cap,
        role,
        mode,
    )
}

fn emit_density_diagnostics(
    groups: BTreeMap<String, Vec<&AirTruthAction>>,
    function_index: &BTreeMap<String, (AirSpan, String)>,
    module_path_for_file: &BTreeMap<String, String>,
    paths: &[String],
    cap: u32,
    role: DensityRole,
    mode: CheckMode,
) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (fn_sym, actions) in groups {
        let count = actions.len() as u32;
        if count <= cap {
            continue;
        }
        let Some((fn_span, file_path)) = function_index.get(&fn_sym) else {
            continue;
        };
        let Some(module_path) = module_path_for_file.get(file_path) else {
            continue;
        };
        let Some(matched_pattern) = paths
            .iter()
            .find(|pat| matches_pattern(pat, module_path))
            .cloned()
        else {
            continue;
        };
        let why = build_density_why(module_path, &matched_pattern, count, cap, &actions, role);
        out.push(density_diagnostic(
            &fn_sym,
            fn_span,
            module_path,
            count,
            role,
            why,
            mode,
        ));
    }
    out
}

fn build_density_why(
    module_path: &str,
    matched_pattern: &str,
    count: u32,
    cap: u32,
    actions: &[&AirTruthAction],
    role: DensityRole,
) -> Vec<String> {
    let mut why = vec![
        format!(
            "module `{module_path}` matches {} pattern `{matched_pattern}`",
            role.lockfile_paths_field()
        ),
        format!(
            "{count} StringCompare/EnumMatch action(s) — cap is {cap} (`{}`)",
            role.lockfile_cap_field()
        ),
    ];
    for action in actions.iter().take(5) {
        why.push(format!(
            "{} `{}` at {}:{}",
            format_kind(&action.action),
            action.target,
            action.span.file,
            action.span.line_start
        ));
    }
    if actions.len() > 5 {
        why.push(format!("(+ {} more action(s) elided)", actions.len() - 5));
    }
    why
}

fn density_diagnostic(
    fn_sym: &str,
    fn_span: &AirSpan,
    module_path: &str,
    count: u32,
    role: DensityRole,
    why: Vec<String>,
    mode: CheckMode,
) -> Diagnostic {
    Diagnostic {
        rule_id: role.rule_id().to_string(),
        severity: mode.elevate(Severity::Warning),
        span: fn_span.clone(),
        concept: None,
        message: format!(
            "{role} `{fn_sym}` makes {count} branch-style decision(s); \
             {role}s should not host this much policy",
            role = role.role_label()
        ),
        why,
        suggested_fix: Some(role.suggested_fix(fn_sym, module_path, count)),
    }
}

/// RM003 — handler module containing domain policy (branch-rich logic).
///
/// For each `AirItem::Function` whose enclosing file's `module_path` matches a
/// pattern in `handler_paths`, count the function's `StringCompare` +
/// `EnumMatch` `AirTruthAction`s. Fires when that count exceeds
/// [`RmSection::effective_max_handler_decisions`]. Handlers are supposed to
/// orchestrate; branch-rich policy belongs in a domain module the handler
/// delegates to.
///
/// Severity: Warning by default; Fatal under `--agent-strict`.
///
/// Silent when `handler_paths` is empty: same opt-in UX as the rest of RM.
pub fn rm003(air: &AirWorkspace, section: &RmSection, mode: CheckMode) -> Vec<Diagnostic> {
    density_rule(
        air,
        &section.handler_paths,
        section.effective_max_handler_decisions(),
        DensityRole::Handler,
        mode,
    )
}

/// RM004 — repository module containing branch-rich logic.
///
/// Mirrors RM003's shape: for each `AirItem::Function` whose enclosing file's
/// `module_path` matches a pattern in `repository_paths`, count the function's
/// `StringCompare` + `EnumMatch` actions and fire when that count exceeds
/// [`RmSection::effective_max_repository_decisions`]. The prescription
/// differs from RM003: repository functions should be thin queries, so the
/// fix is to push branches up into the domain layer the repository feeds —
/// not down into a sibling module the way RM003 suggests.
///
/// Severity: Warning by default; Fatal under `--agent-strict`.
///
/// Silent when `repository_paths` is empty.
pub fn rm004(air: &AirWorkspace, section: &RmSection, mode: CheckMode) -> Vec<Diagnostic> {
    density_rule(
        air,
        &section.repository_paths,
        section.effective_max_repository_decisions(),
        DensityRole::Repository,
        mode,
    )
}

/// RM005 — validator function performing IO.
///
/// For every `FactKind::ExternalIo` or `FactKind::PersistenceWrite` fact
/// whose `target` is a `Function`, look up the targeted function and fire
/// when its enclosing file's `module_path` matches a pattern in
/// `validator_paths`. A validator is conceptually a pure decision function;
/// performing IO inside one means the function fails differently for the
/// same input depending on external state — a responsibility violation.
///
/// Severity: Warning by default; Fatal under `--agent-strict`.
///
/// Silent when `validator_paths` is empty: same opt-in UX as the rest of RM.
///
/// Module-path resolution: the file's `module_path` is checked first; if
/// no match, the function symbol itself is matched against the same
/// patterns. Lets `*::tests::*` exempt patterns reach inline test modules
/// whose file `module_path` doesn't include the segment.
pub fn rm005(air: &AirWorkspace, section: &RmSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.validator_paths.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for fact in &air.facts {
        if !is_io_fact_kind(fact.kind) {
            continue;
        }
        let FactTarget::Function { symbol } = &fact.target else {
            continue;
        };
        let Some((module_path, fn_span)) = lookup_function(air, symbol) else {
            continue;
        };
        let matched_pattern = section
            .validator_paths
            .iter()
            .find(|pat| matches_pattern(pat, module_path) || matches_pattern(pat, symbol));
        let Some(matched_pattern) = matched_pattern else {
            continue;
        };
        out.push(rm005_diagnostic(
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

fn is_io_fact_kind(kind: FactKind) -> bool {
    matches!(kind, FactKind::ExternalIo | FactKind::PersistenceWrite)
}

fn rm005_diagnostic(
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
        format!("module `{module_path}` matches validator_paths pattern `{matched_pattern}`"),
        format!("fact kind: {kind_label}"),
        format!("evidence: `{evidence}`"),
    ];
    for r in &fact.reasons {
        why.push(r.clone());
    }
    why.push(
        "validation and IO are different responsibilities — IO inside a \
         validator means the function fails differently for the same input \
         depending on external state"
            .into(),
    );
    Diagnostic {
        rule_id: "RM005".to_string(),
        severity: mode.elevate(Severity::Warning),
        span,
        concept: None,
        message: format!(
            "validator function `{symbol}` performs `{kind_label}` (`{evidence}`) — \
             validators must be pure decisions"
        ),
        why,
        suggested_fix: Some(format!(
            "split `{symbol}` into a pure-decision part (operates on inputs only) \
             and a separate function that does the IO. The validator should accept \
             already-fetched data, not fetch it itself. If `{module_path}` is not \
             actually a validator module, narrow `paradigms.RM.validator_paths` in \
             `.locus/lock.json`."
        )),
    }
}

/// RM006 — domain type method performs persistence write.
///
/// For every `FactKind::PersistenceWrite` fact whose `target` is a
/// `Function`, fire when the function symbol *looks like a method* — i.e.
/// it has at least three `::` segments and at least one segment whose
/// first character is uppercase (a TypeName) — AND the function's
/// enclosing file's `module_path` matches a pattern in `domain_paths_rm`.
/// A domain type whose methods perform persistence is mixing the model
/// with the storage concern.
///
/// Severity: Warning by default; Fatal under `--agent-strict`.
///
/// Silent when `domain_paths_rm` is empty.
///
/// Module-path resolution: the file's `module_path` is checked first; if
/// no match, the function symbol itself is matched against the same
/// patterns.
pub fn rm006(air: &AirWorkspace, section: &RmSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.domain_paths_rm.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for fact in &air.facts {
        if fact.kind != FactKind::PersistenceWrite {
            continue;
        }
        let FactTarget::Function { symbol } = &fact.target else {
            continue;
        };
        if !looks_like_method(symbol) {
            continue;
        }
        let Some((module_path, fn_span)) = lookup_function(air, symbol) else {
            continue;
        };
        let matched_pattern = section
            .domain_paths_rm
            .iter()
            .find(|pat| matches_pattern(pat, module_path) || matches_pattern(pat, symbol));
        let Some(matched_pattern) = matched_pattern else {
            continue;
        };
        out.push(rm006_diagnostic(
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

/// Heuristic: a function symbol is "method-shaped" when it has at least
/// three `::` segments (e.g. `pkg::Type::method`) AND at least one
/// segment past the first whose initial character is an ASCII uppercase
/// letter (the TypeName segment). Free functions are typically `pkg::fn`
/// or `pkg::module::fn` where every segment past the first starts with
/// lowercase, so the heuristic excludes them by default. Inline
/// `mod tests {}` adds a lowercase `tests` segment that doesn't trip the
/// uppercase check.
fn looks_like_method(symbol: &str) -> bool {
    let segs: Vec<&str> = symbol.split("::").collect();
    if segs.len() < 3 {
        return false;
    }
    // Skip the first segment (package name) — packages can be capitalised
    // in some ecosystems but the rule cares about method-on-type shape,
    // which appears later in the path.
    segs.iter()
        .skip(1)
        .take(segs.len() - 2) // exclude the last segment (the method name itself)
        .any(|seg| seg.chars().next().is_some_and(|c| c.is_ascii_uppercase()))
}

fn rm006_diagnostic(
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
    let evidence = fact.evidence.as_deref().unwrap_or("?");
    let mut why = vec![
        format!("module `{module_path}` matches domain_paths_rm pattern `{matched_pattern}`"),
        format!("symbol `{symbol}` is method-shaped (contains a TypeName segment)"),
        format!("persistence-write evidence: `{evidence}`"),
    ];
    for r in &fact.reasons {
        why.push(r.clone());
    }
    why.push(
        "domain methods that write to storage couple the model to the persistence \
         framework"
            .into(),
    );
    Diagnostic {
        rule_id: "RM006".to_string(),
        severity: mode.elevate(Severity::Warning),
        span,
        concept: None,
        message: format!(
            "domain method `{symbol}` performs persistence write — domain types \
             should be pure data shapes, not storage-aware"
        ),
        why,
        suggested_fix: Some(format!(
            "extract the persistence call into a separate `Repository` adapter \
             and keep the domain method on `{symbol}` pure (operates on `&self` \
             and returns a value). If `{module_path}` is not actually a domain \
             module, narrow `paradigms.RM.domain_paths_rm` in `.locus/lock.json`."
        )),
    }
}

// ── RuleDefinition impls (governance spine migration, epic #71) ──────────────

use crate::governance::finding::{FindingSource, RuleFinding};
use crate::governance::ids::{ParadigmId, RuleId};
use crate::governance::rule::{RuleContext, RuleDefinition};

const RM_PARADIGM: ParadigmId = ParadigmId::new("RM");
const RM001_ID: RuleId = RuleId::new("RM001");
const RM002_ID: RuleId = RuleId::new("RM002");
const RM003_ID: RuleId = RuleId::new("RM003");
const RM004_ID: RuleId = RuleId::new("RM004");
const RM005_ID: RuleId = RuleId::new("RM005");
const RM006_ID: RuleId = RuleId::new("RM006");

pub struct Rm001Rule;
pub static RM001_RULE: Rm001Rule = Rm001Rule;

impl RuleDefinition for Rm001Rule {
    fn id(&self) -> RuleId {
        RM001_ID
    }
    fn paradigm(&self) -> ParadigmId {
        RM_PARADIGM
    }
    fn title(&self) -> &'static str {
        "function performs too many distinct kinds of work"
    }
    fn default_severity(&self) -> crate::diagnostics::Severity {
        crate::diagnostics::Severity::Warning
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        use super::lockfile_schema::RmSection;
        let section: RmSection = ctx.lockfile.paradigm_section("RM").unwrap_or_default();
        rm001(ctx.air, &section, ctx.mode)
            .into_iter()
            .map(|d| RuleFinding {
                id: ctx.finding_ids.next(),
                source: FindingSource::RegisteredRule(RM001_ID),
                rule_id: Some(RM001_ID),
                paradigm_id: Some(RM_PARADIGM),
                default_severity: d.severity,
                span: Some(d.span),
                concept: d.concept,
                message: d.message,
                evidence: vec![],
                why: d.why,
                suggested_fix: d.suggested_fix,
                diagnostic_code: None,
            })
            .collect()
    }
}

pub struct Rm002Rule;
pub static RM002_RULE: Rm002Rule = Rm002Rule;

impl RuleDefinition for Rm002Rule {
    fn id(&self) -> RuleId {
        RM002_ID
    }
    fn paradigm(&self) -> ParadigmId {
        RM_PARADIGM
    }
    fn title(&self) -> &'static str {
        "converter performs a side-effect fact"
    }
    fn default_severity(&self) -> crate::diagnostics::Severity {
        crate::diagnostics::Severity::Warning
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        use super::lockfile_schema::RmSection;
        let section: RmSection = ctx.lockfile.paradigm_section("RM").unwrap_or_default();
        rm002(ctx.air, &section, ctx.mode)
            .into_iter()
            .map(|d| RuleFinding {
                id: ctx.finding_ids.next(),
                source: FindingSource::RegisteredRule(RM002_ID),
                rule_id: Some(RM002_ID),
                paradigm_id: Some(RM_PARADIGM),
                default_severity: d.severity,
                span: Some(d.span),
                concept: d.concept,
                message: d.message,
                evidence: vec![],
                why: d.why,
                suggested_fix: d.suggested_fix,
                diagnostic_code: None,
            })
            .collect()
    }
}

pub struct Rm003Rule;
pub static RM003_RULE: Rm003Rule = Rm003Rule;

impl RuleDefinition for Rm003Rule {
    fn id(&self) -> RuleId {
        RM003_ID
    }
    fn paradigm(&self) -> ParadigmId {
        RM_PARADIGM
    }
    fn title(&self) -> &'static str {
        "handler module containing branch-rich domain policy"
    }
    fn default_severity(&self) -> crate::diagnostics::Severity {
        crate::diagnostics::Severity::Warning
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        use super::lockfile_schema::RmSection;
        let section: RmSection = ctx.lockfile.paradigm_section("RM").unwrap_or_default();
        rm003(ctx.air, &section, ctx.mode)
            .into_iter()
            .map(|d| RuleFinding {
                id: ctx.finding_ids.next(),
                source: FindingSource::RegisteredRule(RM003_ID),
                rule_id: Some(RM003_ID),
                paradigm_id: Some(RM_PARADIGM),
                default_severity: d.severity,
                span: Some(d.span),
                concept: d.concept,
                message: d.message,
                evidence: vec![],
                why: d.why,
                suggested_fix: d.suggested_fix,
                diagnostic_code: None,
            })
            .collect()
    }
}

pub struct Rm004Rule;
pub static RM004_RULE: Rm004Rule = Rm004Rule;

impl RuleDefinition for Rm004Rule {
    fn id(&self) -> RuleId {
        RM004_ID
    }
    fn paradigm(&self) -> ParadigmId {
        RM_PARADIGM
    }
    fn title(&self) -> &'static str {
        "repository module containing branch-rich domain logic"
    }
    fn default_severity(&self) -> crate::diagnostics::Severity {
        crate::diagnostics::Severity::Warning
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        use super::lockfile_schema::RmSection;
        let section: RmSection = ctx.lockfile.paradigm_section("RM").unwrap_or_default();
        rm004(ctx.air, &section, ctx.mode)
            .into_iter()
            .map(|d| RuleFinding {
                id: ctx.finding_ids.next(),
                source: FindingSource::RegisteredRule(RM004_ID),
                rule_id: Some(RM004_ID),
                paradigm_id: Some(RM_PARADIGM),
                default_severity: d.severity,
                span: Some(d.span),
                concept: d.concept,
                message: d.message,
                evidence: vec![],
                why: d.why,
                suggested_fix: d.suggested_fix,
                diagnostic_code: None,
            })
            .collect()
    }
}

pub struct Rm005Rule;
pub static RM005_RULE: Rm005Rule = Rm005Rule;

impl RuleDefinition for Rm005Rule {
    fn id(&self) -> RuleId {
        RM005_ID
    }
    fn paradigm(&self) -> ParadigmId {
        RM_PARADIGM
    }
    fn title(&self) -> &'static str {
        "validator function performing IO"
    }
    fn default_severity(&self) -> crate::diagnostics::Severity {
        crate::diagnostics::Severity::Warning
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        use super::lockfile_schema::RmSection;
        let section: RmSection = ctx.lockfile.paradigm_section("RM").unwrap_or_default();
        rm005(ctx.air, &section, ctx.mode)
            .into_iter()
            .map(|d| RuleFinding {
                id: ctx.finding_ids.next(),
                source: FindingSource::RegisteredRule(RM005_ID),
                rule_id: Some(RM005_ID),
                paradigm_id: Some(RM_PARADIGM),
                default_severity: d.severity,
                span: Some(d.span),
                concept: d.concept,
                message: d.message,
                evidence: vec![],
                why: d.why,
                suggested_fix: d.suggested_fix,
                diagnostic_code: None,
            })
            .collect()
    }
}

pub struct Rm006Rule;
pub static RM006_RULE: Rm006Rule = Rm006Rule;

impl RuleDefinition for Rm006Rule {
    fn id(&self) -> RuleId {
        RM006_ID
    }
    fn paradigm(&self) -> ParadigmId {
        RM_PARADIGM
    }
    fn title(&self) -> &'static str {
        "domain type method performs persistence write"
    }
    fn default_severity(&self) -> crate::diagnostics::Severity {
        crate::diagnostics::Severity::Warning
    }
    fn observe(&self, ctx: &RuleContext<'_>) -> Vec<RuleFinding> {
        use super::lockfile_schema::RmSection;
        let section: RmSection = ctx.lockfile.paradigm_section("RM").unwrap_or_default();
        rm006(ctx.air, &section, ctx.mode)
            .into_iter()
            .map(|d| RuleFinding {
                id: ctx.finding_ids.next(),
                source: FindingSource::RegisteredRule(RM006_ID),
                rule_id: Some(RM006_ID),
                paradigm_id: Some(RM_PARADIGM),
                default_severity: d.severity,
                span: Some(d.span),
                concept: d.concept,
                message: d.message,
                evidence: vec![],
                why: d.why,
                suggested_fix: d.suggested_fix,
                diagnostic_code: None,
            })
            .collect()
    }
}

#[cfg(test)]
#[path = "rules_tests.rs"]
mod rules_tests;
