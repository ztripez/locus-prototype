//! rustdoc-types JSON → [`ResolvedConversion`] walker.
//!
//! All the "translate rustdoc's resolved view into Locus's AIR shape"
//! logic lives here, separate from the cargo / rustdoc invocation
//! machinery in [`super`]. Splitting them keeps each module focused on
//! one architectural concern (process IO vs JSON parsing).

// locus: ot canonical

use std::path::Path;

use locus_air::{AirSpan, ConversionMechanism, SemanticBackend};
use rustdoc_types::{Crate, GenericArg, GenericArgs, Item, ItemEnum, Type};

use crate::ResolvedConversion;

pub(super) fn collect_conversions(
    krate: &Crate,
    workspace_root: &Path,
    out: &mut Vec<ResolvedConversion>,
) {
    for item in krate.index.values() {
        let ItemEnum::Impl(impl_block) = &item.inner else {
            continue;
        };
        // Skip compiler-implied impls (auto-traits like Sync/Send) and
        // materialised blanket impls (`impl<T> From<T> for T` projected
        // onto each concrete type). Only user-written `From` / `TryFrom`
        // impls have `is_synthetic == false` AND `blanket_impl == None`.
        if impl_block.is_synthetic || impl_block.blanket_impl.is_some() {
            continue;
        }
        let Some(trait_path) = &impl_block.trait_ else {
            continue;
        };
        let Some(mechanism) = conversion_mechanism_for_trait(&trait_path.path) else {
            continue;
        };
        let Some(from_type) = extract_from_type_arg(trait_path.args.as_deref()) else {
            continue;
        };
        let from_canonical = render_type_canonical(&from_type, krate);
        let to_canonical = render_type_canonical(&impl_block.for_, krate);
        let span = air_span_from_item(item, workspace_root);
        let symbol = format!(
            "impl {} for {}",
            render_trait_path(trait_path),
            to_canonical
        );
        out.push(ResolvedConversion::new(
            from_canonical,
            to_canonical,
            mechanism,
            symbol,
            span,
            SemanticBackend::RustdocJson,
        ));
    }
}

/// Recognise the trait identity from its rustdoc-resolved path.
///
/// Locked to the exact stdlib trait paths so a user-defined trait
/// named `mycrate::From` (or any other suffix-collision) is not
/// mistaken for a conversion. rustdoc's resolved path is the trait's
/// **defining** path, so we get `core::convert::From` for what was
/// written as `impl From<T>`. Both `core::convert::*` and
/// `std::convert::*` are accepted because rustdoc occasionally chooses
/// `std::` over `core::` depending on what's been imported; the two
/// re-export the same trait, but the resolved path string differs.
///
/// Plain `From` / `TryFrom` (no `::` prefix) appear when the trait is
/// referenced through Rust's prelude. The rustdoc JSON resolves the
/// import to its definition, so this is rare in practice — but we
/// accept the bare form as well so prelude-imported impls don't slip
/// through silently.
pub(super) fn conversion_mechanism_for_trait(path: &str) -> Option<ConversionMechanism> {
    const FROM_PATHS: &[&str] = &["core::convert::From", "std::convert::From", "From"];
    const TRY_FROM_PATHS: &[&str] = &["core::convert::TryFrom", "std::convert::TryFrom", "TryFrom"];
    if FROM_PATHS.contains(&path) {
        return Some(ConversionMechanism::InfallibleAdapter);
    }
    if TRY_FROM_PATHS.contains(&path) {
        return Some(ConversionMechanism::FallibleAdapter);
    }
    None
}

/// `impl From<T> for U` — pull `T` out of the trait path's generic args.
fn extract_from_type_arg(args: Option<&GenericArgs>) -> Option<Type> {
    let GenericArgs::AngleBracketed { args, .. } = args? else {
        return None;
    };
    for arg in args {
        if let GenericArg::Type(t) = arg {
            return Some(t.clone());
        }
    }
    None
}

/// Render a [`Type`] using its **canonical** path when available.
///
/// rustdoc's `Path.path` is *what was written* (`Vec` if the user wrote
/// `Vec`, `std::vec::Vec` if they wrote that). We override that with
/// the canonical path from `crate.paths` (e.g. `std::vec::Vec`) so the
/// `ResolvedConversion` carries the resolved identity, not the source
/// surface text — that's the whole point of the semantic backend.
fn render_type_canonical(ty: &Type, krate: &Crate) -> String {
    match ty {
        Type::ResolvedPath(p) => {
            let canonical = krate
                .paths
                .get(&p.id)
                .map(|summary| summary.path.join("::"))
                .unwrap_or_else(|| p.path.clone());
            render_path_with_generics(&canonical, p.args.as_deref(), krate)
        }
        Type::Primitive(name) | Type::Generic(name) => name.clone(),
        Type::Tuple(parts) => {
            let rendered: Vec<String> = parts
                .iter()
                .map(|t| render_type_canonical(t, krate))
                .collect();
            format!("({})", rendered.join(", "))
        }
        Type::Slice(inner) => format!("[{}]", render_type_canonical(inner, krate)),
        Type::Array { type_, len } => {
            format!("[{}; {}]", render_type_canonical(type_, krate), len)
        }
        Type::BorrowedRef {
            lifetime,
            is_mutable,
            type_,
        } => {
            let life = lifetime
                .as_ref()
                .map(|l| format!("{l} "))
                .unwrap_or_default();
            let mutability = if *is_mutable { "mut " } else { "" };
            format!(
                "&{life}{mutability}{inner}",
                inner = render_type_canonical(type_, krate)
            )
        }
        // Other variants are rare in From / TryFrom positions; fall back
        // to debug representation rather than dropping the row. Phase 3
        // can revisit if real workspaces show meaningful frequencies.
        _ => format!("{ty:?}"),
    }
}

