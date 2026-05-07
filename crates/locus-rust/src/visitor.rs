//! `syn::File` → `Vec<AirItem>`.
//!
//! Phase 1 emits direct extractions only. Inference (concept overlap,
//! boundary roles) lives in `locus-core`.

use crate::type_render::{render_path, render_type};
use locus_air::{
    ActionKind, AirConversion, AirField, AirFunction, AirImport, AirItem, AirSpan, AirTruthAction,
    AirType, AirVariant, ConversionMechanism, TypeKind, Visibility,
};
use quote::ToTokens;
use syn::{
    Expr, Fields, File, ImplItem, Item, ItemEnum, ItemFn, ItemImpl, ItemMod, ItemStruct, ItemType,
    ItemUnion, ItemUse, Pat, ReturnType, Stmt, UseTree, Visibility as SynVis, spanned::Spanned,
};

pub fn collect_items(file: &File, file_path: &str, module: Option<&str>) -> Vec<AirItem> {
    let mut out = Vec::new();
    let module_str = module.unwrap_or("crate").to_string();
    // crate-name prefix used to rewrite `crate::*` import paths into the
    // package-prefixed symbol form. Falls back to the literal "crate" when
    // the caller didn't supply a module — keeps tests that still pass `None`
    // working.
    let crate_name = module
        .and_then(|m| m.split("::").next())
        .unwrap_or("crate")
        .to_string();
    walk_items(&file.items, &module_str, &crate_name, file_path, &mut out);
    out
}

fn walk_items(
    items: &[Item],
    module: &str,
    crate_name: &str,
    file_path: &str,
    out: &mut Vec<AirItem>,
) {
    for item in items {
        match item {
            Item::Struct(s) => emit_struct(s, module, file_path, out),
            Item::Enum(e) => emit_enum(e, module, file_path, out),
            Item::Type(a) => emit_alias(a, module, file_path, out),
            Item::Union(u) => emit_union(u, module, file_path, out),
            Item::Fn(f) => emit_fn(f, module, file_path, out),
            Item::Impl(i) => emit_impl(i, module, file_path, out),
            Item::Mod(m) => emit_mod(m, module, crate_name, file_path, out),
            Item::Use(u) => emit_use(u, crate_name, file_path, out),
            _ => {}
        }
    }
}

fn emit_struct(s: &ItemStruct, module: &str, file_path: &str, out: &mut Vec<AirItem>) {
    let name = s.ident.to_string();
    let symbol = format!("{module}::{name}");
    let fields = collect_named_fields(&s.fields);
    let (derives, attrs) = split_attrs(&s.attrs);
    out.push(AirItem::Type(AirType {
        kind: TypeKind::Struct,
        name,
        symbol,
        visibility: vis_of(&s.vis),
        fields,
        variants: Vec::new(),
        derives,
        attrs,
        span: span_of(file_path, s.span()),
    }));
}

fn emit_enum(e: &ItemEnum, module: &str, file_path: &str, out: &mut Vec<AirItem>) {
    let name = e.ident.to_string();
    let symbol = format!("{module}::{name}");
    let (derives, attrs) = split_attrs(&e.attrs);
    let variants = e
        .variants
        .iter()
        .map(|v| AirVariant {
            name: v.ident.to_string(),
            fields: collect_named_fields(&v.fields),
        })
        .collect();
    out.push(AirItem::Type(AirType {
        kind: TypeKind::Enum,
        name,
        symbol,
        visibility: vis_of(&e.vis),
        fields: Vec::new(),
        variants,
        derives,
        attrs,
        span: span_of(file_path, e.span()),
    }));
}

fn emit_alias(a: &ItemType, module: &str, file_path: &str, out: &mut Vec<AirItem>) {
    let name = a.ident.to_string();
    let symbol = format!("{module}::{name}");
    let (derives, attrs) = split_attrs(&a.attrs);
    let alias_target = render_type(&a.ty);
    out.push(AirItem::Type(AirType {
        kind: TypeKind::Alias,
        name,
        symbol,
        visibility: vis_of(&a.vis),
        fields: vec![AirField {
            name: "<aliased>".to_string(),
            type_text: alias_target,
            visibility: Visibility::Public,
        }],
        variants: Vec::new(),
        derives,
        attrs,
        span: span_of(file_path, a.span()),
    }));
}

