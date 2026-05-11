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

/// Per-name incumbent tracker for FO001.
struct Fo001Incumbent<'a> {
    feature: &'a FoFeature,
    symbol: String,
    #[allow(dead_code)]
    span: AirSpan,
}

fn fo001_diagnostic(
    ty: &locus_air::AirType,
    module_path: &str,
    feature: &FoFeature,
    prev_feature_name: &str,
    prev_symbol: &str,
    mode: CheckMode,
) -> Diagnostic {
    Diagnostic {
        rule_id: "FO001".to_string(),
        severity: mode.elevate(Severity::Fatal),
        span: ty.span.clone(),
        concept: Some(ty.name.clone()),
        message: format!(
            "type `{}` is defined in both feature `{prev_feature_name}` and feature `{}`",
            ty.name, feature.name,
        ),
        why: vec![
            format!("`{}` belongs to feature `{}`", ty.symbol, feature.name),
            format!(
                "`{module_path}` matches feature `{}`'s module pattern `{}`",
                feature.name, feature.module
            ),
            format!(
                "feature `{prev_feature_name}` already defines a public type `{}` (`{prev_symbol}`)",
                ty.name,
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
    }
}

/// FO001 — same public type name defined in two different features.
///
/// Groups public types by name across feature-owned modules. Fires once per
/// non-incumbent duplicate (second, third, etc. feature to define the name).
/// Always Fatal — at most one feature can own the canonical concept.
pub fn fo001(air: &AirWorkspace, section: &FoSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.features.is_empty() {
        return Vec::new();
    }
    let mut incumbents: BTreeMap<String, Fo001Incumbent<'_>> = BTreeMap::new();

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
                            Fo001Incumbent {
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
                        out.push(fo001_diagnostic(
                            ty,
                            module_path,
                            feature,
                            &prev.feature.name,
                            &prev.symbol,
                            mode,
                        ));
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

fn fo004_diagnostic(
    ty: &locus_air::AirType,
    module_path: &str,
    field: &locus_air::AirField,
    token: &str,
    feature: &FoFeature,
    mode: CheckMode,
) -> Diagnostic {
    Diagnostic {
        rule_id: "FO004".to_string(),
        severity: mode.elevate(Severity::Warning),
        span: ty.span.clone(),
        concept: Some(ty.name.clone()),
        message: format!(
            "shared type `{}` in `{module_path}` has field `{}`: `{}` \
             referencing feature `{}` internal symbol `{token}`",
            ty.name, field.name, field.type_text, feature.name,
        ),
        why: vec![
            format!("type `{}` lives in shared module `{module_path}`", ty.symbol),
            format!("field `{}` has type text `{}`", field.name, field.type_text),
            format!(
                "path token `{token}` matches feature `{}`'s module pattern `{}`",
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
    }
}

/// FO004 — shared type field references a feature-internal symbol.
///
/// For each type in a `shared_paths` module, splits field `type_text` into
/// path-like tokens and fires when any token matches a declared feature's
/// `module` pattern. Silent when `shared_paths` or `features` is empty.
///
/// Severity: Warning; Fatal under `--agent-strict`.
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
                            out.push(fo004_diagnostic(
                                ty, module_path, field, token, feature, mode,
                            ));
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
mod rules_tests;
