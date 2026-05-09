//! FO rules.
//!
//! Implemented:
//! - [`fo001`]: same concept defined in two different features (the inverse
//!   of DG003 — DG003 forbids feature A *reaching into* feature B's
//!   internals; FO001 forbids feature A and feature B both *defining* the
//!   same public type name).
//! - [`fo004`]: a type defined inside a `shared_paths` region has a field
//!   whose `type_text` mentions a feature-internal symbol — i.e. a
//!   "shared" DTO that secretly knows about a specific feature's internals.
//!   The shared module stops being shared.
//!
//! Follow the OT (`crates/locus-core/src/paradigms/one_truth/rules.rs`) and
//! DG (`crates/locus-core/src/paradigms/dependency_graph/rules.rs`) patterns
//! when adding more rules: each rule is a `pub fn <prefix>NNN(...) -> Vec<Diagnostic>`,
//! lockfile-driven where possible, with severity handling via
//! `CheckMode::elevate`.

use std::collections::BTreeMap;

use locus_air::{AirItem, AirSpan, AirWorkspace, Visibility};

use super::lockfile_schema::{FoFeature, FoSection, matches_pattern};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

/// FO001 — same concept defined in two different features.
///
/// For every public `AirItem::Type`, compute `(feature, type_name)` if the
/// file's `module_path` matches some feature's `module` pattern. Group by
/// `type_name` (case-sensitive). Whenever the same name is defined in two or
/// more different features, fire one diagnostic per non-incumbent definition
/// (the second, third, etc. feature to define that name). The "incumbent" is
/// the feature whose definition is encountered first in workspace iteration
/// order (package, then file, then item).
///
/// Always Fatal: same-name public types across features is a structural
/// ownership conflict — at most one feature can own the canonical concept.
pub fn fo001(air: &AirWorkspace, section: &FoSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.features.is_empty() {
        return Vec::new();
    }

    // For each type name, remember the first (feature, symbol, span) we
    // saw. Iteration order over packages/files/items is the source-walk
    // order, so "first" is deterministic for a given AIR.
    struct Incumbent<'a> {
        feature: &'a FoFeature,
        symbol: String,
        #[allow(dead_code)]
        span: AirSpan,
    }
    let mut incumbents: BTreeMap<String, Incumbent<'_>> = BTreeMap::new();

    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            let Some(feature) = owning_feature(&section.features, module_path) else {
                continue;
            };
            for item in &file.items {
                let AirItem::Type(ty) = item else {
                    continue;
                };
                if ty.visibility != Visibility::Public {
                    continue;
                }
                match incumbents.get(&ty.name) {
                    None => {
                        incumbents.insert(
                            ty.name.clone(),
                            Incumbent {
                                feature,
                                symbol: ty.symbol.clone(),
                                span: ty.span.clone(),
                            },
                        );
                    }
                    Some(prev) if std::ptr::eq(prev.feature, feature) => {
                        // Same name, same feature — not a feature-ownership
                        // conflict. (OT may still complain if the symbol is a
                        // duplicate; that's a different paradigm.)
                    }
                    Some(prev) => {
                        out.push(Diagnostic {
                            rule_id: "FO001".to_string(),
                            severity: mode.elevate(Severity::Fatal),
                            span: ty.span.clone(),
                            concept: Some(ty.name.clone()),
                            message: format!(
                                "type `{name}` is defined in both feature `{a}` and feature `{b}`",
                                name = ty.name,
                                a = prev.feature.name,
                                b = feature.name,
                            ),
                            why: vec![
                                format!("`{}` belongs to feature `{}`", ty.symbol, feature.name),
                                format!(
                                    "`{module_path}` matches feature `{}`'s module pattern `{}`",
                                    feature.name, feature.module
                                ),
                                format!(
                                    "feature `{}` already defines a public type `{}` (`{}`)",
                                    prev.feature.name, ty.name, prev.symbol
                                ),
                            ],
                            suggested_fix: Some(format!(
                                "rename this type to a feature-specific name (e.g. \
                                 `{feat_name}::{name}` could become `{feat_pascal}{name}`), or \
                                 move the concept to whichever feature owns it and import it \
                                 from there",
                                feat_name = feature.name,
                                feat_pascal = pascalize(&feature.name),
                                name = ty.name,
                            )),
                        });
                    }
                }
            }
        }
    }
    out
}

/// Find the first feature whose `module` pattern matches `path`. Returns
/// `None` when the path doesn't belong to any declared feature. Mirrors DG's
/// resolver semantics: overlapping `module` patterns are user error and
/// resolve by declaration order.
fn owning_feature<'a>(features: &'a [FoFeature], path: &str) -> Option<&'a FoFeature> {
    features.iter().find(|f| matches_pattern(&f.module, path))
}

