//! `locus dg ...` — symbol-by-symbol mutators for the DG lockfile section.
//!
//! Mirror of OT's `accept` module. All operations validate the inputs and
//! refuse silent overwrites unless `force` is set.

use thiserror::Error;

use super::lockfile_schema::{DgSection, FeatureDefinition, ForbiddenEdge};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DgEditError {
    #[error("`from` pattern must not be empty")]
    EmptyFrom,
    #[error("`to` pattern must not be empty")]
    EmptyTo,
    #[error(
        "edge `{from}` -> `{to}` already exists in forbidden_edges; pass --force to update its reason"
    )]
    DuplicateEdge { from: String, to: String },
    #[error("feature name must not be empty")]
    EmptyFeatureName,
    #[error("feature module pattern must not be empty")]
    EmptyFeatureModule,
    #[error(
        "feature `{0}` already exists; pass --force to overwrite its module pattern and public_api"
    )]
    DuplicateFeature(String),
    #[error("shared path pattern must not be empty")]
    EmptySharedPath,
}

/// Add a forbidden edge to the DG section. If an edge with the same
/// `(from, to)` pair already exists, refuse unless `force` is set; with
/// `force`, the edge's reason is overwritten with the new one.
pub fn forbid_edge(
    section: &mut DgSection,
    from: &str,
    to: &str,
    reason: Option<&str>,
    force: bool,
) -> Result<(), DgEditError> {
    if from.is_empty() {
        return Err(DgEditError::EmptyFrom);
    }
    if to.is_empty() {
        return Err(DgEditError::EmptyTo);
    }
    if let Some(existing) = section
        .forbidden_edges
        .iter_mut()
        .find(|e| e.from == from && e.to == to)
    {
        if !force {
            return Err(DgEditError::DuplicateEdge {
                from: from.into(),
                to: to.into(),
            });
        }
        existing.reason = reason.map(str::to_string);
        return Ok(());
    }
    section.forbidden_edges.push(ForbiddenEdge {
        from: from.into(),
        to: to.into(),
        reason: reason.map(str::to_string),
    });
    Ok(())
}

/// Define (or overwrite, with `force`) a named feature. Sets `module` and
/// `public_api` patterns so DG003 can decide whether cross-feature imports
/// are legal.
pub fn define_feature(
    section: &mut DgSection,
    name: &str,
    module: &str,
    public_api: &[String],
    force: bool,
) -> Result<(), DgEditError> {
    if name.is_empty() {
        return Err(DgEditError::EmptyFeatureName);
    }
    if module.is_empty() {
        return Err(DgEditError::EmptyFeatureModule);
    }
    if let Some(existing) = section.features.iter_mut().find(|f| f.name == name) {
        if !force {
            return Err(DgEditError::DuplicateFeature(name.to_string()));
        }
        existing.module = module.to_string();
        existing.public_api = public_api.to_vec();
        return Ok(());
    }
    section.features.push(FeatureDefinition {
        name: name.to_string(),
        module: module.to_string(),
        public_api: public_api.to_vec(),
    });
    Ok(())
}

