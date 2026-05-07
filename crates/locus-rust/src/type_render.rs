//! Clean type-text rendering.
//!
//! `quote::ToTokens::to_string()` separates every token with a single space,
//! producing strings like `"Result < User , Error >"` and
//! `"impl TryFrom < UserDto > for User"`. AIR consumers (and the lockfile, in
//! particular) read these strings; we don't want extra whitespace cemented.
//!
//! This module walks `syn` types directly and emits idiomatic Rust spelling:
//! `Result<User, Error>`, `&mut Foo`, `(T, U)`, `dyn Foo + Send`. For variants
//! we don't enumerate (rare cases like inline async traits, verbatim tokens),
//! we fall back to the spacy rendering with a targeted normalizer.

use quote::ToTokens;
use syn::{
    GenericArgument, Path, PathArguments, ReturnType, TraitBoundModifier, Type, TypeParamBound,
    TypePath,
};

/// Render a syn type as clean Rust source text.
pub fn render_type(ty: &Type) -> String {
    let mut s = String::new();
    render_type_into(&mut s, ty);
    s
}

/// Render a syn path (used for trait references in `impl Trait for Self`).
pub fn render_path(path: &Path) -> String {
    let mut s = String::new();
    render_path_into(&mut s, path);
    s
}

fn render_type_into(out: &mut String, ty: &Type) {
    match ty {
        Type::Path(tp) => render_type_path_into(out, tp),
        Type::Reference(r) => {
            out.push('&');
            if let Some(lt) = &r.lifetime {
                out.push('\'');
                out.push_str(&lt.ident.to_string());
                out.push(' ');
            }
            if r.mutability.is_some() {
                out.push_str("mut ");
            }
            render_type_into(out, &r.elem);
        }
        Type::Tuple(t) => {
            out.push('(');
            for (i, elem) in t.elems.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                render_type_into(out, elem);
            }
            // Trailing comma for single-element tuples to disambiguate from Paren.
            if t.elems.len() == 1 {
                out.push(',');
            }
            out.push(')');
        }
        Type::Array(a) => {
            out.push('[');
            render_type_into(out, &a.elem);
            out.push_str("; ");
            out.push_str(&normalize_spaces(a.len.to_token_stream().to_string()));
            out.push(']');
        }
        Type::Slice(s) => {
            out.push('[');
            render_type_into(out, &s.elem);
            out.push(']');
        }
        Type::Ptr(p) => {
            out.push('*');
            if p.const_token.is_some() {
                out.push_str("const ");
            } else if p.mutability.is_some() {
                out.push_str("mut ");
            }
            render_type_into(out, &p.elem);
        }
        Type::TraitObject(t) => {
            out.push_str("dyn ");
            render_bounds_into(out, &t.bounds);
        }
        Type::ImplTrait(i) => {
            out.push_str("impl ");
            render_bounds_into(out, &i.bounds);
        }
        Type::BareFn(f) => {
            out.push_str("fn(");
            for (i, arg) in f.inputs.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                render_type_into(out, &arg.ty);
            }
            out.push(')');
            if let ReturnType::Type(_, ret) = &f.output {
                out.push_str(" -> ");
                render_type_into(out, ret);
            }
        }
        Type::Paren(p) => {
            out.push('(');
            render_type_into(out, &p.elem);
            out.push(')');
        }
        Type::Group(g) => render_type_into(out, &g.elem),
        Type::Infer(_) => out.push('_'),
        Type::Never(_) => out.push('!'),
        other => out.push_str(&normalize_spaces(other.to_token_stream().to_string())),
    }
}

fn render_type_path_into(out: &mut String, tp: &TypePath) {
    if let Some(qself) = &tp.qself {
        out.push('<');
        render_type_into(out, &qself.ty);
        // qself.position is the count of leading path segments to render
        // before "as Trait>::". Rare in practice — we approximate by always
        // emitting "as <prefix-of-path>" when position > 0.
        if qself.position > 0 {
            out.push_str(" as ");
            let mut p = Path {
                leading_colon: tp.path.leading_colon,
                segments: tp
                    .path
                    .segments
                    .iter()
                    .take(qself.position)
                    .cloned()
                    .collect(),
            };
            // Re-render the trait portion.
            render_path_into(out, &p);
            // Drop those segments from the trailing render below.
            p.segments.clear();
        }
        out.push_str(">::");
        // Render remaining segments after the qualified portion.
        for (i, seg) in tp.path.segments.iter().enumerate().skip(qself.position) {
            if i > qself.position {
                out.push_str("::");
            }
            render_path_segment_into(out, seg);
        }
    } else {
        render_path_into(out, &tp.path);
    }
}

