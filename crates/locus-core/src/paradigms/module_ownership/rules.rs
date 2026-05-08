//! MO rule implementations.
//!
//! Implemented:
//! - [`mo001`]: too many public top-level types in a single file.
//! - [`mo002`]: responsibility entropy in a single file (canonical/boundary/
//!   converter hints, handler-named functions, persistence imports, io call
//!   sites — too many distinct architectural roles co-existing).
//! - [`mo003`]: canonical hint co-located with a boundary hint in the same file.
//! - [`mo004`]: canonical hint co-located with a handler-named function in the
//!   same file.

use locus_air::{
    AirFile, AirHint, AirImport, AirItem, AirSpan, AirWorkspace, HintKind, Visibility,
};

use super::lockfile_schema::{MoSection, matches_name_glob, matches_pattern};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

/// MO001 — module file has too many public top-level types.
///
/// For each `AirFile` with a `module_path`, count `AirItem::Type` items
/// whose visibility is `Public`. Compare against the file's effective
/// budget:
/// - if the file's `module_path` matches an override's `module` pattern,
///   the override's `max_public_types` wins;
/// - otherwise the section's `default_max_public_types` (or the constant
///   fallback) is used.
///
/// One diagnostic per file (not per type) — the violation is the file's
/// responsibility, not any individual type.
///
/// Severity: Warning by default. `--agent-strict` elevates to Fatal via
/// [`CheckMode::elevate`].
///
/// Lockfile-driven silence: when the section is fully default (no
/// `default_max_public_types` set AND no overrides), MO001 emits nothing.
/// Same convention as the other lockfile-driven rules — pre-onboarding,
/// we don't have the user's intent and shouldn't fire on un-configured
/// projects.
pub fn mo001(air: &AirWorkspace, section: &MoSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.default_max_public_types.is_none() && section.overrides.is_empty() {
        return Vec::new();
    }
    let default_budget = section.effective_default();
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            let count = file
                .items
                .iter()
                .filter(
                    |item| matches!(item, AirItem::Type(t) if t.visibility == Visibility::Public),
                )
                .count() as u32;

            let matched_override = section.matching_override(module_path);
            let budget = matched_override
                .map(|o| o.max_public_types)
                .unwrap_or(default_budget);
            if count <= budget {
                continue;
            }

            // Anchor the diagnostic at the file's first public type when
            // possible — otherwise at line 1 of the file. Either way, the
            // diagnostic is per-file, not per-type.
            let span = file
                .items
                .iter()
                .find_map(|item| match item {
                    AirItem::Type(t) if t.visibility == Visibility::Public => Some(t.span.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| locus_air::AirSpan::new(file.path.clone(), 1, 1));

            let mut why = vec![
                format!("file `{module_path}` defines {count} public top-level type(s)"),
                if let Some(o) = matched_override {
                    format!("budget {budget} from override `module = {}`", o.module)
                } else {
                    format!("budget {budget} (workspace default)")
                },
            ];
            if matched_override.is_none() && section.default_max_public_types.is_none() {
                why.push(format!(
                    "no `default_max_public_types` configured; using built-in fallback {}",
                    default_budget
                ));
            }

            out.push(Diagnostic {
                rule_id: "MO001".to_string(),
                severity: mode.elevate(Severity::Warning),
                span,
                concept: None,
                message: format!(
                    "module `{module_path}` has {count} public top-level types (budget {budget})"
                ),
                why,
                suggested_fix: Some(
                    "split the module into submodules each owning one architectural role, \
                     or — if this density is intended (e.g. an API surface) — raise the \
                     budget by adding an override to `paradigms.MO.overrides` in \
                     `locus.lock`"
                        .into(),
                ),
            });
        }
    }
    out
}

/// In-code default callee patterns flagged as "io" by MO002. Kept as a
/// constant rather than a lockfile field because MO002's spec only enumerates
/// `entropy_threshold`, `handler_name_patterns`, and `persistence_import_patterns`
/// as configurable surface — the io contributor is a built-in heuristic, not
/// user policy. If a project legitimately makes io calls in a non-blob file,
/// the noise is absorbed by the entropy threshold (count must reach 3+).
const IO_CALLEE_PATTERNS: &[&str] = &[
    "*::fs::*",
    "*::net::*",
    "*::TcpStream::*",
    "*::TcpListener::*",
    "*::UdpSocket::*",
];