/// FO004 — shared type field references a feature-internal symbol.
///
/// For every `AirItem::Type` whose enclosing `AirFile.module_path` matches
/// any pattern in `section.shared_paths`, scan each field's `type_text`
/// for path-like tokens (split on non-identifier characters and `::`)
/// that match any declared feature's `module` pattern. Fires once per
/// (shared type, field, feature-mention).
///
/// The motivating shape: a workspace declares `shared::dto` as a shared
/// module and `crate::billing::*` as a feature. When `shared::dto::Receipt`
/// has a field of type `Vec<crate::billing::Invoice>`, the shared DTO
/// has secretly become billing-coupled — defeating the point of sharing
/// it across other features. The fix is either to move the type into
/// `billing` (where the coupling already lives) or to mediate the
/// coupling through a feature-neutral shape.
///
/// Stays silent when `shared_paths` is empty OR `features` is empty:
/// the rule needs both halves to reason about boundary violations, so
/// silence is the correct posture for un-onboarded codebases.
///
/// Severity: Warning by default; Fatal under `--agent-strict` via
/// [`CheckMode::elevate`]. The decision-tier is "warn-then-discuss":
/// some shared modules legitimately depend on a single feature's types
/// (a billing-event schema in a `shared::events` crate is fine).
pub fn fo004(air: &AirWorkspace, section: &FoSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.shared_paths.is_empty() || section.features.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            if !section
                .shared_paths
                .iter()
                .any(|pat| matches_pattern(pat, module_path))
            {
                continue;
            }
            for item in &file.items {
                let AirItem::Type(ty) = item else {
                    continue;
                };
                for field in &ty.fields {
                    for token in path_like_tokens(&field.type_text) {
                        if let Some(feature) = section
                            .features
                            .iter()
                            .find(|f| matches_pattern(&f.module, token))
                        {
                            out.push(Diagnostic {
                                rule_id: "FO004".to_string(),
                                severity: mode.elevate(Severity::Warning),
                                span: ty.span.clone(),
                                concept: Some(ty.name.clone()),
                                message: format!(
                                    "shared type `{ty_name}` in `{module_path}` has field \
                                     `{field}: {field_type}` referencing feature `{feat}` \
                                     internal symbol `{token}`",
                                    ty_name = ty.name,
                                    field = field.name,
                                    field_type = field.type_text,
                                    feat = feature.name,
                                ),
                                why: vec![
                                    format!(
                                        "type `{}` lives in shared module `{module_path}`",
                                        ty.symbol
                                    ),
                                    format!(
                                        "field `{}` has type text `{}`",
                                        field.name, field.type_text
                                    ),
                                    format!(
                                        "path token `{token}` matches feature `{}`'s \
                                         module pattern `{}`",
                                        feature.name, feature.module
                                    ),
                                    "Feature Ownership: a shared module that names a \
                                     feature-internal type is no longer shared — every \
                                     consumer now indirectly depends on that feature"
                                        .into(),
                                ],
                                suggested_fix: Some(format!(
                                    "either move `{}` into feature `{}` (where the \
                                     coupling already lives) or replace the field's type \
                                     with a feature-neutral DTO. If the coupling is \
                                     intentional (e.g. a billing-event schema), narrow \
                                     `paradigms.FO.shared_paths` so this module is no \
                                     longer treated as shared.",
                                    ty.name, feature.name
                                )),
                            });
                            // Each (field, feature) pair fires at most once.
                            break;
                        }
                    }
                }
            }
        }
    }
    out
}

/// Split `type_text` into path-like tokens — sequences of identifier
/// characters and `::` separators — and yield each non-empty token.
/// Non-identifier characters (`<`, `>`, `,`, `&`, ` `, `(`, `)`, `[`,
/// `]`, `*`, `'`) are treated as separators. Tokens without `::` are
/// still yielded; FO callers feed declared `module` patterns which are
/// segment-aligned, so a single-segment token like `String` cannot
/// accidentally match `crate::billing::*`.
fn path_like_tokens(type_text: &str) -> impl Iterator<Item = &str> {
    type_text
        .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == ':'))
        .filter(|s| !s.is_empty())
}

/// Lower-snake → UpperCamel for the suggested-fix prose. Best-effort: split on
/// `_`/`::`/`-`/whitespace, capitalize each chunk, concatenate. We only use
/// this to nudge the user toward a rename — exact prefix doesn't matter.
fn pascalize(s: &str) -> String {
    s.split(|c: char| c == '_' || c == '-' || c == ':' || c.is_whitespace())
        .filter(|chunk| !chunk.is_empty())
        .map(|chunk| {
            let mut chars = chunk.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => {
                    first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase()
                }
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "rules_tests.rs"]
mod tests;