fn emit_union(u: &ItemUnion, module: &str, file_path: &str, out: &mut Vec<AirItem>) {
    let name = u.ident.to_string();
    let symbol = format!("{module}::{name}");
    let (derives, attrs) = split_attrs(&u.attrs);
    let fields = u
        .fields
        .named
        .iter()
        .map(|f| AirField {
            name: f.ident.as_ref().map(|i| i.to_string()).unwrap_or_default(),
            type_text: render_type(&f.ty),
            visibility: vis_of(&f.vis),
        })
        .collect();
    out.push(AirItem::Type(AirType {
        kind: TypeKind::Union,
        name,
        symbol,
        visibility: vis_of(&u.vis),
        fields,
        variants: Vec::new(),
        derives,
        attrs,
        span: span_of(file_path, u.span()),
    }));
}

fn emit_fn(f: &ItemFn, module: &str, file_path: &str, out: &mut Vec<AirItem>) {
    let name = f.sig.ident.to_string();
    let symbol = format!("{module}::{name}");
    let params = f
        .sig
        .inputs
        .iter()
        .map(|arg| match arg {
            syn::FnArg::Receiver(_) => ("self".to_string(), "Self".to_string()),
            syn::FnArg::Typed(pt) => {
                let pname = match &*pt.pat {
                    Pat::Ident(pi) => pi.ident.to_string(),
                    other => other.to_token_stream().to_string(),
                };
                (pname, render_type(&pt.ty))
            }
        })
        .collect::<Vec<_>>();

    let return_type = match &f.sig.output {
        ReturnType::Default => None,
        ReturnType::Type(_, ty) => Some(render_type(ty)),
    };

    out.push(AirItem::Function(AirFunction {
        name: name.clone(),
        symbol: symbol.clone(),
        visibility: vis_of(&f.vis),
        params: params.clone(),
        return_type: return_type.clone(),
        span: span_of(file_path, f.span()),
    }));

    // Free-function converter signal: single arg, returns something concept-shaped.
    if let Some(mech) = free_fn_converter_kind(&name)
        && params.len() == 1
        && let Some(ret) = &return_type
    {
        let from_ty = strip_refs(&params[0].1);
        let to_ty = strip_result_or_option(ret).unwrap_or_else(|| ret.clone());
        if !from_ty.is_empty() && !to_ty.is_empty() && from_ty != to_ty {
            out.push(AirItem::Conversion(AirConversion {
                from: from_ty,
                to: to_ty,
                mechanism: mech,
                symbol: symbol.clone(),
                span: span_of(file_path, f.span()),
            }));
        }
    }

    scan_fn_body_for_truth_actions(&f.block, &symbol, file_path, out);
}

fn emit_impl(i: &ItemImpl, module: &str, file_path: &str, out: &mut Vec<AirItem>) {
    let self_ty = render_type(&i.self_ty);

    if let Some((_, trait_path, _)) = &i.trait_ {
        let last = trait_path
            .segments
            .last()
            .map(|s| s.ident.to_string())
            .unwrap_or_default();
        if last == "From" || last == "TryFrom" {
            // Pull <T> out of the trait's last segment generics.
            if let Some(seg) = trait_path.segments.last()
                && let syn::PathArguments::AngleBracketed(ab) = &seg.arguments
                && let Some(syn::GenericArgument::Type(t)) = ab.args.first()
            {
                let from_ty = render_type(t);
                let mech = if last == "TryFrom" {
                    ConversionMechanism::TryFrom
                } else {
                    ConversionMechanism::From
                };
                out.push(AirItem::Conversion(AirConversion {
                    from: from_ty,
                    to: self_ty.clone(),
                    mechanism: mech,
                    symbol: format!("{module}::impl {} for {}", render_path(trait_path), self_ty),
                    span: span_of(file_path, i.span()),
                }));
            }
        }
    } else {
        // Inherent impl: detect `to_*` / `into_*` methods returning a different type.
        for item in &i.items {
            if let ImplItem::Fn(m) = item
                && let Some(mech) = inherent_method_converter_kind(&m.sig.ident.to_string())
                && let ReturnType::Type(_, ty) = &m.sig.output
            {
                let return_text = render_type(ty);
                let to_ty = strip_result_or_option(&return_text).unwrap_or(return_text);
                let from_ty = strip_refs(&self_ty);
                if !to_ty.is_empty() && from_ty != to_ty {
                    out.push(AirItem::Conversion(AirConversion {
                        from: from_ty,
                        to: to_ty,
                        mechanism: mech,
                        symbol: format!("{module}::{}::{}", self_ty, m.sig.ident),
                        span: span_of(file_path, m.span()),
                    }));
                }
            }
        }
    }
}

fn emit_mod(
    m: &ItemMod,
    parent_module: &str,
    crate_name: &str,
    file_path: &str,
    out: &mut Vec<AirItem>,
) {
    let Some((_, items)) = &m.content else {
        return; // out-of-line module; its file is walked separately.
    };
    let nested = format!("{parent_module}::{}", m.ident);
    walk_items(items, &nested, crate_name, file_path, out);
}

