//! PA rule implementations.
//!
//! Implemented:
//! - [`pa001`]: trait declared and immediately implemented in the same file
//!   (co-located port and adapter — the port wasn't actually abstracted).

use std::collections::BTreeMap;

use locus_air::{AirImpl, AirItem, AirWorkspace, TypeKind};

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
                        ty.name, imp.self_ty, ty.span.file
                    ),
                    why: vec![
                        format!("trait `{}` declared in `{}`", ty.symbol, ty.span.file),
                        format!(
                            "sole impl is `impl {} for {}` in the same file",
                            ty.name, imp.self_ty
                        ),
                        "no `accepted_colocated_traits` pattern matched".into(),
                    ],
                    suggested_fix: Some(format!(
                        "move `{}` to a ports module (typically `application::ports::*`) and the \
                         impl for `{}` to an adapter/infrastructure module; if this trait is a \
                         genuine utility helper rather than a port, accept it via \
                         `paradigms.PA.accepted_colocated_traits` in `locus.lock`",
                        ty.name, imp.self_ty
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
fn build_trait_to_impls(air: &AirWorkspace) -> BTreeMap<&str, Vec<&AirImpl>> {
    let mut out: BTreeMap<&str, Vec<&AirImpl>> = BTreeMap::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                let AirItem::Impl(imp) = item else {
                    continue;
                };
                let Some(tp) = imp.trait_path.as_deref() else {
                    continue;
                };
                let short = tp.rsplit("::").next().unwrap_or(tp);
                out.entry(short).or_default().push(imp);
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
            derives: Vec::new(),
            attrs: Vec::new(),
            span: AirSpan::new(file, line, line),
            doc: None,
        })
    }

    fn impl_item(trait_path: Option<&str>, self_ty: &str, file: &str, line: u32) -> AirItem {
        AirItem::Impl(AirImpl {
            trait_path: trait_path.map(|s| s.to_string()),
            self_ty: self_ty.into(),
            method_names: Vec::new(),
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
}
