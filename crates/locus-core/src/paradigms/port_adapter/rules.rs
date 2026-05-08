//! PA rule implementations.
//!
//! Implemented:
//! - [`pa001`]: trait declared and immediately implemented in the same file
//!   (co-located port and adapter — the port wasn't actually abstracted).
//! - [`pa002`]: application/domain file imports a concrete adapter framework
//!   (`reqwest::*`, `sqlx::*`, …) — that's an adapter detail, not domain
//!   concern.
//! - [`pa003`]: application function performs an external-IO call directly
//!   instead of going through a declared port.
//! - [`pa004`]: an adapter type is constructed outside any composition
//!   root / bootstrap / composition module.

use std::collections::BTreeMap;

use locus_air::{
    ActionKind, AirFact, AirImplBlock, AirItem, AirSpan, AirWorkspace, FactKind, FactTarget,
    TypeKind,
};

use super::lockfile_schema::{PaSection, matches_pattern};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

/// PA001 — port and its sole impl in the same file.
///
/// A trait declared and immediately implemented in the same file is the
/// classic "I made a port to abstract this thing, but I never actually
/// abstracted anything" smell. Ports belong in `application::ports::*`,
/// adapters in `infrastructure::*` or boundary modules — physical separation
/// is the whole point of the port/adapter split.
///
/// Algorithm:
/// - For every `AirItem::Type` with `kind: TypeKind::Trait`, find its impls
///   by short name (last `::` segment of `trait_path`).
/// - If exactly one impl exists AND that impl's `span.file` equals the
///   trait's `span.file`, fire PA001.
/// - Skip if zero impls (intentionally-uninhabited trait — that's AB's
///   problem) or 2+ impls (already cross-file split, by definition).
/// - Skip if the trait's symbol or short name matches any pattern in
///   `accepted_colocated_traits`.
///
/// Severity: Warning by default; elevated to Fatal under `--agent-strict`.
pub fn pa001(air: &AirWorkspace, section: &PaSection, mode: CheckMode) -> Vec<Diagnostic> {
    let trait_to_impls = build_trait_to_impls(air);

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                let AirItem::Type(ty) = item else {
                    continue;
                };
                if ty.kind != TypeKind::Trait {
                    continue;
                }

                let impls = match trait_to_impls.get(ty.name.as_str()) {
                    Some(v) => v,
                    None => continue, // zero impls — intentionally-uninhabited
                };
                if impls.len() != 1 {
                    continue; // zero (handled above) or 2+ (already split)
                }
                let imp = impls[0];
                if imp.span.file != ty.span.file {
                    continue; // adapter already lives in a different file
                }

                if section
                    .accepted_colocated_traits
                    .iter()
                    .any(|pat| matches_pattern(pat, &ty.symbol) || matches_pattern(pat, &ty.name))
                {
                    continue;
                }

                out.push(Diagnostic {
                    rule_id: "PA001".to_string(),
                    severity: mode.elevate(Severity::Warning),
                    span: ty.span.clone(),
                    concept: None,
                    message: format!(
                        "trait `{}` and its only impl (`{}`) share file `{}`",
                        ty.name, imp.target_type, ty.span.file
                    ),
                    why: vec![
                        format!("trait `{}` declared in `{}`", ty.symbol, ty.span.file),
                        format!(
                            "sole impl is `impl {} for {}` in the same file",
                            ty.name, imp.target_type
                        ),
                        "no `accepted_colocated_traits` pattern matched".into(),
                    ],
                    suggested_fix: Some(format!(
                        "move `{}` to a ports module (typically `application::ports::*`) and the \
                         impl for `{}` to an adapter/infrastructure module; if this trait is a \
                         genuine utility helper rather than a port, accept it via \
                         `paradigms.PA.accepted_colocated_traits` in `locus.lock`",
                        ty.name, imp.target_type
                    )),
                });
            }
        }
    }
    out
}

/// Index every `AirItem::Impl` with a `trait_path` by the trait's short name
/// (last `::` segment). Inherent impls (`trait_path: None`) are excluded —
/// they aren't port implementations.
fn build_trait_to_impls(air: &AirWorkspace) -> BTreeMap<&str, Vec<&AirImplBlock>> {
    let mut out: BTreeMap<&str, Vec<&AirImplBlock>> = BTreeMap::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                let AirItem::Impl(imp) = item else {
                    continue;
                };
                let Some(tp) = imp.interface.as_deref() else {
                    continue;
                };
                let short = tp.rsplit("::").next().unwrap_or(tp);
                out.entry(short).or_default().push(imp);
            }
        }
    }
    out
}

