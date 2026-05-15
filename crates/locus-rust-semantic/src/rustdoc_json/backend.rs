//! [`RustdocJsonBackend`] — the [`SemanticAdapter`] impl + its
//! `resolve_conversions` orchestration.
//!
//! Sibling modules:
//! - [`super::cargo_invoke`] — `cargo metadata` / `cargo rustdoc`
//!   shelling and JSON loading.
//! - [`super::walk`] — rustdoc-types → AIR translation.

// locus: ot canonical

use std::path::Path;

use crate::{AdapterError, ResolvedConversion, SemanticAdapter};

use super::cargo_invoke::{list_workspace_packages, parse_rustdoc_json, run_rustdoc_for};
use super::walk::collect_conversions;

/// First concrete [`SemanticAdapter`] backend — produces resolved
/// `From` / `TryFrom` impl facts by shelling out to nightly rustdoc.
pub struct RustdocJsonBackend {
    /// Toolchain spec passed via `cargo +<toolchain>`. Defaults to
    /// `nightly`. Override for CI environments that pin a specific
    /// nightly date.
    toolchain: String,
}

impl RustdocJsonBackend {
    pub fn new() -> Self {
        Self {
            toolchain: "nightly".to_string(),
        }
    }

    /// Override the toolchain used to invoke rustdoc. `"nightly"`,
    /// `"nightly-2026-05-01"`, etc.
    pub fn with_toolchain(mut self, toolchain: impl Into<String>) -> Self {
        self.toolchain = toolchain.into();
        self
    }
}

impl Default for RustdocJsonBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl SemanticAdapter for RustdocJsonBackend {
    fn name(&self) -> &'static str {
        "rustdoc-json"
    }

    fn resolve_conversions(
        &self,
        workspace_root: &Path,
    ) -> Result<Vec<ResolvedConversion>, AdapterError> {
        let manifest = workspace_root.join("Cargo.toml");
        if !manifest.exists() {
            return Err(AdapterError::WorkspaceFailed {
                message: format!("no Cargo.toml at {}", manifest.display()),
            });
        }

        let packages = list_workspace_packages(&self.toolchain, workspace_root)?;
        let mut out = Vec::new();
        for package in &packages {
            let json_path = run_rustdoc_for(&self.toolchain, workspace_root, package)?;
            let krate = parse_rustdoc_json(&json_path)?;
            collect_conversions(&krate, workspace_root, &mut out);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_name_matches_semantic_backend_variant() {
        // The trait's `name()` must align with the AIR provenance enum
        // variant the backend's emissions carry. Pin it.
        let backend = RustdocJsonBackend::new();
        assert_eq!(backend.name(), "rustdoc-json");
        // The variant the backend tags its records with:
        let provenance_variant = locus_air::SemanticBackend::RustdocJson;
        // Round-trip via serde to confirm the kebab-case discriminant
        // lines up with the human-readable backend name.
        let serialised = serde_json::to_value(provenance_variant).unwrap();
        assert_eq!(serialised, serde_json::Value::String("rustdoc-json".into()));
    }
}
