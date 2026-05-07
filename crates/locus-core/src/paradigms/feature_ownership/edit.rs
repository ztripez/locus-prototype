//! `locus fo ...` — symbol-by-symbol mutators for the FO lockfile section.
//!
//! Mirror of DG's `define_feature` shape: features are identity-keyed by
//! `name` and refuse silent overwrites unless `force` is set.

use thiserror::Error;

use super::lockfile_schema::{FoFeature, FoSection};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FoEditError {
    #[error("feature name must not be empty")]
    EmptyName,
    #[error("feature module pattern must not be empty")]
    EmptyModule,
    #[error("feature `{0}` already exists; pass --force to overwrite its module pattern")]
    DuplicateName(String),
}

/// Define (or overwrite, with `force`) a named feature region. Identity is
/// the `name`; refuses duplicates without `force`, replaces the `module`
/// pattern when `force` is set.
pub fn define_feature(
    section: &mut FoSection,
    name: &str,
    module: &str,
    force: bool,
) -> Result<(), FoEditError> {
    if name.is_empty() {
        return Err(FoEditError::EmptyName);
    }
    if module.is_empty() {
        return Err(FoEditError::EmptyModule);
    }
    if let Some(existing) = section.features.iter_mut().find(|f| f.name == name) {
        if !force {
            return Err(FoEditError::DuplicateName(name.to_string()));
        }
        existing.module = module.to_string();
        return Ok(());
    }
    section.features.push(FoFeature {
        name: name.to_string(),
        module: module.to_string(),
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn define_feature_appends_a_new_entry() {
        let mut section = FoSection::default();
        define_feature(&mut section, "billing", "billing::*", false).unwrap();
        assert_eq!(section.features.len(), 1);
        assert_eq!(section.features[0].name, "billing");
        assert_eq!(section.features[0].module, "billing::*");
    }

    #[test]
    fn define_feature_rejects_duplicate_without_force() {
        let mut section = FoSection::default();
        define_feature(&mut section, "billing", "billing::*", false).unwrap();
        let err = define_feature(&mut section, "billing", "billing_v2::*", false).unwrap_err();
        assert!(matches!(err, FoEditError::DuplicateName(_)));
        assert_eq!(section.features[0].module, "billing::*");
    }

    #[test]
    fn define_feature_with_force_overwrites_module() {
        let mut section = FoSection::default();
        define_feature(&mut section, "billing", "billing::*", false).unwrap();
        define_feature(&mut section, "billing", "billing_v2::*", true).unwrap();
        assert_eq!(section.features.len(), 1);
        assert_eq!(section.features[0].module, "billing_v2::*");
    }

    #[test]
    fn define_feature_rejects_empty_inputs() {
        let mut section = FoSection::default();
        assert_eq!(
            define_feature(&mut section, "", "x::*", false).unwrap_err(),
            FoEditError::EmptyName
        );
        assert_eq!(
            define_feature(&mut section, "x", "", false).unwrap_err(),
            FoEditError::EmptyModule
        );
    }
}