/// PA002 — concrete adapter import in application/domain layer.
///
/// For each `AirItem::Import` in a file whose `module_path` matches a pattern
/// in `application_paths`, fire when the import's `path` matches a pattern in
/// `concrete_adapter_patterns`.
///
/// Severity: Fatal — application/domain code reaching directly into a
/// concrete adapter (`reqwest::Client`, `sqlx::PgPool`, …) breaks the
/// port/adapter split that PA defends. Same justification as BO001/DG001
/// for forbidden edges.
///
/// Silent until BOTH `application_paths` and `concrete_adapter_patterns`
/// are populated.
pub fn pa002(air: &AirWorkspace, section: &PaSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.application_paths.is_empty() || section.concrete_adapter_patterns.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            let Some(application_pattern) = section
                .application_paths
                .iter()
                .find(|pat| matches_pattern(pat, module_path))
            else {
                continue;
            };
            for item in &file.items {
                let AirItem::Import(imp) = item else {
                    continue;
                };
                let Some(adapter_pattern) = section
                    .concrete_adapter_patterns
                    .iter()
                    .find(|pat| matches_pattern(pat, &imp.path))
                else {
                    continue;
                };
                out.push(Diagnostic {
                    rule_id: "PA002".to_string(),
                    severity: mode.elevate(Severity::Fatal),
                    span: imp.span.clone(),
                    concept: None,
                    message: format!(
                        "application/domain module `{module_path}` imports concrete \
                         adapter `{}`",
                        imp.path
                    ),
                    why: vec![
                        format!(
                            "module `{module_path}` matches application_paths \
                             pattern `{application_pattern}`"
                        ),
                        format!(
                            "import `{}` matches concrete_adapter_patterns \
                             pattern `{adapter_pattern}`",
                            imp.path
                        ),
                        "application/domain code must depend on ports (traits), \
                         not concrete adapters; the adapter belongs at the \
                         boundary"
                            .into(),
                    ],
                    suggested_fix: Some(format!(
                        "introduce a port (trait) the application depends on, \
                         move the `{}` usage into an infrastructure adapter \
                         that implements the port; if the import is genuinely \
                         a non-adapter utility, narrow \
                         `paradigms.PA.concrete_adapter_patterns` in `locus.lock`",
                        imp.path
                    )),
                });
            }
        }
    }
    out
}

