//! Rust source adapter: scans a Cargo workspace and emits AIR.

use std::path::{Path, PathBuf};

use locus_air::{AirFile, AirPackage, AirWorkspace};
use thiserror::Error;
use walkdir::WalkDir;

mod hints;
pub mod loaders;
mod module_path;
mod type_render;
mod visitor;

pub use hints::scan_hints;
pub use loaders::StdRtLoader;
pub use module_path::{derive_module_path, package_to_crate_name};
pub use type_render::{render_path, render_type};
pub use visitor::collect_items;

#[derive(Debug, Error)]
pub enum ScanError {
    #[error("cargo metadata failed: {0}")]
    CargoMetadata(#[from] cargo_metadata::Error),
    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Scan a Cargo workspace and return an enriched [`AirWorkspace`].
///
/// Runs the language-shaped visitor (producing types, functions,
/// imports, call-sites, ...) and then applies all default loaders
/// ([`default_loaders`]) which translate framework-shaped call patterns
/// into normalized [`locus_air::AirFact`] entries on the workspace.
///
/// Use [`scan_raw`] when you want the visitor output without the
/// loader tier (useful for snapshot tests / introspection).
pub fn scan(workspace_root: &Path) -> Result<AirWorkspace, ScanError> {
    let mut air = scan_raw(workspace_root)?;
    apply_default_loaders(&mut air);
    Ok(air)
}

/// Lower-level scan that runs the visitor only — no loaders, no facts.
/// Useful for tests / tooling that want to inspect raw AIR without
/// loader-derived noise.
pub fn scan_raw(workspace_root: &Path) -> Result<AirWorkspace, ScanError> {
    let manifest = workspace_root.join("Cargo.toml");
    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(&manifest)
        .no_deps()
        .exec()?;

    let workspace_members: std::collections::HashSet<_> =
        metadata.workspace_members.iter().collect();

    let mut packages = Vec::new();
    for pkg in &metadata.packages {
        if !workspace_members.contains(&pkg.id) {
            continue;
        }
        let pkg_root = pkg
            .manifest_path
            .parent()
            .map(|p| p.as_std_path().to_path_buf())
            .unwrap_or_else(|| workspace_root.to_path_buf());

        let crate_name = lib_crate_name(pkg);
        let files = collect_files(&pkg_root, &crate_name)?;
        packages.push(AirPackage {
            name: pkg.name.clone(),
            version: pkg.version.to_string(),
            root_dir: pkg_root.to_string_lossy().into_owned(),
            files,
        });
    }

    Ok(AirWorkspace::new(packages))
}

/// The default loader stack applied by [`scan`]. Currently a single
/// [`StdRtLoader`]; framework-specific loaders (reqwest, sqlx, ...) will
/// be added here as they land.
pub fn default_loaders() -> Vec<Box<dyn locus_core::Loader>> {
    vec![Box::new(loaders::std_rt::StdRtLoader)]
}

fn apply_default_loaders(air: &mut AirWorkspace) {
    let loaders = default_loaders();
    locus_core::loaders::apply_loaders(air, &loaders);
}

/// Pick the crate name to use as the symbol prefix for `pkg`. Prefers an
/// explicit `lib` target name (which may differ from the package name), then
/// the cargo package name with hyphens converted to underscores.
fn lib_crate_name(pkg: &cargo_metadata::Package) -> String {
    pkg.targets
        .iter()
        .find(|t| {
            t.kind
                .iter()
                .any(|k| k == "lib" || k == "rlib" || k == "proc-macro")
        })
        .map(|t| t.name.clone())
        .unwrap_or_else(|| package_to_crate_name(&pkg.name))
}

fn collect_files(pkg_root: &Path, crate_name: &str) -> Result<Vec<AirFile>, ScanError> {
    let src_dir = pkg_root.join("src");
    if !src_dir.exists() {
        return Ok(Vec::new());
    }

    let mut files = Vec::new();
    for entry in WalkDir::new(&src_dir).into_iter().filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("rs") {
            continue;
        }
        if is_generated_path(path) {
            continue;
        }
        files.push(scan_file(pkg_root, path, crate_name)?);
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

fn is_generated_path(path: &Path) -> bool {
    let s = path.to_string_lossy();
    s.contains("/generated/")
        || s.contains("/build/")
        || path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.ends_with("_generated.rs"))
            .unwrap_or(false)
}

fn scan_file(pkg_root: &Path, path: &Path, crate_name: &str) -> Result<AirFile, ScanError> {
    let source = std::fs::read_to_string(path).map_err(|source| ScanError::Io {
        path: path.to_path_buf(),
        source,
    })?;

    let path_str = path.to_string_lossy().into_owned();
    let module_path = derive_module_path(pkg_root, path, crate_name);
    let hints = scan_hints(&source, &path_str);
    let line_count = source.lines().count() as u32;

    match syn::parse_file(&source) {
        Ok(file) => {
            let items = collect_items(&file, &path_str, module_path.as_deref());
            Ok(AirFile {
                path: path_str,
                module_path,
                items,
                hints,
                parse_error: None,
                line_count,
            })
        }
        Err(err) => Ok(AirFile {
            path: path_str,
            module_path,
            items: Vec::new(),
            hints,
            parse_error: Some(err.to_string()),
            line_count,
        }),
    }
}