/// Anchor a per-file diagnostic at a useful span: the first item in the
/// file when present, otherwise line 1 of the file.
fn file_anchor_span(file: &AirFile) -> AirSpan {
    file.items
        .iter()
        .map(|item| match item {
            AirItem::Type(t) => t.span.clone(),
            AirItem::Function(f) => f.span.clone(),
            AirItem::Conversion(c) => c.span.clone(),
            AirItem::Import(i) => i.span.clone(),
            AirItem::Impl(i) => i.span.clone(),
            AirItem::TruthAction(a) => a.span.clone(),
            AirItem::CallSite(c) => c.span.clone(),
            AirItem::Usage(u) => u.span.clone(),
            AirItem::SilentDiscard(d) => d.span.clone(),
            AirItem::PartialResultMatch(p) => p.span.clone(),
            AirItem::MatchArm(a) => a.span.clone(),
            AirItem::ClosureMethodCall(c) => c.span.clone(),
            AirItem::FallbackCall(c) => c.span.clone(),
            AirItem::RetryLoop(l) => l.span.clone(),
            AirItem::ScrutineeLiteral(l) => l.span.clone(),
        })
        .next()
        .unwrap_or_else(|| AirSpan::new(file.path.clone(), 1, 1))
}

fn has_canonical_hint(file: &AirFile) -> bool {
    file.hints
        .iter()
        .any(|h| matches!(h.kind, HintKind::Canonical))
}

fn has_boundary_hint(file: &AirFile) -> bool {
    file.hints
        .iter()
        .any(|h| matches!(h.kind, HintKind::Boundary { .. }))
}

fn has_converter_hint(file: &AirFile) -> bool {
    file.hints
        .iter()
        .any(|h| matches!(h.kind, HintKind::Converter))
}

fn has_handler_named_function(file: &AirFile, patterns: &[&str]) -> bool {
    file.items.iter().any(|item| {
        let AirItem::Function(f) = item else {
            return false;
        };
        patterns.iter().any(|p| matches_name_glob(p, &f.name))
    })
}

fn has_persistence_import(file: &AirFile, patterns: &[&str]) -> bool {
    file.items.iter().any(|item| {
        let AirItem::Import(AirImport { path, .. }) = item else {
            return false;
        };
        patterns.iter().any(|p| matches_pattern(p, path))
    })
}

fn has_io_call_site(file: &AirFile) -> bool {
    file.items.iter().any(|item| {
        let AirItem::CallSite(c) = item else {
            return false;
        };
        IO_CALLEE_PATTERNS
            .iter()
            .any(|p| matches_pattern(p, &c.callee))
    })
}

