//! DC rule implementations.
//!
//! Implemented:
//! - [`dc001`]: public type or function has no doc comment. Heuristic
//!   baseline for documentation ownership — a public symbol with no
//!   `///` / `#[doc = "..."]` is an undocumented API surface, which the
//!   spec calls out as a failure of documentation ownership
//!   (`docs/PARADIGMS.md` §"Paradigm 17: Documentation / Comment
//!   Ownership").
//! - [`dc002`]: public type or function carries doc text containing a
//!   forbidden phrase from the lockfile's `forbidden_doc_phrases` list —
//!   high-signal LLM-transcript residue (`"as discussed"`,
//!   `"the prompt"`, …) and stale planning markers (`"TODO"`,
//!   `"for now"`, …). Inference-shaped: per-phrase confidence drives
//!   `Severity::from_confidence`.
//!
//! DC001 is opt-in: it returns no diagnostics unless
//! `paradigms.DC.require_public_docs` is `true`. Patterns listed in
//! `paradigms.DC.exempt_paths` skip the file entirely (intended for test
//! modules, generated code, FFI shims).

use locus_air::{AirItem, AirWorkspace, Visibility};

use super::lockfile_schema::{DcSection, matches_pattern};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

/// DC001 — public API has no doc comment.
///
/// For every `AirFile` whose `module_path` does *not* match any pattern in
/// `exempt_paths`, fire one diagnostic per `AirItem::Type` or
/// `AirItem::Function` whose `visibility` is `Public` and whose `doc` is
/// `None`.
///
/// Returns no diagnostics when `section.require_public_docs` is `false`
/// (the default). This keeps the rule silent for projects that haven't
/// opted into the "public API must be documented" policy.
///
/// Severity: Warning by default; Fatal under `--agent-strict`. Documented
/// public API is a guardrail agents are particularly prone to skipping, so
/// the strict-mode elevation is deliberate.
pub fn dc001(air: &AirWorkspace, section: &DcSection, mode: CheckMode) -> Vec<Diagnostic> {
    if !section.require_public_docs {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            // Files without a module_path can't be matched against
            // exempt_paths. Treat them as non-exempt — the rule still
            // applies, falling back on the file `path` for diagnostic
            // text.
            let module_path = file.module_path.as_deref();
            if let Some(mp) = module_path
                && section
                    .exempt_paths
                    .iter()
                    .any(|pat| matches_pattern(pat, mp))
            {
                continue;
            }
            let module_label = module_path.unwrap_or(&file.path);

            for item in &file.items {
                match item {
                    AirItem::Type(ty) => {
                        if ty.visibility != Visibility::Public {
                            continue;
                        }
                        if ty.doc.is_some() {
                            continue;
                        }
                        out.push(Diagnostic {
                            rule_id: "DC001".to_string(),
                            severity: mode.elevate(Severity::Warning),
                            span: ty.span.clone(),
                            concept: None,
                            message: format!(
                                "public type `{}` in `{}` has no doc comment",
                                ty.name, module_label,
                            ),
                            why: vec![
                                format!("type `{}` (`{}`)", ty.name, ty.symbol),
                                "visibility is Public".into(),
                                "doc is None (no `///` or `#[doc = \"...\"]` text)".into(),
                                format!(
                                    "module `{module_label}` did not match any \
                                     `paradigms.DC.exempt_paths` pattern"
                                ),
                            ],
                            suggested_fix: Some(format!(
                                "add a `///` doc comment on `{}` describing why it exists \
                                 and what invariant it carries; if this region is \
                                 intentionally undocumented, add a pattern to \
                                 `paradigms.DC.exempt_paths` (e.g. `{module_label}` or a \
                                 `parent::*` wildcard) — see `docs/PARADIGMS.md` \
                                 §\"Paradigm 17: Documentation / Comment Ownership\"",
                                ty.name,
                            )),
                        });
                    }
                    AirItem::Function(func) => {
                        if func.visibility != Visibility::Public {
                            continue;
                        }
                        if func.doc.is_some() {
                            continue;
                        }
                        out.push(Diagnostic {
                            rule_id: "DC001".to_string(),
                            severity: mode.elevate(Severity::Warning),
                            span: func.span.clone(),
                            concept: None,
                            message: format!(
                                "public function `{}` in `{}` has no doc comment",
                                func.name, module_label,
                            ),
                            why: vec![
                                format!("function `{}` (`{}`)", func.name, func.symbol),
                                "visibility is Public".into(),
                                "doc is None (no `///` or `#[doc = \"...\"]` text)".into(),
                                format!(
                                    "module `{module_label}` did not match any \
                                     `paradigms.DC.exempt_paths` pattern"
                                ),
                            ],
                            suggested_fix: Some(format!(
                                "add a `///` doc comment on `{}` describing why it exists \
                                 and what invariant it carries; if this region is \
                                 intentionally undocumented, add a pattern to \
                                 `paradigms.DC.exempt_paths` (e.g. `{module_label}` or a \
                                 `parent::*` wildcard) — see `docs/PARADIGMS.md` \
                                 §\"Paradigm 17: Documentation / Comment Ownership\"",
                                func.name,
                            )),
                        });
                    }
                    _ => {}
                }
            }
        }
    }
    out
}