/// PA003 — application function performs an external-IO call without
/// going through a declared port.
///
/// For every `FactKind::ExternalIo` fact whose `target` is a `Function`,
/// resolve the function's enclosing module path and fire when that path
/// matches one of `application_paths`. The `std-rt` loader emits these
/// facts for `std::process::Command::*`, `std::net::*`, and similar
/// outbound primitives. Application code reaching directly into those
/// primitives bypasses the port layer entirely — same posture as PA002,
/// just enforced against runtime call-site evidence rather than imports.
///
/// Severity: Fatal — structural; agent-strict already elevates anything.
///
/// Silent when `application_paths` is empty: same opt-in UX as PA002.
///
/// Module-path resolution: the file's `module_path` is checked first; if
/// no match, the function symbol itself is matched against the same
/// patterns. This lets an `*::tests::*` carve-out cover inline `mod tests`
/// blocks that live at a deeper symbol path than the file.
pub fn pa003(air: &AirWorkspace, section: &PaSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.application_paths.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for fact in &air.facts {
        if fact.kind != FactKind::ExternalIo {
            continue;
        }
        let FactTarget::Function { symbol } = &fact.target else {
            continue;
        };
        let Some((module_path, fn_span)) = pa_lookup_function(air, symbol) else {
            continue;
        };
        let matched_pattern = section
            .application_paths
            .iter()
            .find(|pat| matches_pattern(pat, module_path) || matches_pattern(pat, symbol));
        let Some(matched_pattern) = matched_pattern else {
            continue;
        };
        out.push(pa003_diagnostic(
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

/// Resolve `symbol` against AIR. Returns the enclosing file's module_path
/// and the function's span. Mirrors `runtime_work::rules::lookup_function`.
fn pa_lookup_function<'a>(air: &'a AirWorkspace, symbol: &str) -> Option<(&'a str, AirSpan)> {
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

fn pa003_diagnostic(
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
        format!("module `{module_path}` matches application_paths pattern `{matched_pattern}`"),
        format!("external-IO evidence: `{evidence}`"),
    ];
    for r in &fact.reasons {
        why.push(r.clone());
    }
    why.push(
        "external IO must be abstracted behind a port (trait) and \
         implemented in an adapter, not called directly from application code"
            .into(),
    );
    Diagnostic {
        rule_id: "PA003".to_string(),
        severity: mode.elevate(Severity::Fatal),
        span,
        concept: None,
        message: format!(
            "application function `{symbol}` performs external IO `{evidence}` \
             without going through a declared port"
        ),
        why,
        suggested_fix: Some(format!(
            "introduce a port trait for the IO concept (e.g. `HttpClient`, \
             `ProcessRunner`, `Network`), implement it in an adapter module, \
             and inject the adapter through the composition root. The \
             application function `{symbol}` should depend on the trait, not \
             reach for `{evidence}` directly. If `{module_path}` is not \
             actually application code, narrow `paradigms.PA.application_paths` \
             in `locus.lock`."
        )),
    }
}

/// PA004 — adapter construction outside composition root.
///
/// For each `AirItem::TruthAction { action: Construct, target }`, fire when
/// `target` matches a pattern in `adapter_type_patterns` AND the action's
/// enclosing file (`AirFile.module_path`) does NOT match any pattern in
/// `accepted_construction_paths`.
///
/// Severity: Fatal — adapters constructed outside the composition root
/// undermine the whole point of having one.
///
/// Silent when `adapter_type_patterns` is empty. Defaults populate
/// `accepted_construction_paths` so the user only needs to opt in by listing
/// adapter types.
pub fn pa004(air: &AirWorkspace, section: &PaSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.adapter_type_patterns.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let module_path = file.module_path.as_deref().unwrap_or("");
            // If the file itself is an accepted construction path, skip
            // every action it contains.
            if section
                .accepted_construction_paths
                .iter()
                .any(|pat| matches_pattern(pat, module_path))
            {
                continue;
            }
            for item in &file.items {
                let AirItem::TruthAction(a) = item else {
                    continue;
                };
                if a.action != ActionKind::Construct {
                    continue;
                }
                let Some(adapter_pattern) = section
                    .adapter_type_patterns
                    .iter()
                    .find(|pat| matches_pattern(pat, &a.target))
                else {
                    continue;
                };
                let function_label = a
                    .function
                    .as_deref()
                    .unwrap_or("(no enclosing function recorded)");
                let module_label = if module_path.is_empty() {
                    "(unknown module)"
                } else {
                    module_path
                };
                out.push(Diagnostic {
                    rule_id: "PA004".to_string(),
                    severity: mode.elevate(Severity::Fatal),
                    span: a.span.clone(),
                    concept: None,
                    message: format!(
                        "adapter `{}` constructed in module `{module_label}` \
                         outside any accepted construction path",
                        a.target
                    ),
                    why: vec![
                        format!(
                            "target `{}` matches adapter_type_patterns pattern \
                             `{adapter_pattern}`",
                            a.target
                        ),
                        format!(
                            "module `{module_label}` matches none of the \
                             `accepted_construction_paths` patterns"
                        ),
                        format!("enclosing function: `{function_label}`"),
                    ],
                    suggested_fix: Some(format!(
                        "move the construction of `{}` into a composition \
                         root (e.g. `main`, a `bootstrap` module, or a \
                         declared `composition::*` module); if `{module_label}` \
                         is itself a legitimate composition site, add it to \
                         `paradigms.PA.accepted_construction_paths` in \
                         `locus.lock`",
                        a.target
                    )),
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use locus_air::{AIR_SCHEMA_VERSION, AirFile, AirPackage, AirSpan, AirType, Visibility};

    fn trait_item(name: &str, symbol: &str, file: &str, line: u32) -> AirItem {
        AirItem::Type(AirType {
            kind: TypeKind::Trait,
            name: name.into(),
            symbol: symbol.into(),
            visibility: Visibility::Public,
            fields: Vec::new(),
            variants: Vec::new(),
            decorators: Vec::new(),
            symbol_segments: Vec::new(),
            span: AirSpan::new(file, line, line),
            doc: None,
        })
    }

    fn impl_item(trait_path: Option<&str>, self_ty: &str, file: &str, line: u32) -> AirItem {
        AirItem::Impl(AirImplBlock {
            interface: trait_path.map(|s| s.to_string()),
            target_type: self_ty.into(),
            method_names: Vec::new(),
            dispatch: locus_air::ImplDispatch::Static,
            span: AirSpan::new(file, line, line),
        })
    }

    fn workspace(files: Vec<(&str, Vec<AirItem>)>) -> AirWorkspace {
        AirWorkspace {
            schema_version: AIR_SCHEMA_VERSION,
            packages: vec![AirPackage {
                name: "x".into(),
                version: "0".into(),
                root_dir: "/".into(),
                files: files
                    .into_iter()
                    .map(|(path, items)| AirFile {
                        path: path.into(),
                        module_path: Some(path.replace('/', "::").replace(".rs", "")),
                        items,
                        hints: Vec::new(),
                        parse_error: None,
                        line_count: 1,
                    })
                    .collect(),
            }],
            facts: Vec::new(),
        }
    }

    #[test]
    fn pa001_fires_when_trait_and_only_impl_share_file() {
        let air = workspace(vec![(
            "src/lib.rs",
            vec![
                trait_item("Clock", "x::Clock", "src/lib.rs", 10),
                impl_item(Some("x::Clock"), "SystemClock", "src/lib.rs", 20),
            ],
        )]);
        let diags = pa001(&air, &PaSection::default(), CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "PA001");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(diags[0].message.contains("Clock"));
        assert!(diags[0].message.contains("SystemClock"));
        assert!(diags[0].message.contains("src/lib.rs"));
    }

    #[test]
    fn pa001_quiet_when_impl_in_different_file() {
        let air = workspace(vec![
            (
                "src/ports.rs",
                vec![trait_item("Clock", "x::ports::Clock", "src/ports.rs", 10)],
            ),
            (
                "src/adapters.rs",
                vec![impl_item(
                    Some("x::ports::Clock"),
                    "SystemClock",
                    "src/adapters.rs",
                    5,
                )],
            ),
        ]);
        assert!(pa001(&air, &PaSection::default(), CheckMode::Human).is_empty());
    }

    #[test]
    fn pa001_quiet_when_trait_has_zero_impls() {
        let air = workspace(vec![(
            "src/lib.rs",
            vec![trait_item("Clock", "x::Clock", "src/lib.rs", 10)],
        )]);
        assert!(pa001(&air, &PaSection::default(), CheckMode::Human).is_empty());
    }

    #[test]
    fn pa001_quiet_when_trait_has_two_or_more_impls() {
        let air = workspace(vec![(
            "src/lib.rs",
            vec![
                trait_item("Clock", "x::Clock", "src/lib.rs", 10),
                impl_item(Some("x::Clock"), "SystemClock", "src/lib.rs", 20),
                impl_item(Some("x::Clock"), "TestClock", "src/lib.rs", 30),
            ],
        )]);
        assert!(pa001(&air, &PaSection::default(), CheckMode::Human).is_empty());
    }

    #[test]
    fn pa001_pattern_in_accepted_colocated_traits_exempts_trait() {
        let air = workspace(vec![(
            "src/lib.rs",
            vec![
                trait_item("Helper", "x::utils::Helper", "src/lib.rs", 10),
                impl_item(Some("x::utils::Helper"), "Thing", "src/lib.rs", 20),
            ],
        )]);
        let section = PaSection {
            accepted_colocated_traits: vec!["x::utils::*".into()],
            ..Default::default()
        };
        assert!(pa001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn pa001_short_name_pattern_exempts_trait() {
        // Short-name fallback: `Helper` matches the trait's `name` even when
        // its `symbol` is fully-qualified.
        let air = workspace(vec![(
            "src/lib.rs",
            vec![
                trait_item("Helper", "x::utils::Helper", "src/lib.rs", 10),
                impl_item(Some("x::utils::Helper"), "Thing", "src/lib.rs", 20),
            ],
        )]);
        let section = PaSection {
            accepted_colocated_traits: vec!["Helper".into()],
            ..Default::default()
        };
        assert!(pa001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn pa001_inherent_impls_are_not_counted() {
        // Inherent `impl Foo` (no trait) must not count toward the "sole
        // impl" tally — otherwise a trait with zero trait-impls but one
        // inherent impl on the self type would falsely fire.
        let air = workspace(vec![(
            "src/lib.rs",
            vec![
                trait_item("Clock", "x::Clock", "src/lib.rs", 10),
                impl_item(None, "Clock", "src/lib.rs", 20), // inherent — ignored
            ],
        )]);
        assert!(pa001(&air, &PaSection::default(), CheckMode::Human).is_empty());
    }

    #[test]
    fn pa001_agent_strict_elevates_to_fatal() {
        let air = workspace(vec![(
            "src/lib.rs",
            vec![
                trait_item("Clock", "x::Clock", "src/lib.rs", 10),
                impl_item(Some("x::Clock"), "SystemClock", "src/lib.rs", 20),
            ],
        )]);
        let diags = pa001(&air, &PaSection::default(), CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    #[test]
    fn pa001_matches_impl_by_trait_short_name() {
        // Trait's symbol may be `x::ports::Clock` while impl's `trait_path`
        // is the same fully-qualified path. The matcher uses the short name
        // (last `::` segment) so both line up.
        let air = workspace(vec![(
            "src/lib.rs",
            vec![
                trait_item("Clock", "x::ports::Clock", "src/lib.rs", 10),
                impl_item(Some("x::ports::Clock"), "SystemClock", "src/lib.rs", 20),
            ],
        )]);
        let diags = pa001(&air, &PaSection::default(), CheckMode::Human);
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn pa001_diagnostic_includes_why_and_fix() {
        let air = workspace(vec![(
            "src/lib.rs",
            vec![
                trait_item("Clock", "x::Clock", "src/lib.rs", 10),
                impl_item(Some("x::Clock"), "SystemClock", "src/lib.rs", 20),
            ],
        )]);
        let diags = pa001(&air, &PaSection::default(), CheckMode::Human);
        assert_eq!(diags.len(), 1);
        let d = &diags[0];
        assert!(d.why.iter().any(|w| w.contains("declared in")));
        assert!(d.why.iter().any(|w| w.contains("sole impl")));
        assert!(
            d.why
                .iter()
                .any(|w| w.contains("accepted_colocated_traits"))
        );
        let fix = d.suggested_fix.as_deref().unwrap_or("");
        assert!(fix.contains("ports"));
        assert!(fix.contains("accepted_colocated_traits"));
    }

    // ----- PA002 / PA004 helpers -----

    fn import_item(path: &str, file: &str, line: u32) -> AirItem {
        use locus_air::AirImport;
        AirItem::Import(AirImport {
            path: path.into(),
            path_segments: Vec::new(),
            visibility: Visibility::Private,
            span: AirSpan::new(file, line, line),
        })
    }

    fn construct_action(target: &str, function: &str, file: &str, line: u32) -> AirItem {
        use locus_air::AirTruthAction;
        AirItem::TruthAction(AirTruthAction {
            action: ActionKind::Construct,
            target: target.into(),
            function: Some(function.into()),
            span: AirSpan::new(file, line, line),
            confidence: 0.95,
            reasons: vec!["struct literal".into()],
        })
    }

    fn workspace_with_module(module_path: &str, file: &str, items: Vec<AirItem>) -> AirWorkspace {
        AirWorkspace {
            schema_version: AIR_SCHEMA_VERSION,
            packages: vec![AirPackage {
                name: "x".into(),
                version: "0".into(),
                root_dir: "/".into(),
                files: vec![AirFile {
                    path: file.into(),
                    module_path: Some(module_path.into()),
                    items,
                    hints: Vec::new(),
                    parse_error: None,
                    line_count: 1,
                }],
            }],
            facts: Vec::new(),
        }
    }

    // ----- PA002 -----

    #[test]
    fn pa002_fires_when_application_imports_concrete_adapter() {
        let air = workspace_with_module(
            "crate::application::user",
            "src/app.rs",
            vec![import_item("reqwest::Client", "src/app.rs", 4)],
        );
        let section = PaSection {
            application_paths: vec!["crate::application::*".into()],
            concrete_adapter_patterns: vec!["reqwest::*".into()],
            ..Default::default()
        };
        let diags = pa002(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "PA002");
        assert_eq!(diags[0].severity, Severity::Fatal);
        assert!(diags[0].message.contains("reqwest::Client"));
        assert!(
            diags[0]
                .why
                .iter()
                .any(|w| w.contains("crate::application::*"))
        );
    }

    #[test]
    fn pa002_quiet_when_import_outside_application_layer() {
        // Infrastructure layer is allowed to import concrete adapters.
        let air = workspace_with_module(
            "crate::infrastructure::http_client",
            "src/inf.rs",
            vec![import_item("reqwest::Client", "src/inf.rs", 1)],
        );
        let section = PaSection {
            application_paths: vec!["crate::application::*".into()],
            concrete_adapter_patterns: vec!["reqwest::*".into()],
            ..Default::default()
        };
        assert!(pa002(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn pa002_silent_when_application_paths_empty() {
        let air = workspace_with_module(
            "crate::application::user",
            "src/app.rs",
            vec![import_item("reqwest::Client", "src/app.rs", 1)],
        );
        let section = PaSection {
            application_paths: vec![],
            concrete_adapter_patterns: vec!["reqwest::*".into()],
            ..Default::default()
        };
        assert!(pa002(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn pa002_silent_when_concrete_adapter_patterns_empty() {
        let air = workspace_with_module(
            "crate::application::user",
            "src/app.rs",
            vec![import_item("reqwest::Client", "src/app.rs", 1)],
        );
        let section = PaSection {
            application_paths: vec!["crate::application::*".into()],
            concrete_adapter_patterns: vec![],
            ..Default::default()
        };
        assert!(pa002(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn pa002_quiet_when_application_imports_non_adapter_path() {
        let air = workspace_with_module(
            "crate::application::user",
            "src/app.rs",
            vec![import_item("crate::domain::User", "src/app.rs", 1)],
        );
        let section = PaSection {
            application_paths: vec!["crate::application::*".into()],
            concrete_adapter_patterns: vec!["sqlx::*".into(), "reqwest::*".into()],
            ..Default::default()
        };
        assert!(pa002(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn pa002_agent_strict_keeps_fatal() {
        let air = workspace_with_module(
            "crate::application::user",
            "src/app.rs",
            vec![import_item("sqlx::PgPool", "src/app.rs", 1)],
        );
        let section = PaSection {
            application_paths: vec!["crate::application::*".into()],
            concrete_adapter_patterns: vec!["sqlx::*".into()],
            ..Default::default()
        };
        let diags = pa002(&air, &section, CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    // ----- PA004 -----

    #[test]
    fn pa004_fires_when_adapter_constructed_outside_root() {
        let air = workspace_with_module(
            "crate::handler",
            "src/handler.rs",
            vec![construct_action(
                "PgUserRepository",
                "crate::handler::create_user",
                "src/handler.rs",
                12,
            )],
        );
        let section = PaSection {
            adapter_type_patterns: vec!["*::PgUserRepository".into()],
            ..Default::default()
        };
        let diags = pa004(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "PA004");
        assert_eq!(diags[0].severity, Severity::Fatal);
        assert!(diags[0].message.contains("PgUserRepository"));
        assert!(diags[0].message.contains("crate::handler"));
    }

    #[test]
    fn pa004_quiet_when_constructed_inside_default_main() {
        let air = workspace_with_module(
            "crate::main",
            "src/main.rs",
            vec![construct_action(
                "PgUserRepository",
                "crate::main::main",
                "src/main.rs",
                3,
            )],
        );
        let section = PaSection {
            adapter_type_patterns: vec!["*::PgUserRepository".into()],
            ..Default::default()
        };
        assert!(pa004(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn pa004_quiet_inside_bootstrap_module() {
        let air = workspace_with_module(
            "crate::bootstrap::wire",
            "src/bootstrap/wire.rs",
            vec![construct_action(
                "PgUserRepository",
                "crate::bootstrap::wire::build",
                "src/bootstrap/wire.rs",
                4,
            )],
        );
        let section = PaSection {
            adapter_type_patterns: vec!["*::PgUserRepository".into()],
            ..Default::default()
        };
        assert!(pa004(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn pa004_silent_when_adapter_type_patterns_empty() {
        let air = workspace_with_module(
            "crate::handler",
            "src/handler.rs",
            vec![construct_action(
                "PgUserRepository",
                "crate::handler::create_user",
                "src/handler.rs",
                12,
            )],
        );
        let section = PaSection::default();
        assert!(pa004(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn pa004_user_supplied_construction_paths_override_default() {
        // Override the default `*::main` etc. with `crate::wire` only;
        // construction in `main` should now fire.
        let air = workspace_with_module(
            "crate::main",
            "src/main.rs",
            vec![construct_action(
                "PgUserRepository",
                "crate::main::main",
                "src/main.rs",
                3,
            )],
        );
        let section = PaSection {
            adapter_type_patterns: vec!["*::PgUserRepository".into()],
            accepted_construction_paths: vec!["crate::wire".into()],
            ..Default::default()
        };
        let diags = pa004(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn pa004_quiet_when_target_does_not_match_adapter_pattern() {
        let air = workspace_with_module(
            "crate::handler",
            "src/handler.rs",
            vec![construct_action(
                "User",
                "crate::handler::create_user",
                "src/handler.rs",
                7,
            )],
        );
        let section = PaSection {
            adapter_type_patterns: vec!["*::PgUserRepository".into()],
            ..Default::default()
        };
        assert!(pa004(&air, &section, CheckMode::Human).is_empty());
    }

    // ----- PA003 -----

    fn func_item(symbol: &str, file: &str, line: u32) -> AirItem {
        use locus_air::AirFunction;
        AirItem::Function(AirFunction {
            name: symbol.rsplit("::").next().unwrap_or(symbol).into(),
            symbol: symbol.into(),
            visibility: Visibility::Public,
            params: Vec::new(),
            return_type: None,
            span: AirSpan::new(file, line, line + 5),
            line_count: 6,
            decorators: Vec::new(),
            symbol_segments: Vec::new(),
            doc: None,
        })
    }

    fn external_io_fact(symbol: &str, evidence: &str, reason: &str) -> AirFact {
        AirFact {
            kind: FactKind::ExternalIo,
            target: FactTarget::Function {
                symbol: symbol.into(),
            },
            source: "test".into(),
            confidence: 1.0,
            reasons: vec![reason.into()],
            evidence: Some(evidence.into()),
        }
    }

    fn workspace_with_module_facts(
        module_path: &str,
        file: &str,
        items: Vec<AirItem>,
        facts: Vec<AirFact>,
    ) -> AirWorkspace {
        AirWorkspace {
            schema_version: AIR_SCHEMA_VERSION,
            packages: vec![AirPackage {
                name: "x".into(),
                version: "0".into(),
                root_dir: "/".into(),
                files: vec![AirFile {
                    path: file.into(),
                    module_path: Some(module_path.into()),
                    items,
                    hints: Vec::new(),
                    parse_error: None,
                    line_count: 1,
                }],
            }],
            facts,
        }
    }

    #[test]
    fn pa003_fires_on_external_io_in_application_path() {
        let air = workspace_with_module_facts(
            "crate::application::user",
            "src/app.rs",
            vec![func_item(
                "crate::application::user::create",
                "src/app.rs",
                8,
            )],
            vec![external_io_fact(
                "crate::application::user::create",
                "std::process::Command::output",
                "std::process::Command::output is external IO",
            )],
        );
        let section = PaSection {
            application_paths: vec!["crate::application::*".into()],
            ..Default::default()
        };
        let diags = pa003(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        let d = &diags[0];
        assert_eq!(d.rule_id, "PA003");
        assert_eq!(d.severity, Severity::Fatal);
        assert_eq!(d.span.line_start, 8);
        assert!(d.message.contains("crate::application::user::create"));
        assert!(d.message.contains("std::process::Command::output"));
        assert!(
            d.why.iter().any(|w| w.contains("crate::application::*")),
            "expected matched pattern in why; got {:?}",
            d.why
        );
        assert!(
            d.why
                .iter()
                .any(|w| w.contains("external IO must be abstracted")),
            "expected port-rationale why; got {:?}",
            d.why
        );
    }

    #[test]
    fn pa003_quiet_when_function_outside_application_paths() {
        let air = workspace_with_module_facts(
            "crate::infrastructure::cmd",
            "src/inf.rs",
            vec![func_item(
                "crate::infrastructure::cmd::run",
                "src/inf.rs",
                4,
            )],
            vec![external_io_fact(
                "crate::infrastructure::cmd::run",
                "std::process::Command::output",
                "external IO",
            )],
        );
        let section = PaSection {
            application_paths: vec!["crate::application::*".into()],
            ..Default::default()
        };
        assert!(pa003(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn pa003_quiet_on_other_fact_kinds() {
        let air = workspace_with_module_facts(
            "crate::application::user",
            "src/app.rs",
            vec![func_item(
                "crate::application::user::create",
                "src/app.rs",
                8,
            )],
            vec![
                AirFact {
                    kind: FactKind::Logging,
                    target: FactTarget::Function {
                        symbol: "crate::application::user::create".into(),
                    },
                    source: "test".into(),
                    confidence: 1.0,
                    reasons: Vec::new(),
                    evidence: Some("tracing::info!".into()),
                },
                AirFact {
                    kind: FactKind::PersistenceWrite,
                    target: FactTarget::Function {
                        symbol: "crate::application::user::create".into(),
                    },
                    source: "test".into(),
                    confidence: 1.0,
                    reasons: Vec::new(),
                    evidence: Some("std::fs::write".into()),
                },
                AirFact {
                    kind: FactKind::BlockingCall,
                    target: FactTarget::Function {
                        symbol: "crate::application::user::create".into(),
                    },
                    source: "test".into(),
                    confidence: 1.0,
                    reasons: Vec::new(),
                    evidence: Some("std::thread::sleep".into()),
                },
            ],
        );
        let section = PaSection {
            application_paths: vec!["crate::application::*".into()],
            ..Default::default()
        };
        assert!(pa003(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn pa003_silent_when_application_paths_empty() {
        let air = workspace_with_module_facts(
            "crate::application::user",
            "src/app.rs",
            vec![func_item(
                "crate::application::user::create",
                "src/app.rs",
                8,
            )],
            vec![external_io_fact(
                "crate::application::user::create",
                "std::net::TcpStream::connect",
                "external IO",
            )],
        );
        let section = PaSection::default();
        assert!(pa003(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn pa003_agent_strict_keeps_fatal() {
        let air = workspace_with_module_facts(
            "crate::application::user",
            "src/app.rs",
            vec![func_item(
                "crate::application::user::create",
                "src/app.rs",
                8,
            )],
            vec![external_io_fact(
                "crate::application::user::create",
                "std::process::Command::output",
                "external IO",
            )],
        );
        let section = PaSection {
            application_paths: vec!["crate::application::*".into()],
            ..Default::default()
        };
        let diags = pa003(&air, &section, CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    #[test]
    fn pa003_per_symbol_test_module_carve_out_via_symbol_match() {
        // The function lives in file `crate::application::user`, and it's
        // also matched by `crate::application::*`. But if the user has a
        // narrower opt-in restricted to non-test paths via a non-overlapping
        // pattern like `crate::application::user::api::*`, the file isn't
        // matched and nothing fires. More important: an inline `tests`
        // module's function symbol embeds `::tests::` — a check against
        // `application_paths` should still match the symbol if the user
        // wrote a pattern like `*::application::*`, but a per-symbol exempt
        // (a `*::tests::*` pattern in `application_paths` would be the
        // wrong direction). Instead, we model "test module under
        // application" by ensuring that when application_paths contains
        // `crate::application::*` and the function symbol is
        // `crate::application::user::tests::it_works` (file module path is
        // `crate::application::user::tests` for an inline mod), the rule
        // still fires — i.e. symbol-anywhere matching reaches inline test
        // modules just like the file path does. The carve-out, when
        // desired, is naturally expressed by *not* including
        // `*::tests::*` style files in `application_paths`.
        let air = workspace_with_module_facts(
            "crate::application::user::tests",
            "src/app.rs",
            vec![func_item(
                "crate::application::user::tests::it_works",
                "src/app.rs",
                30,
            )],
            vec![external_io_fact(
                "crate::application::user::tests::it_works",
                "std::process::Command::output",
                "external IO",
            )],
        );
        // User pinned application paths to non-test sub-paths only — tests
        // module is intentionally excluded.
        let section = PaSection {
            application_paths: vec!["crate::application::user::api::*".into()],
            ..Default::default()
        };
        assert!(
            pa003(&air, &section, CheckMode::Human).is_empty(),
            "tests module not in application_paths must not fire"
        );
    }

    #[test]
    fn pa003_matches_via_function_symbol_when_module_path_is_unrelated() {
        // File-level module_path doesn't match (file is in
        // `crate::other`), but the function symbol embeds the application
        // path because it's an inline submodule. Same fix RW001 already
        // applied.
        let air = workspace_with_module_facts(
            "crate::other",
            "src/other.rs",
            vec![func_item(
                "crate::application::user::create",
                "src/other.rs",
                10,
            )],
            vec![external_io_fact(
                "crate::application::user::create",
                "std::process::Command::output",
                "external IO",
            )],
        );
        let section = PaSection {
            application_paths: vec!["crate::application::*".into()],
            ..Default::default()
        };
        // Symbol-anywhere fallback must catch it.
        let diags = pa003(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1, "symbol fallback should match");
    }
}