/// MO002 — responsibility entropy in a single file.
///
/// Counts the number of distinct architectural roles a file carries:
/// (a) `AirHint::Canonical` present, (b) `AirHint::Boundary` present,
/// (c) `AirHint::Converter` present, (d) any function whose name matches
/// `handler_name_patterns` (default `*_handler`/`handle_*`), (e) any
/// `AirImport.path` matching `persistence_import_patterns`, (f) any
/// `AirItem::CallSite.callee` matching the built-in io pattern set.
///
/// Fires when the count `>= entropy_threshold` (default 3).
///
/// Severity: Warning by default; `--agent-strict` elevates to Fatal.
///
/// Lockfile-driven silence: when the section is fully default (none of
/// `default_max_public_types`, `entropy_threshold`, `handler_name_patterns`,
/// `persistence_import_patterns`, or `overrides` configured), MO002 emits
/// nothing — same convention as MO001 and the DG/UT lockfile-driven rules.
pub fn mo002(air: &AirWorkspace, section: &MoSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.default_max_public_types.is_none()
        && section.entropy_threshold.is_none()
        && section.handler_name_patterns.is_empty()
        && section.persistence_import_patterns.is_empty()
        && section.overrides.is_empty()
    {
        return Vec::new();
    }
    let threshold = section.effective_entropy_threshold();
    let handler_patterns = section.effective_handler_name_patterns();
    let persistence_patterns = section.effective_persistence_import_patterns();
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            let mut roles: Vec<&'static str> = Vec::new();
            if has_canonical_hint(file) {
                roles.push("canonical");
            }
            if has_boundary_hint(file) {
                roles.push("boundary");
            }
            if has_converter_hint(file) {
                roles.push("converter");
            }
            if has_handler_named_function(file, &handler_patterns) {
                roles.push("handler");
            }
            if has_persistence_import(file, &persistence_patterns) {
                roles.push("persistence");
            }
            if has_io_call_site(file) {
                roles.push("io");
            }
            let count = roles.len() as u32;
            if count < threshold {
                continue;
            }
            let span = file_anchor_span(file);
            let role_list = roles.join(", ");
            out.push(Diagnostic {
                rule_id: "MO002".to_string(),
                severity: mode.elevate(Severity::Warning),
                span,
                concept: None,
                message: format!(
                    "module `{module_path}` carries {count} distinct architectural roles \
                     ({role_list}); threshold is {threshold}"
                ),
                why: vec![
                    format!("file `{module_path}` exhibits roles: {role_list}"),
                    format!(
                        "MO002 entropy threshold is {threshold} (configured via \
                         `paradigms.MO.entropy_threshold` in `locus.lock`)"
                    ),
                    "a single file mixing canonical/boundary/converter/handler/persistence/io \
                     roles is a responsibility blob — split each role into its own module"
                        .into(),
                ],
                suggested_fix: Some(
                    "split this file along role boundaries: canonical types into \
                     `domain/`, boundary DTOs into `dto/`, conversions into a \
                     `convert.rs`, handlers into a `handlers/` module, and \
                     persistence/io into an adapter layer. If the density is \
                     intentional, raise `paradigms.MO.entropy_threshold` in \
                     `locus.lock`."
                        .into(),
                ),
            });
        }
    }
    out
}

/// MO003 — canonical type co-located with a boundary type in the same file.
///
/// Fires for any `AirFile` containing both an `AirHint::Canonical` and an
/// `AirHint::Boundary`. The two hints describe opposing roles — canonical
/// types are the domain truth; boundary types are the wire/protocol shadow
/// of that truth — so co-locating them in one file blurs ownership.
///
/// Severity: Warning by default; `--agent-strict` elevates to Fatal. No
/// new lockfile fields — the rule is a pure structural check on hints.
pub fn mo003(air: &AirWorkspace, mode: CheckMode) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            if !(has_canonical_hint(file) && has_boundary_hint(file)) {
                continue;
            }
            let span = file
                .hints
                .iter()
                .find(|h| matches!(h.kind, HintKind::Canonical))
                .map(|h: &AirHint| h.span.clone())
                .unwrap_or_else(|| file_anchor_span(file));
            out.push(Diagnostic {
                rule_id: "MO003".to_string(),
                severity: mode.elevate(Severity::Warning),
                span,
                concept: None,
                message: format!(
                    "module `{module_path}` mixes canonical and boundary types"
                ),
                why: vec![
                    format!("file `{module_path}` has both a `// ot: canonical` and a `// ot: boundary` hint"),
                    "canonical types are the domain truth; boundary types are the \
                     wire/protocol shadow of that truth — keeping them in one file \
                     blurs ownership and makes the converter direction ambiguous"
                        .into(),
                ],
                suggested_fix: Some(
                    "split the file: move canonical types into a `domain/` module \
                     and boundary types into a `dto/` module, with explicit \
                     `From`/`TryFrom` converters between them"
                        .into(),
                ),
            });
        }
    }
    out
}

