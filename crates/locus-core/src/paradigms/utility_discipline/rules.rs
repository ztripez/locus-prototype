//! UT rules.
//!
//! Implemented:
//! - [`ut001`]: utility module defines a public type. A "utility module" by
//!   definition holds domain-free technical helpers; defining a public *type*
//!   in one is a smell because types carry semantics, and semantics belong to
//!   a domain/feature module.
//! - [`ut002`]: utility module imports a forbidden feature/domain path. UT001
//!   catches public types defined in utility modules; UT002 catches helpers
//!   that *know about* domain concepts via imports.
//! - [`ut003`]: new generic-utility-named module without acceptance. Flags
//!   modules whose `module_path` matches one of the configured generic
//!   utility patterns and is not present in `accepted_utility_paths`.
//! - [`ut004`]: domain-concept logic inside a utility module. Fires when a
//!   utility-pathed file constructs (or validates/normalizes) a configured
//!   canonical concept.
//! - [`ut005`]: validation/normalization inside a utility module — same as
//!   UT004 but for any `Validate`/`Normalize` `AirTruthAction`, regardless
//!   of target.

use locus_air::{ActionKind, AirItem, AirWorkspace, Visibility};

use super::lockfile_schema::{UtSection, matches_pattern};
use crate::diagnostics::{CheckMode, Diagnostic, Severity};

/// UT001 — utility module defines a public type.
///
/// For every `AirFile` whose `module_path` matches any pattern in
/// `utility_paths`, fire one diagnostic per public `AirItem::Type`.
///
/// Severity: Warning by default; Fatal under `--agent-strict`. The spec lists
/// this as a heuristic warning — utility modules can legitimately hold private
/// helper types, so the structural fail-fast tier isn't a fit.
pub fn ut001(air: &AirWorkspace, section: &UtSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.utility_paths.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            let Some(pattern) = section
                .utility_paths
                .iter()
                .find(|pat| matches_pattern(pat, module_path))
            else {
                continue;
            };
            for item in &file.items {
                let AirItem::Type(ty) = item else {
                    continue;
                };
                if ty.visibility != Visibility::Public {
                    continue;
                }
                out.push(Diagnostic {
                    rule_id: "UT001".to_string(),
                    severity: mode.elevate(Severity::Warning),
                    span: ty.span.clone(),
                    concept: None,
                    message: format!(
                        "utility module `{module_path}` defines public type `{}` \
                         (matched utility pattern `{pattern}`)",
                        ty.name
                    ),
                    why: vec![
                        format!("module `{module_path}` matches utility pattern `{pattern}`"),
                        format!("public type `{}` (`{}`)", ty.name, ty.symbol),
                        "utility modules must hold only domain-free technical helpers; \
                         public types carry semantics that belong to a domain/feature module"
                            .into(),
                    ],
                    suggested_fix: Some(format!(
                        "move `{}` to a domain/feature module that owns the concept it \
                         represents; if it really is a domain-free helper type, demote it \
                         to private (utility modules can hold private types) or rename the \
                         module so it's no longer marked as utility in \
                         `paradigms.UT.utility_paths`",
                        ty.name
                    )),
                });
            }
        }
    }
    out
}

/// UT002 — utility module imports a forbidden feature/domain path.
///
/// For every `AirFile` whose `module_path` matches any pattern in
/// `utility_paths`, walk its `AirItem::Import` items. Fire when the import
/// path matches any pattern in `forbidden_imports`.
///
/// Severity: Fatal in both modes — a forbidden import declared by the user is
/// a structural violation, mirroring DG001 / BO001.
pub fn ut002(air: &AirWorkspace, section: &UtSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.utility_paths.is_empty() || section.forbidden_imports.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            let Some(utility_pattern) = section
                .utility_paths
                .iter()
                .find(|pat| matches_pattern(pat, module_path))
            else {
                continue;
            };
            for item in &file.items {
                let AirItem::Import(imp) = item else {
                    continue;
                };
                let Some(forbidden_pattern) = section
                    .forbidden_imports
                    .iter()
                    .find(|pat| matches_pattern(pat, &imp.path))
                else {
                    continue;
                };
                out.push(Diagnostic {
                    rule_id: "UT002".to_string(),
                    severity: mode.elevate(Severity::Fatal),
                    span: imp.span.clone(),
                    concept: None,
                    message: format!(
                        "utility module `{module_path}` imports forbidden \
                         feature/domain path `{}`",
                        imp.path
                    ),
                    why: vec![
                        format!(
                            "importer `{module_path}` matches utility_paths pattern \
                             `{utility_pattern}`"
                        ),
                        format!(
                            "import `{}` matches forbidden_imports pattern \
                             `{forbidden_pattern}`",
                            imp.path
                        ),
                        "utility modules must hold only domain-free technical helpers; \
                         importing a feature/domain concept means the helper knows about \
                         semantics that belong to a domain/feature module"
                            .into(),
                    ],
                    suggested_fix: Some(format!(
                        "move the helper that needs `{}` out of the utility module and \
                         into the domain/feature module that owns the concept; if the \
                         dependency is legitimate, remove `{module_path}` from \
                         `paradigms.UT.utility_paths` (or narrow \
                         `paradigms.UT.forbidden_imports`) in `locus.lock`",
                        imp.path
                    )),
                });
            }
        }
    }
    out
}

