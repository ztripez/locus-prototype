use std::path::Path;

/// Derive a `<crate_name>::a::b` style path from a file's location relative
/// to the package root. Returns `None` if the file isn't under
/// `<pkg_root>/src/`.
///
/// `crate_name` is the package's lib name (Rust identifier — hyphens already
/// converted to underscores by the caller). Using the package name rather
/// than the literal `crate` keyword is what makes symbols globally unique
/// across a workspace; otherwise two crates can both produce
/// `crate::user::User` and collide in the lockfile.
///
/// Phase 1 only handles the common cases:
/// - `src/lib.rs`, `src/main.rs` → `<crate_name>`
/// - `src/foo.rs` → `<crate_name>::foo`
/// - `src/foo/mod.rs` → `<crate_name>::foo`
/// - `src/foo/bar.rs` → `<crate_name>::foo::bar`
///
/// Does not resolve `#[path = "..."]` overrides; those need full mod-tree
/// inspection and belong to a later phase.
pub fn derive_module_path(pkg_root: &Path, file: &Path, crate_name: &str) -> Option<String> {
    let src = pkg_root.join("src");
    let rel = file.strip_prefix(&src).ok()?;
    let mut components: Vec<&str> = rel
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect();

    let last = components.pop()?;
    let stem = last.strip_suffix(".rs")?;
    match stem {
        "lib" | "main" if components.is_empty() => Some(crate_name.to_string()),
        "mod" => {
            if components.is_empty() {
                Some(crate_name.to_string())
            } else {
                Some(format!("{crate_name}::{}", components.join("::")))
            }
        }
        other => {
            if components.is_empty() {
                Some(format!("{crate_name}::{other}"))
            } else {
                Some(format!("{crate_name}::{}::{other}", components.join("::")))
            }
        }
    }
}

/// Convert a Cargo package name to its Rust identifier form (hyphens →
/// underscores). This is what `cargo` itself uses as the default lib name.
pub fn package_to_crate_name(pkg_name: &str) -> String {
    pkg_name.replace('-', "_")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn check(file: &str, expected: &str) {
        let pkg = PathBuf::from("/pkg");
        let f = pkg.join(file);
        assert_eq!(
            derive_module_path(&pkg, &f, "my_crate").as_deref(),
            Some(expected)
        );
    }

    #[test]
    fn lib_rs_is_crate_root() {
        check("src/lib.rs", "my_crate")
    }
    #[test]
    fn main_rs_is_crate_root() {
        check("src/main.rs", "my_crate")
    }
    #[test]
    fn flat_module() {
        check("src/foo.rs", "my_crate::foo")
    }
    #[test]
    fn nested_module() {
        check("src/foo/bar.rs", "my_crate::foo::bar")
    }
    #[test]
    fn mod_rs_uses_directory_name() {
        check("src/foo/mod.rs", "my_crate::foo")
    }
    #[test]
    fn returns_none_for_files_outside_src() {
        let pkg = PathBuf::from("/pkg");
        let outside = pkg.join("benches/bench.rs");
        assert_eq!(derive_module_path(&pkg, &outside, "my_crate"), None);
    }

    #[test]
    fn hyphenated_package_name_becomes_underscored() {
        assert_eq!(package_to_crate_name("sample-crate"), "sample_crate");
        assert_eq!(package_to_crate_name("bevy-mcp-broker"), "bevy_mcp_broker");
        assert_eq!(package_to_crate_name("plain"), "plain");
    }
}