/// DC002 — public item's doc comment contains a forbidden phrase.
///
/// Walks every `AirItem::Type` and `AirItem::Function` in the workspace
/// whose `doc.is_some()` and matches each entry in
/// `section.forbidden_doc_phrases` as a case-insensitive substring of the
/// doc text. One diagnostic per (item, phrase). The phrase's `confidence`
/// drives `Severity::from_confidence(confidence, mode)`; if that returns
/// `None` (confidence < 0.50) the diagnostic is skipped — supports
/// user-configured low-confidence demotions.
///
/// Stays silent when `forbidden_doc_phrases` is empty: clearing the list
/// is the documented opt-out. The default seed list is non-empty so users
/// get coverage out of the box.
///
/// Unlike DC001 this rule does not consult `require_public_docs` or
/// `exempt_paths` — DC001 catches the *absence* of docs (a project policy
/// choice), DC002 catches LLM-transcript residue *presence* in the doc
/// text users have already written, which is always a problem.
pub fn dc002(air: &AirWorkspace, section: &DcSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.forbidden_doc_phrases.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let module_label = file.module_path.as_deref().unwrap_or(&file.path);
            for item in &file.items {
                let (kind_label, name, symbol, doc, span, vis) = match item {
                    AirItem::Type(ty) => (
                        "type",
                        &ty.name,
                        &ty.symbol,
                        ty.doc.as_deref(),
                        ty.span.clone(),
                        ty.visibility,
                    ),
                    AirItem::Function(func) => (
                        "function",
                        &func.name,
                        &func.symbol,
                        func.doc.as_deref(),
                        func.span.clone(),
                        func.visibility,
                    ),
                    _ => continue,
                };
                if vis != Visibility::Public {
                    continue;
                }
                let Some(doc_text) = doc else {
                    continue;
                };
                let doc_lower = doc_text.to_lowercase();
                for forbidden in &section.forbidden_doc_phrases {
                    let Some(matched_alias) = matched_phrasing(&doc_lower, forbidden) else {
                        continue;
                    };
                    let Some(severity) = Severity::from_confidence(forbidden.confidence, mode)
                    else {
                        continue;
                    };
                    let primary = &forbidden.phrase;
                    let alias_note = if matched_alias == *primary {
                        String::new()
                    } else {
                        format!(" (alias of `{primary}`)")
                    };
                    out.push(Diagnostic {
                        rule_id: "DC002".to_string(),
                        severity,
                        span: span.clone(),
                        concept: None,
                        message: format!(
                            "public {kind_label} `{name}` in `{module_label}` has a doc \
                             comment containing forbidden phrase `{matched_alias}`{alias_note}"
                        ),
                        why: vec![
                            format!("{kind_label} `{name}` (`{symbol}`)"),
                            format!("matched phrase `{matched_alias}`{alias_note}"),
                            format!("phrase confidence {:.2}", forbidden.confidence),
                            "doc text contains phrase suggesting LLM transcript residue \
                             or stale planning notes"
                                .into(),
                        ],
                        suggested_fix: Some(format!(
                            "rewrite the doc comment on `{name}` to describe what the \
                             {kind_label} *is* and what invariant it carries — not the \
                             conversation it came from. If the marker is intentional \
                             (e.g. a tracked `TODO`) and you want to keep it, demote or \
                             remove the matching entry from \
                             `paradigms.DC.forbidden_doc_phrases`."
                        )),
                    });
                }
            }
        }
    }
    out
}