/// UT003 — new generic-utility-named module without acceptance.
///
/// For every `AirFile` whose `module_path` matches any pattern in
/// `generic_utility_patterns` AND whose `module_path` is *not* present in
/// `accepted_utility_paths`, fire one diagnostic. `accepted_utility_paths`
/// supports the same pattern syntax as `utility_paths` (the user can
/// accept by exact path or by glob).
///
/// Severity: Warning by default; `--agent-strict` elevates to Fatal. The
/// rule goes silent when `generic_utility_patterns` is empty — UT003 is
/// gated on the user explicitly opting in to the generic-naming check.
pub fn ut003(air: &AirWorkspace, section: &UtSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.generic_utility_patterns.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            let Some(matched_pattern) = section
                .generic_utility_patterns
                .iter()
                .find(|p| matches_pattern(p, module_path))
            else {
                continue;
            };
            if section
                .accepted_utility_paths
                .iter()
                .any(|p| matches_pattern(p, module_path))
            {
                continue;
            }
            // Anchor at the file's first item, falling back to line 1.
            let span = file
                .items
                .iter()
                .map(|item| match item {
                    AirItem::Type(t) => t.span.clone(),
                    AirItem::Function(f) => f.span.clone(),
                    AirItem::Import(i) => i.span.clone(),
                    AirItem::Impl(i) => i.span.clone(),
                    AirItem::Conversion(c) => c.span.clone(),
                    AirItem::TruthAction(a) => a.span.clone(),
                    AirItem::Usage(u) => u.span.clone(),
                    AirItem::CallSite(c) => c.span.clone(),
                    AirItem::SilentDiscard(d) => d.span.clone(),
                    AirItem::PartialResultMatch(p) => p.span.clone(),
                    AirItem::MatchArm(a) => a.span.clone(),
                    AirItem::ClosureMethodCall(c) => c.span.clone(),
                    AirItem::FallbackCall(c) => c.span.clone(),
                    AirItem::RetryLoop(l) => l.span.clone(),
                    AirItem::ScrutineeLiteral(l) => l.span.clone(),
                })
                .next()
                .unwrap_or_else(|| locus_air::AirSpan::new(file.path.clone(), 1, 1));
            out.push(Diagnostic {
                rule_id: "UT003".to_string(),
                severity: mode.elevate(Severity::Warning),
                span,
                concept: None,
                message: format!(
                    "module `{module_path}` uses a generic utility name (matched \
                     pattern `{matched_pattern}`) and is not in `accepted_utility_paths`"
                ),
                why: vec![
                    format!(
                        "module `{module_path}` matches generic_utility_patterns \
                         entry `{matched_pattern}`"
                    ),
                    "generic-named modules (`utils`, `helpers`, `common`, `misc`, \
                     `shared`) tend to accumulate unrelated logic; require explicit \
                     acceptance so each one is a deliberate choice"
                        .into(),
                ],
                suggested_fix: Some(format!(
                    "if `{module_path}` is intentionally a utility module, accept it \
                     by adding its path to `paradigms.UT.accepted_utility_paths` in \
                     `locus.lock` (you may also want to add it to `utility_paths` so \
                     UT001/UT002/UT004/UT005 apply). Otherwise rename the module to \
                     reflect its actual responsibility."
                )),
            });
        }
    }
    out
}