fn emit_use(u: &ItemUse, crate_name: &str, file_path: &str, out: &mut Vec<AirItem>) {
    let mut paths = Vec::new();
    flatten_use_tree(&u.tree, String::new(), &mut paths);
    let visibility = vis_of(&u.vis);
    let span = span_of(file_path, u.span());
    for raw in paths {
        let path = normalize_use_path(raw, crate_name);
        out.push(AirItem::Import(AirImport {
            path,
            visibility,
            span: span.clone(),
        }));
    }
}

fn flatten_use_tree(tree: &UseTree, prefix: String, out: &mut Vec<String>) {
    let join = |p: &str, seg: &str| -> String {
        if p.is_empty() {
            seg.to_string()
        } else {
            format!("{p}::{seg}")
        }
    };
    match tree {
        UseTree::Path(p) => {
            let next = join(&prefix, &p.ident.to_string());
            flatten_use_tree(&p.tree, next, out);
        }
        UseTree::Name(n) => out.push(join(&prefix, &n.ident.to_string())),
        UseTree::Rename(r) => out.push(join(&prefix, &r.ident.to_string())),
        UseTree::Glob(_) => out.push(if prefix.is_empty() {
            "*".to_string()
        } else {
            format!("{prefix}::*")
        }),
        UseTree::Group(g) => {
            for inner in &g.items {
                flatten_use_tree(inner, prefix.clone(), out);
            }
        }
    }
}

/// Rewrite leading `crate::` to the package's lib name so the AIR import
/// path lines up with [`AirType::symbol`] (also package-prefixed). `self::`
/// and `super::` paths are left literal — accurate resolution of those would
/// require knowing the file's full module chain, which is a Phase 3 problem.
fn normalize_use_path(path: String, crate_name: &str) -> String {
    if path == "crate" {
        return crate_name.to_string();
    }
    if let Some(rest) = path.strip_prefix("crate::") {
        return format!("{crate_name}::{rest}");
    }
    path
}

fn collect_named_fields(fields: &Fields) -> Vec<AirField> {
    match fields {
        Fields::Named(n) => n
            .named
            .iter()
            .map(|f| AirField {
                name: f.ident.as_ref().map(|i| i.to_string()).unwrap_or_default(),
                type_text: render_type(&f.ty),
                visibility: vis_of(&f.vis),
            })
            .collect(),
        Fields::Unnamed(u) => u
            .unnamed
            .iter()
            .enumerate()
            .map(|(idx, f)| AirField {
                name: idx.to_string(),
                type_text: render_type(&f.ty),
                visibility: vis_of(&f.vis),
            })
            .collect(),
        Fields::Unit => Vec::new(),
    }
}

fn vis_of(v: &SynVis) -> Visibility {
    match v {
        SynVis::Public(_) => Visibility::Public,
        SynVis::Restricted(r) => {
            if r.path.is_ident("crate") {
                Visibility::Crate
            } else {
                Visibility::Restricted
            }
        }
        SynVis::Inherited => Visibility::Private,
    }
}

fn split_attrs(attrs: &[syn::Attribute]) -> (Vec<String>, Vec<String>) {
    let mut derives = Vec::new();
    let mut others = Vec::new();
    for a in attrs {
        if a.path().is_ident("derive") {
            let _ = a.parse_nested_meta(|meta| {
                derives.push(meta.path.to_token_stream().to_string());
                Ok(())
            });
        } else {
            others.push(a.to_token_stream().to_string());
        }
    }
    (derives, others)
}

fn span_of(file: &str, sp: proc_macro2::Span) -> AirSpan {
    let start = sp.start();
    let end = sp.end();
    AirSpan::new(file, start.line as u32, end.line as u32)
}

fn free_fn_converter_kind(name: &str) -> Option<ConversionMechanism> {
    if name.starts_with("to_")
        || name.starts_with("from_")
        || name.starts_with("into_")
        || name.starts_with("map_")
        || name.starts_with("convert_")
    {
        Some(ConversionMechanism::FreeFn)
    } else {
        None
    }
}

fn inherent_method_converter_kind(name: &str) -> Option<ConversionMechanism> {
    if name.starts_with("to_") || name.starts_with("into_") {
        Some(ConversionMechanism::InherentMethod)
    } else {
        None
    }
}

fn strip_refs(ty: &str) -> String {
    let trimmed = ty.trim();
    trimmed
        .trim_start_matches('&')
        .trim_start_matches("mut ")
        .trim()
        .to_string()
}

