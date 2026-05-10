//! OT009 — scattered validation/normalization.
//!
//! Fires when a function lives outside the canonical's owner file *and*
//! outside any accepted converter, but its *name* and *signature* both look
//! like validation/normalization of a known canonical (e.g. `validate_email`
//! returning a `Result<EmailAddress, _>`, or `normalize_user_id(s: &str)
//! -> UserId`). Both signals are required so generic helpers
//! (`fn validate_input(...)`) don't trip the rule.
//!
//! Confidence 0.75. The spec lists this as "warning by default; fatal under
//! `--agent-strict` for high-confidence cases" — `from_confidence(0.75,
//! AgentStrict)` returns `Fatal`, `(0.75, Human)` returns `Warning`.

use std::collections::{BTreeMap, BTreeSet};

use locus_air::{AirItem, AirWorkspace};

use super::super::lockfile_schema::OtSection;
use super::helpers::{file_of_symbol, type_text_references};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

pub fn ot009(air: &AirWorkspace, section: &OtSection, mode: CheckMode) -> Vec<Diagnostic> {
    let mut canonicals: BTreeMap<String, (String, String)> = BTreeMap::new(); // short → (concept, owner_file)
    for (concept_id, entry) in &section.concepts {
        let symbol = &entry.canonical.symbol;
        let Some(short) = symbol.rsplit("::").next() else {
            continue;
        };
        let Some(file_path) = file_of_symbol(air, symbol) else {
            continue;
        };
        canonicals.insert(short.to_string(), (concept_id.clone(), file_path));
    }
    if canonicals.is_empty() {
        return Vec::new();
    }
    let accepted_converters: BTreeSet<&str> = section
        .concepts
        .values()
        .flat_map(|e| e.converters.iter().map(|c| c.symbol.as_str()))
        .collect();

    let confidence = 0.75;
    let Some(severity) = Severity::from_confidence(confidence, mode) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                let AirItem::Function(f) = item else {
                    continue;
                };
                if accepted_converters.contains(f.symbol.as_str()) {
                    continue;
                }
                let Some(prefix) = matched_validate_prefix(&f.name) else {
                    continue;
                };
                // Find a canonical referenced in params/return — that's the
                // "operates on a known concept" part of the signal.
                let mut concept_match: Option<(&str, &str)> = None;
                for (short, (concept, owner)) in &canonicals {
                    let signature_hits =
                        f.params.iter().any(|(_, t)| type_text_references(t, short))
                            || f.return_type
                                .as_deref()
                                .is_some_and(|t| type_text_references(t, short));
                    if signature_hits {
                        concept_match = Some((concept.as_str(), owner.as_str()));
                        break;
                    }
                }
                let Some((concept_id, owner_file)) = concept_match else {
                    continue;
                };
                if file.path == owner_file {
                    continue; // validator inside the canonical's own module is fine
                }
                out.push(Diagnostic {
                    rule_id: "OT009".to_string(),
                    severity,
                    span: f.span.clone(),
                    concept: Some(concept_id.to_string()),
                    message: format!(
                        "`{}` looks like {prefix} for canonical `{concept_id}` but lives \
                         outside the owner module and outside any accepted converter",
                        f.symbol
                    ),
                    why: vec![
                        format!(
                            "function name starts with `{prefix}` (validation/normalization shape)"
                        ),
                        format!("signature references canonical for `{concept_id}`"),
                        format!("owner module: `{owner_file}`"),
                        format!("inference confidence: {confidence:.2}"),
                    ],
                    suggested_fix: Some(format!(
                        "move this into the owner of `{concept_id}` (so the canonical \
                         enforces its own invariants), or accept this function as a \
                         converter via `locus init` if it's the legitimate edge"
                    )),
                });
            }
        }
    }
    out
}

/// Returns the matched prefix if `name` starts with one of the
/// validation/normalization shape prefixes recognised by OT009.
fn matched_validate_prefix(name: &str) -> Option<&'static str> {
    const PREFIXES: &[&str] = &[
        "validate_",
        "is_valid_",
        "check_",
        "verify_",
        "ensure_",
        "normalize_",
        "sanitize_",
        "canonicalize_",
        "parse_",
        "clean_",
    ];
    PREFIXES.iter().copied().find(|p| name.starts_with(p))
}