/// Try the primary `phrase` first, then each alias, against the
/// already-lowercased doc text. Returns the matched phrasing (in its
/// original casing) so the diagnostic can surface what the user wrote,
/// not just the seeded primary. `None` when nothing matched.
///
/// The alias mechanism is the no-LLM, deterministic substitute for
/// embedding-based paraphrase detection: every accepted variant is in
/// the lockfile, every match is a literal substring, every diagnostic
/// is reproducible from inputs alone.
fn matched_phrasing(
    doc_lower: &str,
    forbidden: &super::lockfile_schema::ForbiddenPhrase,
) -> Option<String> {
    if doc_lower.contains(&forbidden.phrase.to_lowercase()) {
        return Some(forbidden.phrase.clone());
    }
    for alias in &forbidden.aliases {
        if doc_lower.contains(&alias.to_lowercase()) {
            return Some(alias.clone());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use locus_air::{
        AIR_SCHEMA_VERSION, AirFile, AirFunction, AirPackage, AirSpan, AirType, TypeKind,
        Visibility,
    };

    fn ty_item(name: &str, vis: Visibility, doc: Option<&str>) -> AirItem {
        AirItem::Type(AirType {
            kind: TypeKind::Struct,
            name: name.into(),
            symbol: format!("x::api::{name}"),
            visibility: vis,
            fields: Vec::new(),
            variants: Vec::new(),
            derives: Vec::new(),
            attrs: Vec::new(),
            span: AirSpan::new("t.rs", 1, 1),
            doc: doc.map(|s| s.to_string()),
        })
    }

    fn fn_item(name: &str, vis: Visibility, doc: Option<&str>) -> AirItem {
        AirItem::Function(AirFunction {
            name: name.into(),
            symbol: format!("x::api::{name}"),
            visibility: vis,
            params: Vec::new(),
            return_type: None,
            span: AirSpan::new("t.rs", 1, 1),
            line_count: 1,
            doc: doc.map(|s| s.to_string()),
        })
    }

    fn air_with_module(module: &str, items: Vec<AirItem>) -> AirWorkspace {
        AirWorkspace {
            schema_version: AIR_SCHEMA_VERSION,
            packages: vec![AirPackage {
                name: "x".into(),
                version: "0".into(),
                root_dir: "/".into(),
                files: vec![AirFile {
                    path: "t.rs".into(),
                    module_path: Some(module.into()),
                    items,
                    hints: Vec::new(),
                    parse_error: None,
                    line_count: 1,
                }],
            }],
            facts: Vec::new(),
        }
    }

    #[test]
    fn dc001_silent_when_require_public_docs_is_default_false() {
        let air = air_with_module(
            "x::api",
            vec![
                ty_item("Widget", Visibility::Public, None),
                fn_item("make_widget", Visibility::Public, None),
            ],
        );
        let section = DcSection::default();
        assert!(dc001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn dc001_fires_on_public_type_without_doc() {
        let air = air_with_module("x::api", vec![ty_item("Widget", Visibility::Public, None)]);
        let section = DcSection {
            require_public_docs: true,
            exempt_paths: Vec::new(),
            ..DcSection::default()
        };
        let diags = dc001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "DC001");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(diags[0].message.contains("Widget"));
        assert!(diags[0].message.contains("x::api"));
        assert!(diags[0].message.contains("no doc comment"));
    }

    #[test]
    fn dc001_fires_on_public_function_without_doc() {
        let air = air_with_module(
            "x::api",
            vec![fn_item("make_widget", Visibility::Public, None)],
        );
        let section = DcSection {
            require_public_docs: true,
            exempt_paths: Vec::new(),
            ..DcSection::default()
        };
        let diags = dc001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "DC001");
        assert!(diags[0].message.contains("make_widget"));
        assert!(diags[0].message.contains("public function"));
    }

    #[test]
    fn dc001_quiet_on_private_items() {
        let air = air_with_module(
            "x::api",
            vec![
                ty_item("Widget", Visibility::Private, None),
                ty_item("Inner", Visibility::Crate, None),
                ty_item("Restricted", Visibility::Restricted, None),
                fn_item("helper", Visibility::Private, None),
                fn_item("crate_helper", Visibility::Crate, None),
            ],
        );
        let section = DcSection {
            require_public_docs: true,
            exempt_paths: Vec::new(),
            ..DcSection::default()
        };
        assert!(dc001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn dc001_quiet_on_items_with_doc() {
        let air = air_with_module(
            "x::api",
            vec![
                ty_item("Widget", Visibility::Public, Some("a thing")),
                fn_item("make_widget", Visibility::Public, Some("makes one")),
            ],
        );
        let section = DcSection {
            require_public_docs: true,
            exempt_paths: Vec::new(),
            ..DcSection::default()
        };
        assert!(dc001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn dc001_skips_files_matching_exempt_paths() {
        let air = air_with_module(
            "x::api::tests",
            vec![
                ty_item("Widget", Visibility::Public, None),
                fn_item("make_widget", Visibility::Public, None),
            ],
        );
        let section = DcSection {
            require_public_docs: true,
            exempt_paths: vec!["x::api::tests::*".into(), "x::api::tests".into()],
            ..DcSection::default()
        };
        assert!(dc001(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn dc001_agent_strict_elevates_to_fatal() {
        let air = air_with_module("x::api", vec![ty_item("Widget", Visibility::Public, None)]);
        let section = DcSection {
            require_public_docs: true,
            exempt_paths: Vec::new(),
            ..DcSection::default()
        };
        let diags = dc001(&air, &section, CheckMode::AgentStrict);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    #[test]
    fn dc001_fires_per_undocumented_item_in_mixed_file() {
        let air = air_with_module(
            "x::api",
            vec![
                ty_item("Documented", Visibility::Public, Some("good")),
                ty_item("UndocType", Visibility::Public, None),
                fn_item("undoc_fn", Visibility::Public, None),
                ty_item("PrivateType", Visibility::Private, None),
            ],
        );
        let section = DcSection {
            require_public_docs: true,
            exempt_paths: Vec::new(),
            ..DcSection::default()
        };
        let diags = dc001(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 2);
        let messages: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
        assert!(messages.iter().any(|m| m.contains("UndocType")));
        assert!(messages.iter().any(|m| m.contains("undoc_fn")));
        assert!(!messages.iter().any(|m| m.contains("Documented")));
        assert!(!messages.iter().any(|m| m.contains("PrivateType")));
    }

    use super::super::lockfile_schema::ForbiddenPhrase;

    #[test]
    fn dc002_fires_when_public_type_doc_contains_as_discussed() {
        let air = air_with_module(
            "x::api",
            vec![ty_item(
                "Widget",
                Visibility::Public,
                Some("As discussed, this represents a widget."),
            )],
        );
        let section = DcSection::default();
        let diags = dc002(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "DC002");
        assert!(diags[0].concept.is_none());
        assert!(diags[0].message.contains("Widget"));
        assert!(diags[0].message.contains("as discussed"));
        // 0.90 confidence => Fatal regardless of mode.
        assert_eq!(diags[0].severity, Severity::Fatal);
    }

    #[test]
    fn dc002_fires_when_public_function_doc_contains_todo() {
        let air = air_with_module(
            "x::api",
            vec![fn_item(
                "make_widget",
                Visibility::Public,
                Some("Returns a widget. TODO: handle errors."),
            )],
        );
        let section = DcSection::default();
        let diags = dc002(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_id, "DC002");
        assert!(diags[0].message.contains("make_widget"));
        assert!(diags[0].message.contains("TODO"));
        // 0.70 confidence under Human => Warning.
        assert_eq!(diags[0].severity, Severity::Warning);
    }

    #[test]
    fn dc002_matching_is_case_insensitive() {
        // Lowercase `todo` in source should still match the seeded `TODO` phrase.
        let air = air_with_module(
            "x::api",
            vec![ty_item(
                "Widget",
                Visibility::Public,
                Some("a widget. todo: revisit."),
            )],
        );
        let section = DcSection::default();
        let diags = dc002(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("TODO"));
    }

    #[test]
    fn dc002_silent_when_forbidden_doc_phrases_is_empty() {
        // Even with a doc that would otherwise match every default phrase,
        // an empty list is the documented opt-out.
        let air = air_with_module(
            "x::api",
            vec![ty_item(
                "Widget",
                Visibility::Public,
                Some("As discussed, TODO, FIXME, HACK, the prompt — for now."),
            )],
        );
        let section = DcSection {
            forbidden_doc_phrases: Vec::new(),
            ..DcSection::default()
        };
        assert!(dc002(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn dc002_fires_once_per_matched_phrase_on_one_item() {
        let air = air_with_module(
            "x::api",
            vec![ty_item(
                "Widget",
                Visibility::Public,
                Some("As discussed, TODO clean this up."),
            )],
        );
        let section = DcSection::default();
        let diags = dc002(&air, &section, CheckMode::Human);
        // Default list contains "as discussed", "TODO", and "clean this up"
        // — three matches on one item.
        assert_eq!(diags.len(), 3);
        let messages: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
        assert!(messages.iter().any(|m| m.contains("as discussed")));
        assert!(messages.iter().any(|m| m.contains("TODO")));
        assert!(messages.iter().any(|m| m.contains("clean this up")));
    }

    #[test]
    fn dc002_agent_strict_elevates_warning_band_to_fatal() {
        // 0.75 confidence => Warning under Human, Fatal under AgentStrict.
        let air = air_with_module(
            "x::api",
            vec![ty_item(
                "Widget",
                Visibility::Public,
                Some("A widget. For now this leaks."),
            )],
        );
        let section = DcSection {
            forbidden_doc_phrases: vec![ForbiddenPhrase {
                phrase: "for now".into(),
                confidence: 0.75,
                aliases: Vec::new(),
            }],
            ..DcSection::default()
        };
        let human = dc002(&air, &section, CheckMode::Human);
        assert_eq!(human.len(), 1);
        assert_eq!(human[0].severity, Severity::Warning);

        let strict = dc002(&air, &section, CheckMode::AgentStrict);
        assert_eq!(strict.len(), 1);
        assert_eq!(strict[0].severity, Severity::Fatal);
    }

    #[test]
    fn dc002_skips_items_without_doc() {
        let air = air_with_module(
            "x::api",
            vec![
                ty_item("NoDoc", Visibility::Public, None),
                fn_item("no_doc_fn", Visibility::Public, None),
            ],
        );
        let section = DcSection::default();
        assert!(dc002(&air, &section, CheckMode::Human).is_empty());
    }

    #[test]
    fn dc002_skips_non_public_items_with_residue() {
        // Residue in a private type is a separate problem; DC002 scopes to
        // public surface only, mirroring DC001.
        let air = air_with_module(
            "x::api",
            vec![ty_item(
                "Internal",
                Visibility::Private,
                Some("TODO: refactor this"),
            )],
        );
        let section = DcSection::default();
        assert!(dc002(&air, &section, CheckMode::Human).is_empty());
    }

    // ---- alias-matching tests ----

    #[test]
    fn dc002_matches_alias_and_surfaces_it_in_diagnostic() {
        let air = air_with_module(
            "x::api",
            vec![ty_item(
                "Widget",
                Visibility::Public,
                Some("This struct is what you wanted; we'll iterate next pass."),
            )],
        );
        let section = DcSection {
            forbidden_doc_phrases: vec![ForbiddenPhrase {
                phrase: "the user wanted".into(),
                confidence: 0.85,
                aliases: vec!["you wanted".into(), "you requested".into()],
            }],
            ..DcSection::default()
        };
        let diags = dc002(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert!(
            diags[0].message.contains("you wanted"),
            "diagnostic should surface the matched alias; got {}",
            diags[0].message,
        );
        assert!(
            diags[0].message.contains("(alias of `the user wanted`)"),
            "diagnostic should note alias-of-primary; got {}",
            diags[0].message,
        );
    }

    #[test]
    fn dc002_primary_phrase_match_does_not_show_alias_note() {
        let air = air_with_module(
            "x::api",
            vec![ty_item(
                "Widget",
                Visibility::Public,
                Some("As discussed, this needs more work."),
            )],
        );
        let section = DcSection {
            forbidden_doc_phrases: vec![ForbiddenPhrase {
                phrase: "as discussed".into(),
                confidence: 0.90,
                aliases: vec!["as we discussed".into(), "we discussed".into()],
            }],
            ..DcSection::default()
        };
        let diags = dc002(&air, &section, CheckMode::Human);
        assert_eq!(diags.len(), 1);
        assert!(
            !diags[0].message.contains("(alias of"),
            "primary-match diagnostic shouldn't carry the alias-note; got {}",
            diags[0].message,
        );
    }

    #[test]
    fn dc002_alias_match_is_case_insensitive() {
        let air = air_with_module(
            "x::api",
            vec![ty_item(
                "Widget",
                Visibility::Public,
                Some("AS WE DISCUSSED, this is fine."),
            )],
        );
        let section = DcSection {
            forbidden_doc_phrases: vec![ForbiddenPhrase {
                phrase: "as discussed".into(),
                confidence: 0.90,
                aliases: vec!["as we discussed".into()],
            }],
            ..DcSection::default()
        };
        assert_eq!(dc002(&air, &section, CheckMode::Human).len(), 1);
    }

    #[test]
    fn dc002_seed_aliases_cover_paraphrased_residue() {
        // The motivating use case: the seed list ships with curated
        // aliases so users don't need to enumerate them per-codebase.
        // "we agreed" should match through the `as discussed` alias set.
        let air = air_with_module(
            "x::api",
            vec![ty_item(
                "Widget",
                Visibility::Public,
                Some("We agreed this was good enough."),
            )],
        );
        let section = DcSection::default();
        let diags = dc002(&air, &section, CheckMode::Human);
        assert_eq!(
            diags.len(),
            1,
            "expected default aliases to catch `we agreed`; got {diags:?}"
        );
    }
}