/// UT004 — domain-concept logic inside a utility module.
///
/// For each `AirFile` whose `module_path` matches `utility_paths`, fire when
/// the file contains an `AirTruthAction::Construct` whose `target` matches
/// any pattern in `canonical_construct_patterns`, OR any `AirTruthAction`
/// with `action ∈ {Validate, Normalize}`. Validate/Normalize actions don't
/// need a pattern match — any utility doing input validation or
/// normalization is by definition implementing domain rules.
///
/// Severity: Warning by default; `--agent-strict` elevates to Fatal.
///
/// Lockfile-driven silence: stays silent until both `utility_paths` is
/// non-empty AND either `canonical_construct_patterns` is populated *or*
/// the file actually carries Validate/Normalize actions. Specifically,
/// the rule short-circuits when `utility_paths` is empty — same convention
/// as UT001/UT002.
pub fn ut004(air: &AirWorkspace, section: &UtSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.utility_paths.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            let Some(utility_pattern) = section
                .utility_paths
                .iter()
                .find(|p| matches_pattern(p, module_path))
            else {
                continue;
            };
            // UT004 only fires on actions whose `target` matches one of the
            // user's `canonical_construct_patterns` — i.e. the file is doing
            // *concept-aware* logic, not just generic helper work. UT005
            // fires on the broader "any Validate/Normalize" shape so it can
            // catch validation that hasn't been canonicalized yet. The two
            // rules deliberately don't overlap on the same action: an
            // action either targets a known concept (UT004) or it doesn't
            // (UT005's territory).
            for item in &file.items {
                let AirItem::TruthAction(action) = item else {
                    continue;
                };
                let target_is_canonical = section
                    .canonical_construct_patterns
                    .iter()
                    .any(|p| matches_pattern(p, &action.target));
                if !target_is_canonical {
                    continue;
                }
                let label = match action.action {
                    ActionKind::Validate => "validation of a canonical concept",
                    ActionKind::Normalize => "normalization of a canonical concept",
                    ActionKind::Construct => "construction of a canonical concept",
                    _ => continue,
                };
                out.push(Diagnostic {
                    rule_id: "UT004".to_string(),
                    severity: mode.elevate(Severity::Warning),
                    span: action.span.clone(),
                    concept: None,
                    message: format!(
                        "utility module `{module_path}` performs {label} on `{}`",
                        action.target
                    ),
                    why: vec![
                        format!(
                            "module `{module_path}` matches utility_paths pattern \
                             `{utility_pattern}`"
                        ),
                        format!(
                            "found `{:?}` action targeting `{}`",
                            action.action, action.target
                        ),
                        "utility modules must hold only domain-free technical \
                         helpers; constructing canonical concepts or performing \
                         validation/normalization is domain logic that belongs in \
                         a feature/domain module"
                            .into(),
                    ],
                    suggested_fix: Some(format!(
                        "move the {label} of `{}` into the domain/feature module \
                         that owns the concept. If `{module_path}` is genuinely \
                         not a utility, remove it from `paradigms.UT.utility_paths` \
                         in `locus.lock`.",
                        action.target
                    )),
                });
            }
        }
    }
    out
}

/// UT005 — validation/normalization inside a utility module.
///
/// Same gate as UT004 but specifically for `AirTruthAction::{Validate,
/// Normalize}` actions, regardless of target. The two rules overlap (UT004
/// catches Validate/Normalize too) but UT005 stays semantically focused on
/// the "validation/normalization is domain logic" message and lets users
/// silence one without the other (e.g. by excluding the module from
/// `utility_paths` for UT005 only — currently both share the same gate).
///
/// Severity: Warning by default; `--agent-strict` elevates to Fatal.
///
/// Lockfile-driven silence: stays silent when `utility_paths` is empty.
pub fn ut005(air: &AirWorkspace, section: &UtSection, mode: CheckMode) -> Vec<Diagnostic> {
    if section.utility_paths.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            let Some(module_path) = file.module_path.as_deref() else {
                continue;
            };
            let Some(utility_pattern) = section
                .utility_paths
                .iter()
                .find(|p| matches_pattern(p, module_path))
            else {
                continue;
            };
            for item in &file.items {
                let AirItem::TruthAction(action) = item else {
                    continue;
                };
                let label = match action.action {
                    ActionKind::Validate => "validation",
                    ActionKind::Normalize => "normalization",
                    _ => continue,
                };
                // UT004 owns the canonical-target case; UT005 covers the
                // non-canonical residual so the two rules don't double-fire
                // on the same action. If the user hasn't populated
                // `canonical_construct_patterns`, the canonical check is
                // vacuously false — UT005 fires on every Validate/Normalize
                // (the broadest posture, matching the rule's intent).
                let target_is_canonical = section
                    .canonical_construct_patterns
                    .iter()
                    .any(|p| matches_pattern(p, &action.target));
                if target_is_canonical {
                    continue;
                }
                out.push(Diagnostic {
                    rule_id: "UT005".to_string(),
                    severity: mode.elevate(Severity::Warning),
                    span: action.span.clone(),
                    concept: None,
                    message: format!(
                        "utility module `{module_path}` performs {label} on `{}`",
                        action.target
                    ),
                    why: vec![
                        format!(
                            "module `{module_path}` matches utility_paths pattern \
                             `{utility_pattern}`"
                        ),
                        format!(
                            "found `{:?}` action targeting `{}`",
                            action.action, action.target
                        ),
                        "validation and normalization express domain rules; \
                         they belong in a domain/feature module, not a \
                         domain-free utility"
                            .into(),
                    ],
                    suggested_fix: Some(format!(
                        "move the {label} of `{}` into the domain/feature module \
                         that owns the rule. If `{module_path}` is genuinely not \
                         a utility, remove it from `paradigms.UT.utility_paths` \
                         in `locus.lock`.",
                        action.target
                    )),
                });
            }
        }
    }
    out
}

#[cfg(test)]
#[path = "rules_tests.rs"]
mod tests;