/// Pull `T` out of `Result<T, E>` / `Option<T>`. Operates on the clean
/// type-text produced by [`render_type`].
fn strip_result_or_option(ty: &str) -> Option<String> {
    let t = ty.trim();
    let inner = t
        .strip_prefix("Result<")
        .or_else(|| t.strip_prefix("Option<"))?;
    let close = inner.rfind('>')?;
    let inside = &inner[..close];
    let first = inside.split(',').next()?.trim();
    Some(first.to_string())
}

fn scan_fn_body_for_truth_actions(
    block: &syn::Block,
    function: &str,
    file_path: &str,
    out: &mut Vec<AirItem>,
) {
    for stmt in &block.stmts {
        scan_stmt(stmt, function, file_path, out);
    }
}

fn scan_stmt(stmt: &Stmt, function: &str, file_path: &str, out: &mut Vec<AirItem>) {
    match stmt {
        Stmt::Local(l) => {
            if let Some(init) = &l.init {
                scan_expr(&init.expr, function, file_path, out);
            }
        }
        Stmt::Expr(e, _) => scan_expr(e, function, file_path, out),
        Stmt::Item(_) | Stmt::Macro(_) => {}
    }
}

fn scan_expr(expr: &Expr, function: &str, file_path: &str, out: &mut Vec<AirItem>) {
    match expr {
        Expr::Struct(s) => {
            let target = s.path.to_token_stream().to_string();
            out.push(AirItem::TruthAction(AirTruthAction {
                action: ActionKind::Construct,
                target,
                function: Some(function.to_string()),
                span: span_of(file_path, s.span()),
                confidence: 0.95,
                reasons: vec!["struct literal in function body".into()],
            }));
            for f in &s.fields {
                scan_expr(&f.expr, function, file_path, out);
            }
        }
        Expr::Match(m) => {
            let target = m.expr.to_token_stream().to_string();
            out.push(AirItem::TruthAction(AirTruthAction {
                action: ActionKind::EnumMatch,
                target,
                function: Some(function.to_string()),
                span: span_of(file_path, m.span()),
                confidence: 0.6,
                reasons: vec!["match expression".into()],
            }));
            scan_expr(&m.expr, function, file_path, out);
            for arm in &m.arms {
                scan_expr(&arm.body, function, file_path, out);
            }
        }
        Expr::Binary(b) if matches!(b.op, syn::BinOp::Eq(_) | syn::BinOp::Ne(_)) => {
            if is_string_compare(&b.left, &b.right) || is_string_compare(&b.right, &b.left) {
                let target = b.left.to_token_stream().to_string();
                out.push(AirItem::TruthAction(AirTruthAction {
                    action: ActionKind::StringCompare,
                    target,
                    function: Some(function.to_string()),
                    span: span_of(file_path, b.span()),
                    confidence: 0.7,
                    reasons: vec!["string-literal equality on field expression".into()],
                }));
            }
            scan_expr(&b.left, function, file_path, out);
            scan_expr(&b.right, function, file_path, out);
        }
        Expr::Block(b) => {
            for stmt in &b.block.stmts {
                scan_stmt(stmt, function, file_path, out);
            }
        }
        Expr::If(i) => {
            scan_expr(&i.cond, function, file_path, out);
            for stmt in &i.then_branch.stmts {
                scan_stmt(stmt, function, file_path, out);
            }
            if let Some((_, else_)) = &i.else_branch {
                scan_expr(else_, function, file_path, out);
            }
        }
        Expr::Call(c) => {
            scan_expr(&c.func, function, file_path, out);
            for a in &c.args {
                scan_expr(a, function, file_path, out);
            }
        }
        Expr::MethodCall(m) => {
            scan_expr(&m.receiver, function, file_path, out);
            for a in &m.args {
                scan_expr(a, function, file_path, out);
            }
        }
        Expr::Return(r) => {
            if let Some(inner) = &r.expr {
                scan_expr(inner, function, file_path, out);
            }
        }
        Expr::Reference(r) => scan_expr(&r.expr, function, file_path, out),
        Expr::Paren(p) => scan_expr(&p.expr, function, file_path, out),
        Expr::Tuple(t) => {
            for e in &t.elems {
                scan_expr(e, function, file_path, out);
            }
        }
        Expr::Try(t) => scan_expr(&t.expr, function, file_path, out),
        Expr::Unary(u) => scan_expr(&u.expr, function, file_path, out),
        _ => {}
    }
}

fn is_string_compare(side: &Expr, other: &Expr) -> bool {
    let Expr::Lit(l) = other else { return false };
    if !matches!(l.lit, syn::Lit::Str(_)) {
        return false;
    }
    matches!(side, Expr::Field(_) | Expr::Path(_) | Expr::MethodCall(_))
}