fn render_path_into(out: &mut String, path: &Path) {
    if path.leading_colon.is_some() {
        out.push_str("::");
    }
    for (i, seg) in path.segments.iter().enumerate() {
        if i > 0 {
            out.push_str("::");
        }
        render_path_segment_into(out, seg);
    }
}

fn render_path_segment_into(out: &mut String, seg: &syn::PathSegment) {
    out.push_str(&seg.ident.to_string());
    match &seg.arguments {
        PathArguments::None => {}
        PathArguments::AngleBracketed(ab) => {
            out.push('<');
            for (j, arg) in ab.args.iter().enumerate() {
                if j > 0 {
                    out.push_str(", ");
                }
                render_generic_arg_into(out, arg);
            }
            out.push('>');
        }
        PathArguments::Parenthesized(p) => {
            out.push('(');
            for (j, ty) in p.inputs.iter().enumerate() {
                if j > 0 {
                    out.push_str(", ");
                }
                render_type_into(out, ty);
            }
            out.push(')');
            if let ReturnType::Type(_, ret) = &p.output {
                out.push_str(" -> ");
                render_type_into(out, ret);
            }
        }
    }
}

fn render_generic_arg_into(out: &mut String, arg: &GenericArgument) {
    match arg {
        GenericArgument::Type(t) => render_type_into(out, t),
        GenericArgument::Lifetime(lt) => {
            out.push('\'');
            out.push_str(&lt.ident.to_string());
        }
        GenericArgument::Const(c) => {
            out.push_str(&normalize_spaces(c.to_token_stream().to_string()));
        }
        GenericArgument::AssocType(a) => {
            out.push_str(&a.ident.to_string());
            out.push_str(" = ");
            render_type_into(out, &a.ty);
        }
        other => out.push_str(&normalize_spaces(other.to_token_stream().to_string())),
    }
}

fn render_bounds_into(
    out: &mut String,
    bounds: &syn::punctuated::Punctuated<TypeParamBound, syn::Token![+]>,
) {
    for (i, b) in bounds.iter().enumerate() {
        if i > 0 {
            out.push_str(" + ");
        }
        match b {
            TypeParamBound::Trait(t) => {
                if matches!(t.modifier, TraitBoundModifier::Maybe(_)) {
                    out.push('?');
                }
                render_path_into(out, &t.path);
            }
            TypeParamBound::Lifetime(lt) => {
                out.push('\'');
                out.push_str(&lt.ident.to_string());
            }
            other => out.push_str(&normalize_spaces(other.to_token_stream().to_string())),
        }
    }
}

/// Targeted cleanup applied to the fallback `to_token_stream()` rendering for
/// type variants we don't walk explicitly. Order matters — collapse around
/// the common punctuators.
fn normalize_spaces(s: String) -> String {
    s.replace(" :: ", "::")
        .replace(":: ", "::")
        .replace(" ::", "::")
        .replace(" < ", "<")
        .replace("< ", "<")
        .replace(" <", "<")
        .replace(" > ", ">")
        .replace(" >", ">")
        .replace("> ", ">")
        .replace(" , ", ", ")
        .replace("& mut ", "&mut ")
        .replace("& '", "&'")
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_str;

    fn t(src: &str) -> String {
        let ty: Type = parse_str(src).expect("parses");
        render_type(&ty)
    }

    #[test]
    fn plain_path() {
        assert_eq!(t("User"), "User");
        assert_eq!(t("crate::domain::User"), "crate::domain::User");
    }

    #[test]
    fn generics_have_no_inner_spaces() {
        assert_eq!(t("Vec<UserDto>"), "Vec<UserDto>");
        assert_eq!(t("Result<User, Error>"), "Result<User, Error>");
        assert_eq!(
            t("HashMap<String, Vec<User>>"),
            "HashMap<String, Vec<User>>"
        );
    }

    #[test]
    fn references() {
        assert_eq!(t("&str"), "&str");
        assert_eq!(t("&mut User"), "&mut User");
        assert_eq!(t("&'a User"), "&'a User");
        assert_eq!(t("&'a mut User"), "&'a mut User");
    }

    #[test]
    fn tuples_and_arrays() {
        assert_eq!(t("(User, Error)"), "(User, Error)");
        assert_eq!(t("[u8; 32]"), "[u8; 32]");
        assert_eq!(t("[u8]"), "[u8]");
    }

    #[test]
    fn dyn_and_impl() {
        assert_eq!(t("dyn Foo"), "dyn Foo");
        assert_eq!(t("dyn Foo + Send"), "dyn Foo + Send");
        assert_eq!(t("impl Iterator<Item = u32>"), "impl Iterator<Item = u32>");
    }
}