/// Append a shared-paths pattern. Duplicate patterns are silently deduped —
/// no value in erroring; the user already declared this once.
pub fn add_shared_path(section: &mut DgSection, pattern: &str) -> Result<(), DgEditError> {
    if pattern.is_empty() {
        return Err(DgEditError::EmptySharedPath);
    }
    if !section.shared_paths.iter().any(|p| p == pattern) {
        section.shared_paths.push(pattern.to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forbid_edge_appends_a_new_entry() {
        let mut section = DgSection::default();
        forbid_edge(&mut section, "lore::domain::*", "lore::api::*", None, false).unwrap();
        assert_eq!(section.forbidden_edges.len(), 1);
        assert_eq!(section.forbidden_edges[0].from, "lore::domain::*");
        assert_eq!(section.forbidden_edges[0].to, "lore::api::*");
        assert_eq!(section.forbidden_edges[0].reason, None);
    }

    #[test]
    fn forbid_edge_records_reason() {
        let mut section = DgSection::default();
        forbid_edge(
            &mut section,
            "lore::domain::*",
            "lore::api::*",
            Some("layered architecture"),
            false,
        )
        .unwrap();
        assert_eq!(
            section.forbidden_edges[0].reason.as_deref(),
            Some("layered architecture")
        );
    }

    #[test]
    fn forbid_edge_rejects_duplicates_without_force() {
        let mut section = DgSection::default();
        forbid_edge(&mut section, "a::*", "b::*", None, false).unwrap();
        let err = forbid_edge(&mut section, "a::*", "b::*", None, false).unwrap_err();
        assert!(matches!(err, DgEditError::DuplicateEdge { .. }));
        assert_eq!(section.forbidden_edges.len(), 1);
    }

    #[test]
    fn forbid_edge_with_force_updates_reason() {
        let mut section = DgSection::default();
        forbid_edge(&mut section, "a::*", "b::*", Some("first reason"), false).unwrap();
        forbid_edge(&mut section, "a::*", "b::*", Some("better reason"), true).unwrap();
        assert_eq!(section.forbidden_edges.len(), 1);
        assert_eq!(
            section.forbidden_edges[0].reason.as_deref(),
            Some("better reason")
        );
    }

    #[test]
    fn forbid_edge_rejects_empty_patterns() {
        let mut section = DgSection::default();
        assert_eq!(
            forbid_edge(&mut section, "", "b::*", None, false).unwrap_err(),
            DgEditError::EmptyFrom
        );
        assert_eq!(
            forbid_edge(&mut section, "a::*", "", None, false).unwrap_err(),
            DgEditError::EmptyTo
        );
    }

    // ---- define_feature ----

    #[test]
    fn define_feature_appends_a_new_entry() {
        let mut section = DgSection::default();
        define_feature(
            &mut section,
            "billing",
            "billing::*",
            &["billing::api::*".into()],
            false,
        )
        .unwrap();
        assert_eq!(section.features.len(), 1);
        assert_eq!(section.features[0].name, "billing");
        assert_eq!(section.features[0].module, "billing::*");
        assert_eq!(
            section.features[0].public_api,
            vec!["billing::api::*".to_string()]
        );
    }

    #[test]
    fn define_feature_rejects_duplicate_without_force() {
        let mut section = DgSection::default();
        define_feature(&mut section, "billing", "billing::*", &[], false).unwrap();
        let err = define_feature(&mut section, "billing", "billing_v2::*", &[], false).unwrap_err();
        assert!(matches!(err, DgEditError::DuplicateFeature(_)));
    }

    #[test]
    fn define_feature_with_force_overwrites_module_and_api() {
        let mut section = DgSection::default();
        define_feature(
            &mut section,
            "billing",
            "billing::*",
            &["billing::api::*".into()],
            false,
        )
        .unwrap();
        define_feature(
            &mut section,
            "billing",
            "billing_v2::*",
            &["billing_v2::api::*".into(), "billing_v2::events::*".into()],
            true,
        )
        .unwrap();
        assert_eq!(section.features.len(), 1);
        assert_eq!(section.features[0].module, "billing_v2::*");
        assert_eq!(section.features[0].public_api.len(), 2);
    }

    #[test]
    fn define_feature_rejects_empty_inputs() {
        let mut section = DgSection::default();
        assert_eq!(
            define_feature(&mut section, "", "x::*", &[], false).unwrap_err(),
            DgEditError::EmptyFeatureName
        );
        assert_eq!(
            define_feature(&mut section, "x", "", &[], false).unwrap_err(),
            DgEditError::EmptyFeatureModule
        );
    }

    // ---- add_shared_path ----

    #[test]
    fn add_shared_path_appends_and_dedupes() {
        let mut section = DgSection::default();
        add_shared_path(&mut section, "core::*").unwrap();
        add_shared_path(&mut section, "common::*").unwrap();
        add_shared_path(&mut section, "core::*").unwrap(); // duplicate, silently deduped
        assert_eq!(section.shared_paths, vec!["core::*", "common::*"]);
    }

    #[test]
    fn add_shared_path_rejects_empty() {
        let mut section = DgSection::default();
        assert_eq!(
            add_shared_path(&mut section, "").unwrap_err(),
            DgEditError::EmptySharedPath
        );
    }
}