fn render_path_with_generics(canonical: &str, args: Option<&GenericArgs>, krate: &Crate) -> String {
    let Some(args) = args else {
        return canonical.to_string();
    };
    match args {
        GenericArgs::AngleBracketed { args, .. } if !args.is_empty() => {
            let rendered: Vec<String> = args
                .iter()
                .filter_map(|a| match a {
                    GenericArg::Type(t) => Some(render_type_canonical(t, krate)),
                    GenericArg::Lifetime(l) => Some(l.clone()),
                    GenericArg::Const(_) => None,
                    GenericArg::Infer => Some("_".to_string()),
                })
                .collect();
            if rendered.is_empty() {
                canonical.to_string()
            } else {
                format!("{canonical}<{}>", rendered.join(", "))
            }
        }
        _ => canonical.to_string(),
    }
}

fn render_trait_path(path: &rustdoc_types::Path) -> String {
    let mut out = path.path.clone();
    if let Some(args) = path.args.as_deref()
        && let GenericArgs::AngleBracketed { args, .. } = args
        && !args.is_empty()
    {
        let parts: Vec<String> = args
            .iter()
            .filter_map(|a| match a {
                GenericArg::Type(Type::ResolvedPath(p)) => Some(p.path.clone()),
                GenericArg::Type(Type::Primitive(n)) | GenericArg::Type(Type::Generic(n)) => {
                    Some(n.clone())
                }
                _ => None,
            })
            .collect();
        if !parts.is_empty() {
            out.push('<');
            out.push_str(&parts.join(", "));
            out.push('>');
        }
    }
    out
}

fn air_span_from_item(item: &Item, workspace_root: &Path) -> AirSpan {
    if let Some(span) = item.span.as_ref() {
        // rustdoc's span.filename is relative to where rustdoc was
        // invoked (the workspace root). Keep paths workspace-relative
        // so they match the syntactic adapter's emissions.
        let path = span.filename.to_string_lossy().into_owned();
        let line_start = span.begin.0 as u32;
        let line_end = span.end.0 as u32;
        return AirSpan::new(path, line_start, line_end);
    }
    // No span (macro-expanded item, etc.) — fall back to the workspace
    // root so consumers still get *something*, but it won't navigate.
    AirSpan::new(workspace_root.to_string_lossy().into_owned(), 1, 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversion_mechanism_accepts_stdlib_from_and_tryfrom() {
        assert_eq!(
            conversion_mechanism_for_trait("core::convert::From"),
            Some(ConversionMechanism::InfallibleAdapter),
        );
        assert_eq!(
            conversion_mechanism_for_trait("std::convert::From"),
            Some(ConversionMechanism::InfallibleAdapter),
        );
        assert_eq!(
            conversion_mechanism_for_trait("core::convert::TryFrom"),
            Some(ConversionMechanism::FallibleAdapter),
        );
        assert_eq!(
            conversion_mechanism_for_trait("std::convert::TryFrom"),
            Some(ConversionMechanism::FallibleAdapter),
        );
        // Prelude-imported bare names are also stdlib-defined under
        // the hood — rustdoc usually resolves them to the full path,
        // but the bare form is accepted as a fallback.
        assert_eq!(
            conversion_mechanism_for_trait("From"),
            Some(ConversionMechanism::InfallibleAdapter),
        );
        assert_eq!(
            conversion_mechanism_for_trait("TryFrom"),
            Some(ConversionMechanism::FallibleAdapter),
        );
    }

    #[test]
    fn conversion_mechanism_rejects_user_defined_traits_named_from() {
        // The whole point of the allowlist: a user-defined trait named
        // `From` outside the stdlib must NOT be classified as a
        // conversion. This guards against the suffix-only regression
        // reported on PR #119.
        assert_eq!(
            conversion_mechanism_for_trait("mycrate::From"),
            None,
            "user-defined `mycrate::From` must not match the stdlib trait",
        );
        assert_eq!(
            conversion_mechanism_for_trait("some_crate::convert::From"),
            None,
            "any `convert::From` outside core/std must not match",
        );
        assert_eq!(
            conversion_mechanism_for_trait("a::b::c::TryFrom"),
            None,
            "user-defined `TryFrom` must not match",
        );
    }

    #[test]
    fn conversion_mechanism_rejects_unrelated_traits() {
        assert_eq!(
            conversion_mechanism_for_trait("foo::bar::SomethingElse"),
            None,
        );
        assert_eq!(conversion_mechanism_for_trait("core::convert::Into"), None,);
    }
}