/// MO004 — handler co-located with a canonical concept in the same file.
///
/// Fires for any `AirFile` containing both an `AirHint::Canonical` *and* a
/// function whose name matches `handler_name_patterns` (reuses the same
/// patterns as MO002, with the same default fallback `*_handler`/`handle_*`).
///
/// Handlers belong to an application/transport layer; canonical types belong
/// to the domain layer. Co-locating them couples the two.
///
/// Severity: Warning by default; `--agent-strict` elevates to Fatal.
pub fn mo004(air: &AirWorkspace, section: &MoSection, mode: CheckMode) -> Vec<Diagnostic> {
    let handler_patterns = section.effective_handler_name_patterns();
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            if !has_canonical_hint(file) {
                continue;
            }
            let handler = file.items.iter().find_map(|item| {
                let AirItem::Function(f) = item else {
                    return None;
                };
                if handler_patterns
                    .iter()
                    .any(|p| matches_name_glob(p, &f.name))
                {
                    Some(f)
                } else {
                    None
                }
            });
            let Some(handler) = handler else {
                continue;
            };
            out.push(Diagnostic {
                rule_id: "MO004".to_string(),
                severity: mode.elevate(Severity::Warning),
                span: handler.span.clone(),
                concept: None,
                message: format!(
                    "module `{module_path}` co-locates handler `{}` with a canonical concept",
                    handler.name
                ),
                why: vec![
                    format!("file `{module_path}` has a `// ot: canonical` hint"),
                    format!(
                        "function `{}` matches handler name pattern (one of {:?})",
                        handler.name, handler_patterns
                    ),
                    "handlers belong to an application/transport layer; canonical \
                     types belong to the domain layer — co-locating them couples \
                     the two and makes the canonical reusable from non-handler \
                     callers harder"
                        .into(),
                ],
                suggested_fix: Some(format!(
                    "move `{}` into a `handlers/` module that depends on the \
                     canonical, instead of defining both in the same file. If the \
                     name match is a false positive, narrow \
                     `paradigms.MO.handler_name_patterns` in `locus.lock`.",
                    handler.name
                )),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::lockfile_schema::{MoOverride, MoSection};
    use super::*;
    use locus_air::{
        AIR_SCHEMA_VERSION, AirCallSite, AirFunction, AirPackage, AirType, CallKind, TypeKind,
    };

    fn pub_type(name: &str) -> AirItem {
        AirItem::Type(AirType {
            kind: TypeKind::Struct,
            name: name.into(),
            symbol: format!("x::{name}"),
            visibility: Visibility::Public,
            fields: Vec::new(),
            variants: Vec::new(),
            decorators: Vec::new(),
            symbol_segments: Vec::new(),
            span: AirSpan::new("t.rs", 1, 1),
            doc: None,
        })
    }

    fn priv_type(name: &str) -> AirItem {
        AirItem::Type(AirType {
            kind: TypeKind::Struct,
            name: name.into(),
            symbol: format!("x::{name}"),
            visibility: Visibility::Private,
            fields: Vec::new(),
            variants: Vec::new(),
            decorators: Vec::new(),
            symbol_segments: Vec::new(),
            span: AirSpan::new("t.rs", 1, 1),
            doc: None,
        })
    }

    fn air_with(module: Option<&str>, items: Vec<AirItem>) -> AirWorkspace {
        AirWorkspace {
            schema_version: AIR_SCHEMA_VERSION,
            packages: vec![AirPackage {
                name: "x".into(),
                version: "0".into(),
                root_dir: "/".into(),
                files: vec![AirFile {
                    path: "t.rs".into(),
                    module_path: module.map(str::to_string),
                    items,
                    hints: Vec::new(),
                    parse_error: None,
                    line_count: 1,
                }],
            }],
            facts: Vec::new(),
        }
    }

    fn n_pub_types(n: usize) -> Vec<AirItem> {
        (0..n).map(|i| pub_type(&format!("T{i}"))).collect()
    }

    fn configured(default_budget: u32) -> MoSection {
        MoSection {
            default_max_public_types: Some(default_budget),
            overrides: Vec::new(),
            entropy_threshold: None,
            handler_name_patterns: Vec::new(),
            persistence_import_patterns: Vec::new(),
        }
    }

    #[test]
    fn mo001_silent_on_default_section() {
        // No fields configured — must stay silent regardless of file shape.
        // Mirrors the DG/OT lockfile-driven convention.
        let air = air_with(Some("foo::bar"), n_pub_types(50));
        let section = MoSection::default();
        assert!(mo001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn mo001_fires_when_count_exceeds_default_budget() {
        // 6 public types under default budget of 5 → fires.
        let air = air_with(Some("foo::bar"), n_pub_types(6));
        let section = configured(5);
        let diags = mo001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1, "expected one diag, got {diags:?}");
        assert_eq!(diags[0].rule_id, "MO001");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(diags[0].message.contains("foo::bar"));
        assert!(diags[0].message.contains("6"));
        assert!(diags[0].message.contains("budget 5"));
    }

    #[test]
    fn mo001_quiet_when_count_at_or_below_default_budget() {
        let section = configured(5);
        // exactly at budget
        let air = air_with(Some("foo::bar"), n_pub_types(5));
        assert!(mo001(&air, &section, CheckMode::Human).is_empty());
        // under budget
        let air = air_with(Some("foo::bar"), n_pub_types(2));
        assert!(mo001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn mo001_only_counts_public_top_level_types() {
        // 4 private + 5 public = 9 items, but only 5 are pub → at budget, quiet.
        let mut items = n_pub_types(5);
        for i in 0..4 {
            items.push(priv_type(&format!("Priv{i}")));
        }
        let air = air_with(Some("foo::bar"), items);
        let section = configured(5);
        assert!(mo001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn mo001_override_raises_budget_effectively() {
        // Default budget 5; api module has 12 public types, override gives 20.
        let air = air_with(Some("lore::api::v1"), n_pub_types(12));
        let section = MoSection {
            default_max_public_types: Some(5),
            overrides: vec![MoOverride {
                module: "lore::api::*".into(),
                max_public_types: 20,
            }],
            ..Default::default()
        };
        assert!(
            mo001(&air, &section, CheckMode::Human).is_empty(),
            "override should raise budget above the file's count"
        );
    }

    #[test]
    fn mo001_override_lowers_budget_effectively() {
        // Default 5; domain file has 5 public types (within default). Override
        // lowers the domain budget to 2 → fires.
        let air = air_with(Some("lore::domain::user"), n_pub_types(5));
        let section = MoSection {
            default_max_public_types: Some(5),
            overrides: vec![MoOverride {
                module: "lore::domain::*".into(),
                max_public_types: 2,
            }],
            ..Default::default()
        };
        let diags = mo001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1, "override should lower budget below count");
        assert_eq!(diags[0].rule_id, "MO001");
        assert!(diags[0].message.contains("budget 2"));
        assert!(
            diags[0]
                .why
                .iter()
                .any(|w| w.contains("override") && w.contains("lore::domain::*")),
            "expected override mention in `why`; got {:?}",
            diags[0].why
        );
    }

    #[test]
    fn mo001_first_override_wins() {
        let air = air_with(Some("lore::api::v1"), n_pub_types(8));
        let section = MoSection {
            default_max_public_types: Some(5),
            overrides: vec![
                MoOverride {
                    module: "lore::api::*".into(),
                    max_public_types: 20,
                },
                MoOverride {
                    module: "lore::*".into(),
                    max_public_types: 3,
                },
            ],
            ..Default::default()
        };
        // First override (20) wins, so 8 public types is fine.
        assert!(mo001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn mo001_agent_strict_elevates_to_fatal() {
        let air = air_with(Some("foo::bar"), n_pub_types(6));
        let section = configured(5);
        let diags = mo001(&air, &section, CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].severity,
            Severity::Fatal,
            "agent-strict should elevate Warning to Fatal"
        );
    }

    #[test]
    fn mo001_skips_files_without_module_path() {
        // No module_path → can't apply overrides → skip entirely.
        let air = air_with(None, n_pub_types(50));
        let section = configured(5);
        assert!(mo001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn mo001_one_diagnostic_per_file() {
        // Two violating files → two diagnostics, regardless of how many
        // public types each contains.
        let air = AirWorkspace {
            schema_version: AIR_SCHEMA_VERSION,
            packages: vec![AirPackage {
                name: "x".into(),
                version: "0".into(),
                root_dir: "/".into(),
                files: vec![
                    AirFile {
                        path: "a.rs".into(),
                        module_path: Some("x::a".into()),
                        items: n_pub_types(10),
                        hints: Vec::new(),
                        parse_error: None,
                        line_count: 1,
                    },
                    AirFile {
                        path: "b.rs".into(),
                        module_path: Some("x::b".into()),
                        items: n_pub_types(7),
                        hints: Vec::new(),
                        parse_error: None,
                        line_count: 1,
                    },
                ],
            }],
            facts: Vec::new(),
        };
        let section = configured(5);
        let diags = mo001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 2, "got {diags:?}");
    }

    #[test]
    fn mo001_with_only_overrides_and_no_default_uses_fallback_for_unmatched() {
        // overrides set → section is non-default → MO001 active. Files that
        // don't match any override fall back to DEFAULT_MAX_PUBLIC_TYPES (5).
        let air = air_with(Some("other::module"), n_pub_types(6));
        let section = MoSection {
            default_max_public_types: None,
            overrides: vec![MoOverride {
                module: "lore::api::*".into(),
                max_public_types: 20,
            }],
            ..Default::default()
        };
        let diags = mo001(&air, &section, CheckMode::Human);
        assert_eq!(
            diags.len(),
            1,
            "fallback budget should apply; got {diags:?}"
        );
        assert!(diags[0].message.contains("budget 5"));
        assert!(
            diags[0].why.iter().any(|w| w.contains("built-in fallback")),
            "expected fallback explanation in why; got {:?}",
            diags[0].why
        );
    }

    // ---- shared helpers for MO002 / MO003 / MO004 tests ----

    fn canonical_hint() -> AirHint {
        AirHint {
            kind: HintKind::Canonical,
            raw: "// ot: canonical".into(),
            span: AirSpan::new("t.rs", 5, 5),
            target_span: Some(AirSpan::new("t.rs", 6, 10)),
        }
    }

    fn boundary_hint() -> AirHint {
        AirHint {
            kind: HintKind::Boundary {
                concept: Some("user".into()),
                boundary: Some("api".into()),
            },
            raw: "// ot: boundary user api".into(),
            span: AirSpan::new("t.rs", 20, 20),
            target_span: Some(AirSpan::new("t.rs", 21, 30)),
        }
    }

    fn converter_hint() -> AirHint {
        AirHint {
            kind: HintKind::Converter,
            raw: "// ot: converter".into(),
            span: AirSpan::new("t.rs", 40, 40),
            target_span: Some(AirSpan::new("t.rs", 41, 45)),
        }
    }

    fn func(name: &str, line: u32) -> AirItem {
        AirItem::Function(AirFunction {
            name: name.into(),
            symbol: format!("x::{name}"),
            visibility: Visibility::Public,
            params: Vec::new(),
            return_type: None,
            span: AirSpan::new("t.rs", line, line + 5),
            line_count: 6,
            decorators: Vec::new(),
            symbol_segments: Vec::new(),
            doc: None,
        })
    }

    fn import(path: &str) -> AirItem {
        AirItem::Import(AirImport {
            path: path.into(),
            path_segments: Vec::new(),
            visibility: Visibility::Private,
            span: AirSpan::new("t.rs", 1, 1),
        })
    }

    fn call_site(callee: &str) -> AirItem {
        AirItem::CallSite(AirCallSite {
            callee: callee.into(),
            kind: CallKind::Function,
            function: None,
            span: AirSpan::new("t.rs", 1, 1),
        })
    }

    fn air_with_full(
        module: Option<&str>,
        items: Vec<AirItem>,
        hints: Vec<AirHint>,
    ) -> AirWorkspace {
        AirWorkspace {
            schema_version: AIR_SCHEMA_VERSION,
            packages: vec![AirPackage {
                name: "x".into(),
                version: "0".into(),
                root_dir: "/".into(),
                files: vec![AirFile {
                    path: "t.rs".into(),
                    module_path: module.map(str::to_string),
                    items,
                    hints,
                    parse_error: None,
                    line_count: 100,
                }],
            }],
            facts: Vec::new(),
        }
    }

    fn mo_section_with_entropy(threshold: u32) -> MoSection {
        MoSection {
            entropy_threshold: Some(threshold),
            ..Default::default()
        }
    }

    // ---- MO002 tests ----

    #[test]
    fn mo002_silent_on_default_section() {
        // Even if a file is a clear blob (canonical+boundary+converter+handler),
        // MO002 stays silent until the lockfile section is configured.
        let air = air_with_full(
            Some("foo::bar"),
            vec![func("user_handler", 10), import("crate::sqlx::query")],
            vec![canonical_hint(), boundary_hint(), converter_hint()],
        );
        let section = MoSection::default();
        assert!(mo002(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn mo002_fires_when_three_roles_meet_default_threshold() {
        // canonical + boundary + handler = 3 roles → at default threshold (3)
        let air = air_with_full(
            Some("foo::bar"),
            vec![func("user_handler", 10)],
            vec![canonical_hint(), boundary_hint()],
        );
        // section is "configured" via entropy_threshold=Some(3) so the rule
        // is active; default threshold path is exercised in another test.
        let section = mo_section_with_entropy(3);
        let diags = mo002(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1, "got {diags:?}");
        assert_eq!(diags[0].rule_id, "MO002");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(diags[0].message.contains("foo::bar"));
        assert!(diags[0].message.contains("3"));
        assert!(diags[0].message.contains("canonical"));
        assert!(diags[0].message.contains("boundary"));
        assert!(diags[0].message.contains("handler"));
    }

    #[test]
    fn mo002_quiet_when_below_threshold() {
        // Only canonical + handler = 2 roles → under default threshold of 3.
        let air = air_with_full(
            Some("foo::bar"),
            vec![func("on_user_handler", 10)],
            vec![canonical_hint()],
        );
        let section = mo_section_with_entropy(3);
        assert!(mo002(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn mo002_counts_persistence_imports_and_io_call_sites() {
        // canonical + persistence import (sqlx) + io call site (fs::read) = 3
        let air = air_with_full(
            Some("foo::bar"),
            vec![
                import("crate::sqlx::query"),
                call_site("std::fs::read_to_string"),
            ],
            vec![canonical_hint()],
        );
        let section = mo_section_with_entropy(3);
        let diags = mo002(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        let m = &diags[0].message;
        assert!(m.contains("canonical"));
        assert!(m.contains("persistence"));
        assert!(m.contains("io"));
    }

    #[test]
    fn mo002_user_handler_patterns_override_defaults() {
        // A function called `process` does NOT match the default
        // `*_handler`/`handle_*` patterns. With user-supplied `process*`
        // pattern it does, raising the role count.
        let air = air_with_full(
            Some("foo::bar"),
            vec![func("process", 10), import("crate::sqlx::query")],
            vec![canonical_hint(), boundary_hint()],
        );
        // Default patterns: canonical + boundary + persistence = 3 → fires.
        // User-narrowed patterns to `does_not_match_*`: canonical + boundary +
        // persistence = 3 (handler still not counted) → still fires.
        // To verify the override path, give threshold = 4 and patterns that
        // match `process` so the count is 4.
        let section = MoSection {
            entropy_threshold: Some(4),
            handler_name_patterns: vec!["process*".into()],
            ..Default::default()
        };
        let diags = mo002(&air, &section, CheckMode::Human);
        assert_eq!(
            diags.len(),
            1,
            "expected fire at threshold 4; got {diags:?}"
        );
        assert!(diags[0].message.contains("handler"));
    }

    #[test]
    fn mo002_agent_strict_elevates_to_fatal_and_skips_no_module_path() {
        // Compound: agent-strict elevates Warning→Fatal; files without a
        // module_path are skipped entirely (no diagnostic).
        let air_with_path = air_with_full(
            Some("foo::bar"),
            vec![func("user_handler", 10)],
            vec![canonical_hint(), boundary_hint()],
        );
        let section = mo_section_with_entropy(3);
        let diags = mo002(&air_with_path, &section, CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);

        let air_no_path = air_with_full(
            None,
            vec![func("user_handler", 10)],
            vec![canonical_hint(), boundary_hint(), converter_hint()],
        );
        assert!(mo002(&air_no_path, &section, CheckMode::Human).is_empty());
    }

    // ---- MO003 tests ----

    #[test]
    fn mo003_fires_when_canonical_and_boundary_co_exist() {
        let air = air_with_full(
            Some("foo::bar"),
            vec![],
            vec![canonical_hint(), boundary_hint()],
        );
        let diags = mo003(&air, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "MO003");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(diags[0].message.contains("foo::bar"));
        assert!(diags[0].message.contains("canonical"));
        assert!(diags[0].message.contains("boundary"));
    }

    #[test]
    fn mo003_quiet_with_only_canonical() {
        let air = air_with_full(Some("foo::bar"), vec![], vec![canonical_hint()]);
        assert!(mo003(&air, CheckMode::Human).is_empty());
    }

    #[test]
    fn mo003_quiet_with_only_boundary() {
        let air = air_with_full(Some("foo::bar"), vec![], vec![boundary_hint()]);
        assert!(mo003(&air, CheckMode::Human).is_empty());
    }

    #[test]
    fn mo003_quiet_with_no_hints() {
        let air = air_with_full(Some("foo::bar"), vec![func("anything", 1)], vec![]);
        assert!(mo003(&air, CheckMode::Human).is_empty());
    }

    #[test]
    fn mo003_skips_files_without_module_path() {
        let air = air_with_full(None, vec![], vec![canonical_hint(), boundary_hint()]);
        assert!(mo003(&air, CheckMode::Human).is_empty());
    }

    #[test]
    fn mo003_agent_strict_elevates_to_fatal() {
        let air = air_with_full(
            Some("foo::bar"),
            vec![],
            vec![canonical_hint(), boundary_hint()],
        );
        let diags = mo003(&air, CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    // ---- MO004 tests ----

    #[test]
    fn mo004_fires_when_canonical_and_handler_co_exist() {
        let air = air_with_full(
            Some("foo::bar"),
            vec![func("user_handler", 10)],
            vec![canonical_hint()],
        );
        let section = MoSection::default();
        let diags = mo004(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "MO004");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(diags[0].message.contains("foo::bar"));
        assert!(diags[0].message.contains("user_handler"));
    }

    #[test]
    fn mo004_quiet_when_only_canonical_no_handler() {
        let air = air_with_full(
            Some("foo::bar"),
            vec![func("compute", 10)],
            vec![canonical_hint()],
        );
        let section = MoSection::default();
        assert!(mo004(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn mo004_quiet_when_only_handler_no_canonical() {
        let air = air_with_full(Some("foo::bar"), vec![func("user_handler", 10)], vec![]);
        let section = MoSection::default();
        assert!(mo004(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn mo004_uses_user_supplied_handler_patterns_when_set() {
        // `process` doesn't match the default `*_handler`/`handle_*` patterns
        // but does match the user-supplied `process*` pattern.
        let air = air_with_full(
            Some("foo::bar"),
            vec![func("process", 10)],
            vec![canonical_hint()],
        );
        let default_section = MoSection::default();
        assert!(mo004(&air, &default_section, CheckMode::Human).is_empty());
        let section = MoSection {
            handler_name_patterns: vec!["process*".into()],
            ..Default::default()
        };
        let diags = mo004(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("process"));
    }

    #[test]
    fn mo004_skips_files_without_module_path() {
        let air = air_with_full(None, vec![func("user_handler", 10)], vec![canonical_hint()]);
        let section = MoSection::default();
        assert!(mo004(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn mo004_agent_strict_elevates_to_fatal() {
        let air = air_with_full(
            Some("foo::bar"),
            vec![func("handle_request", 10)],
            vec![canonical_hint()],
        );
        let section = MoSection::default();
        let diags = mo004(&air, &section, CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }
}
