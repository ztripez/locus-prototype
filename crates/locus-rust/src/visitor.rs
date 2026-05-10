//! `syn::File` → `Vec<AirItem>`.
//!
//! Phase 1 emits direct extractions only. Inference (concept overlap,
//! boundary roles) lives in `locus-core`.

use crate::type_render::{render_path, render_type};
use locus_air::{
    ActionKind, AirCallSite, AirClosureMethodCall, AirConversion, AirDecorator, AirFallbackCall,
    AirField, AirFunction, AirImplBlock, AirImport, AirItem, AirMatchArm, AirPartialResultMatch,
    AirRetryLoop, AirScrutineeLiteral, AirSilentDiscard, AirSpan, AirTruthAction, AirType,
    AirVariant, ArmBodyShape, CallKind, ConversionMechanism, DecoratorSource, DiscardKind,
    FallbackPattern, ImplDispatch, LiteralContext, LiteralKind, LoopKind, ResultMatchVariant,
    TypeKind, Visibility,
};

/// Split a Rust-style `::`-joined symbol into segments for AIR's
/// language-agnostic `*_segments` fields. Empty input → empty Vec.
fn segments_of(symbol: &str) -> Vec<String> {
    if symbol.is_empty() {
        return Vec::new();
    }
    symbol.split("::").map(|s| s.to_string()).collect()
}
use quote::ToTokens;
use syn::{
    Expr, ExprLit, Fields, File, ImplItem, Item, ItemEnum, ItemFn, ItemImpl, ItemMod, ItemStruct,
    ItemTrait, ItemType, ItemUnion, ItemUse, Lit, Meta, Pat, ReturnType, Stmt, UseTree,
    Visibility as SynVis, spanned::Spanned,
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
            Item::Trait(t) => emit_trait(t, module, file_path, out),
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
    let symbol_segments = segments_of(&symbol);
    let fields = collect_named_fields(&s.fields);
    let (decorators, doc) = split_attrs(&s.attrs);
    out.push(AirItem::Type(AirType {
        kind: TypeKind::Struct,
        name,
        symbol,
        visibility: vis_of(&s.vis),
        fields,
        variants: Vec::new(),
        decorators,
        symbol_segments,
        span: span_of(file_path, s.span()),
        doc,
    }));
}

fn emit_enum(e: &ItemEnum, module: &str, file_path: &str, out: &mut Vec<AirItem>) {
    let name = e.ident.to_string();
    let symbol = format!("{module}::{name}");
    let symbol_segments = segments_of(&symbol);
    let (decorators, doc) = split_attrs(&e.attrs);
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
        decorators,
        symbol_segments,
        span: span_of(file_path, e.span()),
        doc,
    }));
}

fn emit_alias(a: &ItemType, module: &str, file_path: &str, out: &mut Vec<AirItem>) {
    let name = a.ident.to_string();
    let symbol = format!("{module}::{name}");
    let symbol_segments = segments_of(&symbol);
    let (decorators, doc) = split_attrs(&a.attrs);
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
        decorators,
        symbol_segments,
        span: span_of(file_path, a.span()),
        doc,
    }));
}

fn emit_union(u: &ItemUnion, module: &str, file_path: &str, out: &mut Vec<AirItem>) {
    let name = u.ident.to_string();
    let symbol = format!("{module}::{name}");
    let symbol_segments = segments_of(&symbol);
    let (decorators, doc) = split_attrs(&u.attrs);
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
        decorators,
        symbol_segments,
        span: span_of(file_path, u.span()),
        doc,
    }));
}

fn emit_trait(t: &ItemTrait, module: &str, file_path: &str, out: &mut Vec<AirItem>) {
    let name = t.ident.to_string();
    let symbol = format!("{module}::{name}");
    let symbol_segments = segments_of(&symbol);
    let (decorators, doc) = split_attrs(&t.attrs);
    out.push(AirItem::Type(AirType {
        kind: TypeKind::Trait,
        name,
        symbol,
        visibility: vis_of(&t.vis),
        fields: Vec::new(),
        variants: Vec::new(),
        decorators,
        symbol_segments,
        span: span_of(file_path, t.span()),
        doc,
    }));
}

fn emit_fn(f: &ItemFn, module: &str, file_path: &str, out: &mut Vec<AirItem>) {
    let name = f.sig.ident.to_string();
    let symbol = format!("{module}::{name}");
    let (params, return_type) = emit_fn_air_function(f, &symbol, file_path, out);
    emit_fn_converter(f, &name, &symbol, &params, &return_type, file_path, out);
    scan_fn_body_for_truth_actions(&f.block, &symbol, file_path, out);
}

/// Collect function signature facts and push an `AirFunction` item.
/// Returns `(params, return_type)` so the caller can reuse them for
/// converter detection without re-parsing the signature.
fn emit_fn_air_function(
    f: &ItemFn,
    symbol: &str,
    file_path: &str,
    out: &mut Vec<AirItem>,
) -> (Vec<(String, String)>, Option<String>) {
    let symbol_segments = segments_of(symbol);
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
    let span = span_of(file_path, f.span());
    let line_count = span.line_end.saturating_sub(span.line_start) + 1;
    let (decorators, doc) = split_attrs(&f.attrs);
    out.push(AirItem::Function(AirFunction {
        name: f.sig.ident.to_string(),
        symbol: symbol.to_string(),
        symbol_segments,
        visibility: vis_of(&f.vis),
        params: params.clone(),
        return_type: return_type.clone(),
        span,
        line_count,
        decorators,
        doc,
    }));
    (params, return_type)
}

/// Free-function converter signal: single arg, returns something concept-shaped.
/// Pushes an `AirConversion` when the name matches a converter prefix.
fn emit_fn_converter(
    f: &ItemFn,
    name: &str,
    symbol: &str,
    params: &[(String, String)],
    return_type: &Option<String>,
    file_path: &str,
    out: &mut Vec<AirItem>,
) {
    if let Some(mech) = free_fn_converter_kind(name)
        && params.len() == 1
        && let Some(ret) = return_type
    {
        let from_ty = strip_refs(&params[0].1);
        let to_ty = strip_result_or_option(ret).unwrap_or_else(|| ret.clone());
        if !from_ty.is_empty() && !to_ty.is_empty() && from_ty != to_ty {
            out.push(AirItem::Conversion(AirConversion {
                from: from_ty,
                to: to_ty,
                mechanism: mech,
                symbol: symbol.to_string(),
                span: span_of(file_path, f.span()),
            }));
        }
    }
}

fn emit_impl(i: &ItemImpl, module: &str, file_path: &str, out: &mut Vec<AirItem>) {
    let self_ty = render_type(&i.self_ty);
    emit_impl_impl_block(i, &self_ty, file_path, out);
    if let Some((_, trait_path, _)) = &i.trait_ {
        emit_impl_trait_conversion(i, trait_path, &self_ty, module, file_path, out);
    } else {
        emit_impl_inherent_conversions(i, &self_ty, module, file_path, out);
    }
}

/// Push an `AirImplBlock` summary for every impl block. Conversion-shaped
/// impls additionally produce `AirConversion` items (via the other handlers);
/// that overlap is intentional — different paradigms (PA, AB) consume the
/// impl summary, while OT consumes the conversion view.
fn emit_impl_impl_block(i: &ItemImpl, self_ty: &str, file_path: &str, out: &mut Vec<AirItem>) {
    let trait_path_text = i
        .trait_
        .as_ref()
        .map(|(_, trait_path, _)| render_path(trait_path));
    let method_names = i
        .items
        .iter()
        .filter_map(|item| match item {
            ImplItem::Fn(m) => Some(m.sig.ident.to_string()),
            _ => None,
        })
        .collect();
    // Rust adapter emits Static for explicit `impl Trait for Type`.
    // `impl dyn Trait` would be Dynamic, but those don't appear at
    // item-level — they're type-position only. Future Go adapter
    // will emit Structural for implicit interface satisfaction.
    out.push(AirItem::Impl(AirImplBlock {
        interface: trait_path_text,
        target_type: self_ty.to_string(),
        method_names,
        dispatch: ImplDispatch::Static,
        span: span_of(file_path, i.span()),
    }));
}

/// For `impl From<T> for Self` / `impl TryFrom<T> for Self`, emit an
/// `AirConversion` so the OT paradigm can track the conversion path.
fn emit_impl_trait_conversion(
    i: &ItemImpl,
    trait_path: &syn::Path,
    self_ty: &str,
    module: &str,
    file_path: &str,
    out: &mut Vec<AirItem>,
) {
    let last = trait_path
        .segments
        .last()
        .map(|s| s.ident.to_string())
        .unwrap_or_default();
    if last != "From" && last != "TryFrom" {
        return;
    }
    // Pull <T> out of the trait's last segment generics.
    if let Some(seg) = trait_path.segments.last()
        && let syn::PathArguments::AngleBracketed(ab) = &seg.arguments
        && let Some(syn::GenericArgument::Type(t)) = ab.args.first()
    {
        let from_ty = render_type(t);
        let mech = if last == "TryFrom" {
            ConversionMechanism::FallibleAdapter
        } else {
            ConversionMechanism::InfallibleAdapter
        };
        out.push(AirItem::Conversion(AirConversion {
            from: from_ty,
            to: self_ty.to_string(),
            mechanism: mech,
            symbol: format!("{module}::impl {} for {}", render_path(trait_path), self_ty),
            span: span_of(file_path, i.span()),
        }));
    }
}

/// Inherent impl: detect `to_*` / `into_*` methods returning a different type
/// and emit an `AirConversion` for each one found.
fn emit_impl_inherent_conversions(
    i: &ItemImpl,
    self_ty: &str,
    module: &str,
    file_path: &str,
    out: &mut Vec<AirItem>,
) {
    for item in &i.items {
        if let ImplItem::Fn(m) = item
            && let Some(mech) = inherent_method_converter_kind(&m.sig.ident.to_string())
            && let ReturnType::Type(_, ty) = &m.sig.output
        {
            let return_text = render_type(ty);
            let to_ty = strip_result_or_option(&return_text).unwrap_or(return_text);
            let from_ty = strip_refs(self_ty);
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
        let path_segments = segments_of(&path);
        out.push(AirItem::Import(AirImport {
            path,
            path_segments,
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
                Visibility::Module
            } else {
                Visibility::Restricted
            }
        }
        SynVis::Inherited => Visibility::Private,
    }
}

/// Returns `(decorators, doc)`. AIR v13 unified Rust `#[derive(...)]`
/// and `#[attr(...)]` forms into a single `Vec<AirDecorator>` with a
/// `source` tag (`Derive` vs `Attribute`). Doc attrs (`///` and
/// `#[doc = "..."]`) are still extracted separately into `doc`.
fn split_attrs(attrs: &[syn::Attribute]) -> (Vec<AirDecorator>, Option<String>) {
    let mut decorators: Vec<AirDecorator> = Vec::new();
    let mut doc_lines: Vec<String> = Vec::new();
    for a in attrs {
        if a.path().is_ident("derive") {
            let _ = a.parse_nested_meta(|meta| {
                decorators.push(AirDecorator {
                    source: DecoratorSource::Derive,
                    name: meta.path.to_token_stream().to_string(),
                    args: Vec::new(),
                });
                Ok(())
            });
        } else if a.path().is_ident("doc") {
            if let Meta::NameValue(nv) = &a.meta
                && let Expr::Lit(ExprLit {
                    lit: Lit::Str(s), ..
                }) = &nv.value
            {
                let raw = s.value();
                // Rustdoc convention: a single leading space is added by `///`
                // and should be stripped so the doc text matches the source.
                doc_lines.push(raw.strip_prefix(' ').unwrap_or(&raw).to_string());
            }
        } else {
            // Non-doc, non-derive attribute — `#[serde(rename = "x")]`,
            // `#[inline]`, `#[allow(...)]`. Render the whole attribute
            // text as the decorator name; v13 keeps args empty (the
            // text already includes the parenthesised arg block); a
            // future refinement could parse args out separately.
            decorators.push(AirDecorator {
                source: DecoratorSource::Attribute,
                name: a.to_token_stream().to_string(),
                args: Vec::new(),
            });
        }
    }
    let doc = if doc_lines.is_empty() {
        None
    } else {
        Some(doc_lines.join("\n"))
    };
    (decorators, doc)
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
        Some(ConversionMechanism::FreeFunction)
    } else {
        None
    }
}

fn inherent_method_converter_kind(name: &str) -> Option<ConversionMechanism> {
    if name.starts_with("to_") || name.starts_with("into_") {
        Some(ConversionMechanism::InstanceMethod)
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
            // `let _ = expr;` — silent-discard binding. Captured here so
            // the FL paradigm can flag `let _ = result;` as a swallowed
            // failure; FL004 decides which discards are legitimate.
            if matches!(l.pat, Pat::Wild(_))
                && let Some(init) = &l.init
            {
                let (callee, kind) = classify_discard_init(&init.expr);
                out.push(AirItem::SilentDiscard(AirSilentDiscard {
                    callee,
                    kind,
                    function: Some(function.to_string()),
                    span: span_of(file_path, l.span()),
                }));
            }
            if let Some(init) = &l.init {
                scan_expr(&init.expr, function, file_path, out);
            }
        }
        Stmt::Expr(e, _) => scan_expr(e, function, file_path, out),
        Stmt::Macro(m) => {
            // `println!`, `dbg!`, etc. at statement position. Same shape as
            // Expr::Macro: framework-neutral CallSite. Loaders translate the
            // callee path into normalized facts (e.g. LogsRaw / LogsStructured).
            out.push(AirItem::CallSite(AirCallSite {
                callee: render_path(&m.mac.path),
                kind: CallKind::Meta,
                function: Some(function.to_string()),
                span: span_of(file_path, m.mac.span()),
            }));
        }
        Stmt::Item(_) => {}
    }
}

/// Classify the right-hand side of a `let _ = <expr>;` for FL004.
///
/// Only call-shaped expressions get a meaningful callee — other shapes
/// (`let _ = some_field;`, `let _ = literal;`, blocks, etc.) are recorded
/// as `DiscardKind::Other` with no callee. FL004 ignores `Other` by
/// default because the false-positive surface for general expression
/// discards is too large to be useful.
fn classify_discard_init(expr: &Expr) -> (Option<String>, DiscardKind) {
    match expr {
        Expr::MethodCall(m) => (Some(m.method.to_string()), DiscardKind::Method),
        Expr::Call(c) => match &*c.func {
            Expr::Path(p) => (Some(render_path(&p.path)), DiscardKind::Function),
            _ => (None, DiscardKind::Function),
        },
        Expr::Macro(m) => (Some(render_path(&m.mac.path)), DiscardKind::Meta),
        // Peel through transparent wrappers so `let _ = (x.send());` and
        // `let _ = &mut x.lock();` still classify as the underlying call.
        Expr::Paren(p) => classify_discard_init(&p.expr),
        Expr::Reference(r) => classify_discard_init(&r.expr),
        Expr::Try(t) => classify_discard_init(&t.expr),
        _ => (None, DiscardKind::Other),
    }
}

fn scan_expr(expr: &Expr, function: &str, file_path: &str, out: &mut Vec<AirItem>) {
    match expr {
        Expr::Struct(s) => scan_expr_struct(s, function, file_path, out),
        Expr::Match(m) => scan_expr_match(m, function, file_path, out),
        Expr::Binary(b) if matches!(b.op, syn::BinOp::Eq(_) | syn::BinOp::Ne(_)) => {
            scan_expr_binary_eq(b, function, file_path, out);
        }
        Expr::Block(b) => scan_expr_block(b, function, file_path, out),
        Expr::If(i) => scan_expr_if(i, function, file_path, out),
        Expr::Call(c) => scan_expr_call(c, function, file_path, out),
        Expr::Macro(m) => scan_expr_macro(m, function, file_path, out),
        Expr::MethodCall(m) => scan_expr_method_call(m, function, file_path, out),
        Expr::Return(r) => {
            if let Some(inner) = &r.expr {
                scan_expr(inner, function, file_path, out);
            }
        }
        Expr::Reference(r) => scan_expr(&r.expr, function, file_path, out),
        Expr::Paren(p) => scan_expr(&p.expr, function, file_path, out),
        Expr::Tuple(t) => scan_expr_tuple(t, function, file_path, out),
        Expr::Try(t) => scan_expr(&t.expr, function, file_path, out),
        Expr::Unary(u) => scan_expr(&u.expr, function, file_path, out),
        // Loop constructs — emit `RetryLoop` if the body looks like a
        // retry shape (uses `?` and has a `break`). FL012 consumes
        // these. We always emit the item with the propagates / has_break
        // flags so the rule can decide.
        Expr::Loop(l) => scan_expr_loop(l, function, file_path, out),
        Expr::ForLoop(f) => scan_expr_for_loop(f, function, file_path, out),
        Expr::While(w) => scan_expr_while(w, function, file_path, out),
        _ => {}
    }
}

fn scan_expr_struct(s: &syn::ExprStruct, function: &str, file_path: &str, out: &mut Vec<AirItem>) {
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

fn scan_expr_match(m: &syn::ExprMatch, function: &str, file_path: &str, out: &mut Vec<AirItem>) {
    let scrutinee = m.expr.to_token_stream().to_string();
    out.push(AirItem::TruthAction(AirTruthAction {
        action: ActionKind::DiscriminatedMatch,
        target: scrutinee.clone(),
        function: Some(function.to_string()),
        span: span_of(file_path, m.span()),
        confidence: 0.6,
        reasons: vec!["match expression".into()],
    }));
    // Per-arm shape capture so paradigm rules (FL007 catch-all
    // `Err(_)`, FL011 default-variant sinks, ER005 catch-all error
    // mapping) can reason about silence at the arm level.
    for arm in &m.arms {
        let pattern = arm.pat.to_token_stream().to_string();
        let pattern_has_wildcard = pat_has_wildcard(&arm.pat);
        let body_shape = classify_body(&arm.body);
        out.push(AirItem::MatchArm(AirMatchArm {
            scrutinee: scrutinee.clone(),
            pattern,
            pattern_has_wildcard,
            body_shape,
            function: Some(function.to_string()),
            span: span_of(file_path, arm.span()),
        }));
        // ScrutineeLiteral: a literal-pattern arm (CF002 /
        // CF003 territory). Only fires for patterns that are
        // bare literals — `Pat::Lit("active")`, `Pat::Lit(42)`.
        // Tuple/struct patterns containing literals don't emit
        // here (they'd need recursive walking; defer).
        if let Some((value, kind)) = literal_value_of_pat(&arm.pat) {
            out.push(AirItem::ScrutineeLiteral(AirScrutineeLiteral {
                value,
                kind,
                context: LiteralContext::MatchArm,
                function: Some(function.to_string()),
                span: span_of(file_path, arm.span()),
            }));
        }
    }
    scan_expr(&m.expr, function, file_path, out);
    for arm in &m.arms {
        scan_expr(&arm.body, function, file_path, out);
    }
}

fn scan_expr_binary_eq(
    b: &syn::ExprBinary,
    function: &str,
    file_path: &str,
    out: &mut Vec<AirItem>,
) {
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
    // ScrutineeLiteral on binary `==`/`!=`: capture the literal
    // side when the *other* side isn't a literal. This is the
    // CF002 / CF003 detection surface — `if role == "admin"`
    // and similar magic-constant comparisons.
    for (lit_side, other_side) in [(&b.left, &b.right), (&b.right, &b.left)] {
        if let Some((value, kind)) = literal_value_of_expr(lit_side)
            && !is_literal_expr(other_side)
        {
            out.push(AirItem::ScrutineeLiteral(AirScrutineeLiteral {
                value,
                kind,
                context: LiteralContext::BinaryCompare,
                function: Some(function.to_string()),
                span: span_of(file_path, b.span()),
            }));
            break;
        }
    }
    scan_expr(&b.left, function, file_path, out);
    scan_expr(&b.right, function, file_path, out);
}

fn scan_expr_block(b: &syn::ExprBlock, function: &str, file_path: &str, out: &mut Vec<AirItem>) {
    for stmt in &b.block.stmts {
        scan_stmt(stmt, function, file_path, out);
    }
}

fn scan_expr_if(i: &syn::ExprIf, function: &str, file_path: &str, out: &mut Vec<AirItem>) {
    // `if let Ok(...) = expr { ... }` or `if let Err(...) = expr
    // { ... }` *without* an `else` branch. The unmatched arm is
    // silent — the failure (or success) just falls through. FL005
    // consumes this signal.
    if i.else_branch.is_none()
        && let Expr::Let(let_expr) = &*i.cond
        && let Some(variant) = result_variant_of_pat(&let_expr.pat)
    {
        // AIR v13: `variant` is now a `ResultMatchVariant` enum
        // rather than a `String` "Ok"|"Err". Map the Rust-side
        // `Ok`/`Err` to the architectural `Success`/`Failure`.
        let variant_enum = match variant {
            "Ok" => ResultMatchVariant::Success,
            "Err" => ResultMatchVariant::Failure,
            // result_variant_of_pat only returns these two
            // strings; this arm is unreachable.
            _ => unreachable!("result_variant_of_pat returned {variant:?}"),
        };
        out.push(AirItem::PartialResultMatch(AirPartialResultMatch {
            variant: variant_enum,
            function: Some(function.to_string()),
            span: span_of(file_path, i.span()),
        }));
    }
    scan_expr(&i.cond, function, file_path, out);
    for stmt in &i.then_branch.stmts {
        scan_stmt(stmt, function, file_path, out);
    }
    if let Some((_, else_)) = &i.else_branch {
        scan_expr(else_, function, file_path, out);
    }
}

fn scan_expr_call(c: &syn::ExprCall, function: &str, file_path: &str, out: &mut Vec<AirItem>) {
    // Framework-neutral CallSite: just the callee's path text and a
    // CallKind tag. Loaders translate this into normalized facts
    // (SpawnsWork / ReadsEnv / NetworkCall / ...) — the visitor stays
    // out of framework-specific reasoning.
    //
    // Path-shaped callees (`foo::bar(x)`) emit a CallSite; other
    // callee shapes (e.g. an expression returning a fn) don't yet —
    // their callee text isn't a useful path for a loader to match
    // against. We still recurse into args so nested calls are seen.
    if let Expr::Path(p) = &*c.func {
        out.push(AirItem::CallSite(AirCallSite {
            callee: render_path(&p.path),
            kind: CallKind::Function,
            function: Some(function.to_string()),
            span: span_of(file_path, c.span()),
        }));
    }
    scan_expr(&c.func, function, file_path, out);
    for a in &c.args {
        scan_expr(a, function, file_path, out);
    }
}

fn scan_expr_macro(m: &syn::ExprMacro, function: &str, file_path: &str, out: &mut Vec<AirItem>) {
    out.push(AirItem::CallSite(AirCallSite {
        callee: render_path(&m.mac.path),
        kind: CallKind::Meta,
        function: Some(function.to_string()),
        span: span_of(file_path, m.mac.span()),
    }));
}

fn scan_expr_method_call(
    m: &syn::ExprMethodCall,
    function: &str,
    file_path: &str,
    out: &mut Vec<AirItem>,
) {
    // Method-call CallSites carry just the method name — receiver-
    // type resolution is out of scope for this layer, so loaders
    // that need to disambiguate (`x.lock()` on Mutex vs File) will
    // need richer AIR. The CallSite is still useful: the bare name
    // is enough for some loaders (e.g. `.execute(...)` for SQL).
    out.push(AirItem::CallSite(AirCallSite {
        callee: m.method.to_string(),
        kind: CallKind::Method,
        function: Some(function.to_string()),
        span: span_of(file_path, m.span()),
    }));
    // ClosureMethodCall: emit when the first argument is a closure.
    // Lets paradigm rules (FL006 `map_err(|_| ...)`, future
    // closure-shape rules) inspect whether the closure discards
    // its argument.
    if let Some(Expr::Closure(closure)) = m.args.first() {
        let closure_discards_arg = closure_discards_first_arg(closure);
        let body_shape = classify_body(&closure.body);
        out.push(AirItem::ClosureMethodCall(AirClosureMethodCall {
            callee: m.method.to_string(),
            closure_discards_arg,
            body_shape,
            function: Some(function.to_string()),
            span: span_of(file_path, m.span()),
        }));
    }
    scan_expr_method_call_fallback(m, function, file_path, out);
    scan_expr(&m.receiver, function, file_path, out);
    for a in &m.args {
        scan_expr(a, function, file_path, out);
    }
}

/// Emit a `FallbackCall` item for `unwrap_or` / `unwrap_or_default` / `or`
/// method calls. FL010 consumes this to detect invalid-input-to-valid-default
/// conversions. `unwrap_or_default` takes no arg and is recorded with an
/// `Empty` shape so rules can distinguish it from explicit-default forms.
fn scan_expr_method_call_fallback(
    m: &syn::ExprMethodCall,
    function: &str,
    file_path: &str,
    out: &mut Vec<AirItem>,
) {
    // FallbackCall: emit for `unwrap_or` family — methods whose
    // first arg is a default-producing expression (literal /
    // call). FL010 fires on these to catch the "invalid input
    // converted to a valid default" pattern. `unwrap_or_default`
    // takes no arg and is recorded with `Empty` shape so rules
    // can distinguish it from explicit-default forms.
    let method_name = m.method.to_string();
    if !matches!(
        method_name.as_str(),
        "unwrap_or" | "unwrap_or_default" | "or"
    ) {
        return;
    }
    let default_shape = match m.args.first() {
        Some(arg) => classify_body(arg),
        None => ArmBodyShape::Empty,
    };
    // AIR v13: classify the Rust callee into the architectural
    // `FallbackPattern` so non-Rust adapters can map their idioms
    // (TS `??` / `||`, Go's two-value-fallback, Python `value or
    // default`) to the same shape. The Rust method name stays on
    // `callee` as evidence rules can quote.
    let pattern = match method_name.as_str() {
        "unwrap_or" => FallbackPattern::ValueOr,
        "or" => FallbackPattern::Or,
        "unwrap_or_default" => FallbackPattern::DefaultOr,
        // Future Rust callees that fit the family: unreachable
        // today because the outer `matches!` gate covers exactly
        // these three.
        _ => unreachable!("unhandled fallback callee {method_name:?}"),
    };
    out.push(AirItem::FallbackCall(AirFallbackCall {
        pattern,
        callee: method_name,
        default_shape,
        function: Some(function.to_string()),
        span: span_of(file_path, m.span()),
    }));
}

fn scan_expr_tuple(t: &syn::ExprTuple, function: &str, file_path: &str, out: &mut Vec<AirItem>) {
    for e in &t.elems {
        scan_expr(e, function, file_path, out);
    }
}

fn scan_expr_loop(l: &syn::ExprLoop, function: &str, file_path: &str, out: &mut Vec<AirItem>) {
    let propagates = block_has_propagate(&l.body);
    let has_break = block_has_break(&l.body);
    out.push(AirItem::RetryLoop(AirRetryLoop {
        loop_kind: LoopKind::Loop,
        propagates,
        has_break,
        function: Some(function.to_string()),
        span: span_of(file_path, l.span()),
    }));
    for stmt in &l.body.stmts {
        scan_stmt(stmt, function, file_path, out);
    }
}

fn scan_expr_for_loop(
    f: &syn::ExprForLoop,
    function: &str,
    file_path: &str,
    out: &mut Vec<AirItem>,
) {
    let propagates = block_has_propagate(&f.body);
    let has_break = block_has_break(&f.body);
    out.push(AirItem::RetryLoop(AirRetryLoop {
        loop_kind: LoopKind::For,
        propagates,
        has_break,
        function: Some(function.to_string()),
        span: span_of(file_path, f.span()),
    }));
    scan_expr(&f.expr, function, file_path, out);
    for stmt in &f.body.stmts {
        scan_stmt(stmt, function, file_path, out);
    }
}

fn scan_expr_while(w: &syn::ExprWhile, function: &str, file_path: &str, out: &mut Vec<AirItem>) {
    let propagates = block_has_propagate(&w.body);
    let has_break = block_has_break(&w.body);
    out.push(AirItem::RetryLoop(AirRetryLoop {
        loop_kind: LoopKind::While,
        propagates,
        has_break,
        function: Some(function.to_string()),
        span: span_of(file_path, w.span()),
    }));
    scan_expr(&w.cond, function, file_path, out);
    for stmt in &w.body.stmts {
        scan_stmt(stmt, function, file_path, out);
    }
}

fn block_has_propagate(block: &syn::Block) -> bool {
    block.stmts.iter().any(stmt_has_propagate)
}

fn block_has_break(block: &syn::Block) -> bool {
    block.stmts.iter().any(stmt_has_break)
}

fn stmt_has_break(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Local(l) => l
            .init
            .as_ref()
            .is_some_and(|init| expr_has_break(&init.expr)),
        Stmt::Expr(e, _) => expr_has_break(e),
        Stmt::Macro(_) | Stmt::Item(_) => false,
    }
}

fn expr_has_break(expr: &Expr) -> bool {
    match expr {
        Expr::Break(_) => true,
        Expr::If(i) => {
            i.then_branch.stmts.iter().any(stmt_has_break)
                || i.else_branch
                    .as_ref()
                    .is_some_and(|(_, e)| expr_has_break(e))
        }
        Expr::Match(m) => m.arms.iter().any(|arm| expr_has_break(&arm.body)),
        Expr::Block(b) => b.block.stmts.iter().any(stmt_has_break),
        Expr::Paren(p) => expr_has_break(&p.expr),
        // Don't recurse into nested loops — a `break` inside an inner
        // loop is for that loop, not the outer one. Same reasoning the
        // rust borrow-checker uses.
        Expr::Loop(_) | Expr::ForLoop(_) | Expr::While(_) => false,
        _ => false,
    }
}

/// Extract (rendered_value, kind) if `pat` is a literal pattern —
/// `Pat::Lit("active")`, `Pat::Lit(42)`, `Pat::Lit(true)`. Returns
/// `None` for tuple / struct / range / wildcard patterns.
fn literal_value_of_pat(pat: &Pat) -> Option<(String, LiteralKind)> {
    let Pat::Lit(lit) = pat else { return None };
    classify_literal(lit)
}

/// Extract (rendered_value, kind) if `expr` is a literal expression.
fn literal_value_of_expr(expr: &Expr) -> Option<(String, LiteralKind)> {
    if let Expr::Lit(lit) = expr {
        return classify_literal(lit);
    }
    if let Expr::Unary(u) = expr {
        // Negative numeric literals — `-1`, `-3.14` — are technically
        // unary expressions wrapping a positive literal. Render the
        // whole expression so the rule sees `-1` not `1`.
        if let syn::UnOp::Neg(_) = u.op
            && let Expr::Lit(_) = &*u.expr
            && let Some((_, k)) = classify_literal(if let Expr::Lit(l) = &*u.expr {
                l
            } else {
                unreachable!()
            })
        {
            return Some((expr.to_token_stream().to_string(), k));
        }
    }
    None
}

fn classify_literal(lit: &ExprLit) -> Option<(String, LiteralKind)> {
    let kind = match &lit.lit {
        Lit::Str(_) => LiteralKind::Str,
        Lit::Int(_) => LiteralKind::Int,
        Lit::Float(_) => LiteralKind::Float,
        Lit::Bool(_) => LiteralKind::Bool,
        // Char / ByteStr / Byte / Verbatim aren't decision-shaped
        // values in practice; skip them so CF002 / CF003 stay focused.
        _ => return None,
    };
    Some((lit.to_token_stream().to_string(), kind))
}

fn is_literal_expr(expr: &Expr) -> bool {
    matches!(expr, Expr::Lit(_))
        || matches!(expr, Expr::Unary(u) if matches!(u.op, syn::UnOp::Neg(_)) && matches!(&*u.expr, Expr::Lit(_)))
}

/// True when `pat` contains at least one wildcard binder (`_`) anywhere
/// in its tree — bare `_`, `Err(_)`, `Some(Foo(_, x))`, `(_, _)`. Used by
/// FL007 / FL011 / ER005 to detect catch-all arms that silently swallow
/// the unmatched case.
fn pat_has_wildcard(pat: &Pat) -> bool {
    match pat {
        Pat::Wild(_) => true,
        Pat::TupleStruct(ts) => ts.elems.iter().any(pat_has_wildcard),
        Pat::Tuple(t) => t.elems.iter().any(pat_has_wildcard),
        Pat::Struct(s) => s.fields.iter().any(|f| pat_has_wildcard(&f.pat)),
        Pat::Or(o) => o.cases.iter().any(pat_has_wildcard),
        Pat::Paren(p) => pat_has_wildcard(&p.pat),
        Pat::Reference(r) => pat_has_wildcard(&r.pat),
        Pat::Slice(s) => s.elems.iter().any(pat_has_wildcard),
        _ => false,
    }
}

/// Coarse classification of a match-arm body or closure body. The shape
/// vocabulary is shared (see [`ArmBodyShape`]).
///
/// `Empty` is reserved for unit `()` and `{}` blocks. `Literal` covers
/// bare literals. `Call` is a single function/method/macro call —
/// commonly `Default::default()`, `Vec::new()`, `default()`. `Return`
/// is any `return ...`. `Propagate` is detected when the body or any
/// transitively-reachable expression contains a `?`. `Block` is a
/// multi-statement block we don't want to pre-judge.
fn classify_body(expr: &Expr) -> ArmBodyShape {
    match expr {
        Expr::Tuple(t) if t.elems.is_empty() => ArmBodyShape::Empty,
        Expr::Block(b) if b.block.stmts.is_empty() => ArmBodyShape::Empty,
        Expr::Lit(_) => ArmBodyShape::Literal,
        Expr::Call(_) | Expr::MethodCall(_) | Expr::Macro(_) => {
            // A bare call expression is a "call" body (commonly a default
            // factory). Recurse into the call's args is unnecessary — we
            // only care about the top-level shape.
            ArmBodyShape::Call
        }
        Expr::Return(_) => ArmBodyShape::Return,
        Expr::Try(_) => ArmBodyShape::ErrorPropagation,
        Expr::Block(b) => {
            // Multi-statement block — check if any statement contains a `?`
            // that would propagate the error. If yes → Propagate; if any
            // contains a `return` → Return; otherwise → Block.
            let mut has_propagate = false;
            let mut has_return = false;
            for stmt in &b.block.stmts {
                if stmt_has_propagate(stmt) {
                    has_propagate = true;
                }
                if stmt_has_return(stmt) {
                    has_return = true;
                }
            }
            if has_propagate {
                ArmBodyShape::ErrorPropagation
            } else if has_return {
                ArmBodyShape::Return
            } else {
                ArmBodyShape::Block
            }
        }
        Expr::Paren(p) => classify_body(&p.expr),
        Expr::Reference(r) => classify_body(&r.expr),
        Expr::Path(_) => ArmBodyShape::Literal, // bare ident or path used as value
        _ => ArmBodyShape::Other,
    }
}

fn stmt_has_propagate(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Local(l) => l
            .init
            .as_ref()
            .is_some_and(|init| expr_has_propagate(&init.expr)),
        Stmt::Expr(e, _) => expr_has_propagate(e),
        Stmt::Macro(_) | Stmt::Item(_) => false,
    }
}

fn expr_has_propagate(expr: &Expr) -> bool {
    match expr {
        Expr::Try(_) => true,
        Expr::MethodCall(m) => {
            expr_has_propagate(&m.receiver) || m.args.iter().any(expr_has_propagate)
        }
        Expr::Call(c) => expr_has_propagate(&c.func) || c.args.iter().any(expr_has_propagate),
        Expr::Paren(p) => expr_has_propagate(&p.expr),
        Expr::Reference(r) => expr_has_propagate(&r.expr),
        Expr::Tuple(t) => t.elems.iter().any(expr_has_propagate),
        _ => false,
    }
}

fn stmt_has_return(stmt: &Stmt) -> bool {
    matches!(stmt, Stmt::Expr(Expr::Return(_), _))
}

/// True when the closure has zero parameters or its first parameter
/// pattern is a wildcard (`|_| ...`, `|| ...`, `|_, x| ...`). This is
/// the canonical "discarding the input" shape FL006 fires on for
/// `map_err(|_| ...)`.
fn closure_discards_first_arg(closure: &syn::ExprClosure) -> bool {
    let Some(first) = closure.inputs.first() else {
        return true; // `|| ...` — no args at all
    };
    pat_is_pure_wildcard(first)
}

/// Stricter wildcard check than [`pat_has_wildcard`]: only true when the
/// pattern is *exactly* `_` (or a parenthesised/referenced wrap of one).
/// `|(_, x)|` doesn't count as a discard — the `x` is being used.
fn pat_is_pure_wildcard(pat: &Pat) -> bool {
    match pat {
        Pat::Wild(_) => true,
        Pat::Paren(p) => pat_is_pure_wildcard(&p.pat),
        Pat::Reference(r) => pat_is_pure_wildcard(&r.pat),
        _ => false,
    }
}

/// Returns `"Ok"` or `"Err"` if `pat` is a tuple-struct pattern whose path
/// ends in either of those segments — i.e. `Ok(...)`, `Err(...)`,
/// `Result::Ok(...)`, etc. Returns `None` otherwise (other patterns,
/// custom enums, struct patterns, …).
fn result_variant_of_pat(pat: &Pat) -> Option<&'static str> {
    let Pat::TupleStruct(ts) = pat else {
        return None;
    };
    let last = ts.path.segments.last()?.ident.to_string();
    match last.as_str() {
        "Ok" => Some("Ok"),
        "Err" => Some("Err"),
        _ => None,
    }
}

fn is_string_compare(side: &Expr, other: &Expr) -> bool {
    let Expr::Lit(l) = other else { return false };
    if !matches!(l.lit, syn::Lit::Str(_)) {
        return false;
    }
    matches!(side, Expr::Field(_) | Expr::Path(_) | Expr::MethodCall(_))
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;

    fn items_for(src: &str) -> Vec<AirItem> {
        let file = syn::parse_str::<File>(src).expect("test source must parse");
        collect_items(&file, "t.rs", Some("x"))
    }

    fn silent_discards(items: &[AirItem]) -> Vec<&AirSilentDiscard> {
        items
            .iter()
            .filter_map(|i| match i {
                AirItem::SilentDiscard(d) => Some(d),
                _ => None,
            })
            .collect()
    }

    fn partial_if_lets(items: &[AirItem]) -> Vec<&AirPartialResultMatch> {
        items
            .iter()
            .filter_map(|i| match i {
                AirItem::PartialResultMatch(p) => Some(p),
                _ => None,
            })
            .collect()
    }

    // ---- SilentDiscard emission ----

    #[test]
    fn silent_discard_method_call_records_method_kind_and_name() {
        let items = items_for(indoc! {r#"
            fn run() {
                let _ = thing.send(payload);
            }
        "#});
        let discards = silent_discards(&items);
        assert_eq!(discards.len(), 1);
        assert_eq!(discards[0].callee.as_deref(), Some("send"));
        assert_eq!(discards[0].kind, DiscardKind::Method);
        assert_eq!(discards[0].function.as_deref(), Some("x::run"));
    }

    #[test]
    fn silent_discard_function_call_records_function_kind_and_path() {
        let items = items_for(indoc! {r#"
            fn run() {
                let _ = std::fs::write(p, b);
            }
        "#});
        let discards = silent_discards(&items);
        assert_eq!(discards.len(), 1);
        assert_eq!(discards[0].callee.as_deref(), Some("std::fs::write"));
        assert_eq!(discards[0].kind, DiscardKind::Function);
    }

    #[test]
    fn silent_discard_macro_call_records_macro_kind_and_path() {
        let items = items_for(indoc! {r#"
            fn run() {
                let _ = vec![1, 2, 3];
            }
        "#});
        let discards = silent_discards(&items);
        assert_eq!(discards.len(), 1);
        assert_eq!(discards[0].callee.as_deref(), Some("vec"));
        assert_eq!(discards[0].kind, DiscardKind::Meta);
    }

    #[test]
    fn silent_discard_arbitrary_expression_records_other_kind_with_no_callee() {
        let items = items_for(indoc! {r#"
            fn run() {
                let _ = self.field;
            }
        "#});
        let discards = silent_discards(&items);
        assert_eq!(discards.len(), 1);
        assert!(discards[0].callee.is_none());
        assert_eq!(discards[0].kind, DiscardKind::Other);
    }

    #[test]
    fn silent_discard_peels_through_paren_ref_and_try() {
        let items = items_for(indoc! {r#"
            fn run() {
                let _ = (thing.send(payload));
                let _ = &thing.lock();
                let _ = thing.send(payload)?;
            }
        "#});
        let discards = silent_discards(&items);
        assert_eq!(discards.len(), 3);
        for d in &discards {
            assert_eq!(d.kind, DiscardKind::Method);
        }
    }

    #[test]
    fn non_wildcard_let_does_not_emit_silent_discard() {
        let items = items_for(indoc! {r#"
            fn run() {
                let x = thing.send(payload);
                let _y = thing.send(payload);
            }
        "#});
        assert!(silent_discards(&items).is_empty());
    }

    // ---- PartialIfLet emission ----

    #[test]
    fn partial_if_let_ok_without_else_records_ok_variant() {
        let items = items_for(indoc! {r#"
            fn run() {
                if let Ok(x) = parse_thing() {
                    use_value(x);
                }
            }
        "#});
        let parts = partial_if_lets(&items);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].variant, ResultMatchVariant::Success);
        assert_eq!(parts[0].function.as_deref(), Some("x::run"));
    }

    #[test]
    fn partial_if_let_err_without_else_records_err_variant() {
        let items = items_for(indoc! {r#"
            fn run() {
                if let Err(e) = parse_thing() {
                    log_error(e);
                }
            }
        "#});
        let parts = partial_if_lets(&items);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].variant, ResultMatchVariant::Failure);
    }

    #[test]
    fn if_let_with_else_does_not_emit_partial_if_let() {
        let items = items_for(indoc! {r#"
            fn run() {
                if let Ok(x) = parse_thing() {
                    use_value(x);
                } else {
                    handle_error();
                }
            }
        "#});
        assert!(partial_if_lets(&items).is_empty());
    }

    #[test]
    fn if_let_on_non_result_pattern_does_not_emit() {
        let items = items_for(indoc! {r#"
            fn run() {
                if let Some(x) = optional() {
                    use_value(x);
                }
                if let MyEnum::Variant(x) = thing {
                    use_value(x);
                }
            }
        "#});
        assert!(partial_if_lets(&items).is_empty());
    }

    #[test]
    fn if_let_with_path_qualified_ok_still_emits() {
        // `Result::Ok(x)` should still be recognised — last path segment wins.
        let items = items_for(indoc! {r#"
            fn run() {
                if let Result::Ok(x) = parse_thing() {
                    use_value(x);
                }
            }
        "#});
        let parts = partial_if_lets(&items);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].variant, ResultMatchVariant::Success);
    }

    // ---- MatchArm emission ----

    fn match_arms(items: &[AirItem]) -> Vec<&AirMatchArm> {
        items
            .iter()
            .filter_map(|i| match i {
                AirItem::MatchArm(a) => Some(a),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn match_arm_records_pattern_and_wildcard_flag() {
        let items = items_for(indoc! {r#"
            fn run(r: Result<i32, String>) {
                match r {
                    Ok(x) => use_value(x),
                    Err(_) => (),
                }
            }
        "#});
        let arms = match_arms(&items);
        assert_eq!(arms.len(), 2);
        assert!(arms[0].pattern.contains("Ok"));
        assert!(!arms[0].pattern_has_wildcard);
        assert!(arms[1].pattern.contains("Err"));
        assert!(arms[1].pattern_has_wildcard);
    }

    #[test]
    fn match_arm_body_shape_classifications() {
        let items = items_for(indoc! {r#"
            fn run(r: Result<i32, String>) -> Result<i32, String> {
                match r {
                    Ok(x) => x,
                    Err(_) => 0,
                }
            }
        "#});
        let arms = match_arms(&items);
        // Ok(x) => x  : path expr, classifies as Literal-ish
        // Err(_) => 0 : Literal
        assert!(matches!(
            arms[1].body_shape,
            ArmBodyShape::Literal | ArmBodyShape::Other
        ));
    }

    #[test]
    fn match_arm_body_shape_call_for_default_factory() {
        let items = items_for(indoc! {r#"
            fn run(r: Result<i32, String>) -> i32 {
                match r {
                    Ok(x) => x,
                    Err(_) => i32::default(),
                }
            }
        "#});
        let arms = match_arms(&items);
        let err_arm = arms.iter().find(|a| a.pattern_has_wildcard).unwrap();
        assert_eq!(err_arm.body_shape, ArmBodyShape::Call);
    }

    #[test]
    fn match_arm_body_shape_propagate_for_question_mark() {
        let items = items_for(indoc! {r#"
            fn run(r: Result<i32, String>) -> Result<i32, String> {
                match r {
                    Ok(x) => Ok(x),
                    Err(e) => {
                        let v = parse(&e)?;
                        Ok(v)
                    }
                }
            }
        "#});
        let arms = match_arms(&items);
        // Err arm has `?` → Propagate
        let err_arm = arms.iter().find(|a| a.pattern.starts_with("Err")).unwrap();
        assert_eq!(err_arm.body_shape, ArmBodyShape::ErrorPropagation);
    }

    #[test]
    fn match_arm_pattern_wildcard_in_nested_position() {
        let items = items_for(indoc! {r#"
            fn run(t: (i32, String)) {
                match t {
                    (0, _) => (),
                    (_, _) => (),
                    _ => (),
                }
            }
        "#});
        let arms = match_arms(&items);
        assert_eq!(arms.len(), 3);
        for arm in &arms {
            assert!(arm.pattern_has_wildcard, "pattern: {}", arm.pattern);
        }
    }

    // ---- ClosureMethodCall emission ----

    fn closure_method_calls(items: &[AirItem]) -> Vec<&AirClosureMethodCall> {
        items
            .iter()
            .filter_map(|i| match i {
                AirItem::ClosureMethodCall(c) => Some(c),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn map_err_with_underscore_records_discarding_closure() {
        let items = items_for(indoc! {r#"
            fn run() -> Result<i32, String> {
                let x: Result<i32, std::io::Error> = Ok(1);
                x.map_err(|_| "oops".to_string())
            }
        "#});
        let cmcs = closure_method_calls(&items);
        let map_err = cmcs.iter().find(|c| c.callee == "map_err").unwrap();
        assert!(map_err.closure_discards_arg);
    }

    #[test]
    fn map_err_using_arg_records_non_discarding_closure() {
        let items = items_for(indoc! {r#"
            fn run() -> Result<i32, String> {
                let x: Result<i32, std::io::Error> = Ok(1);
                x.map_err(|e| format!("io error: {e}"))
            }
        "#});
        let cmcs = closure_method_calls(&items);
        let map_err = cmcs.iter().find(|c| c.callee == "map_err").unwrap();
        assert!(!map_err.closure_discards_arg);
    }

    #[test]
    fn unwrap_or_else_with_no_arg_closure_is_treated_as_discarding() {
        let items = items_for(indoc! {r#"
            fn run() -> i32 {
                let x: Option<i32> = None;
                x.unwrap_or_else(|| 0)
            }
        "#});
        let cmcs = closure_method_calls(&items);
        let uo = cmcs.iter().find(|c| c.callee == "unwrap_or_else").unwrap();
        assert!(uo.closure_discards_arg); // no-arg closure is "no input being used"
    }

    #[test]
    fn method_call_without_closure_arg_does_not_emit_closure_method_call() {
        let items = items_for(indoc! {r#"
            fn run() {
                vec.push(42);
            }
        "#});
        assert!(closure_method_calls(&items).is_empty());
    }

    // ---- FallbackCall emission ----

    fn fallback_calls(items: &[AirItem]) -> Vec<&AirFallbackCall> {
        items
            .iter()
            .filter_map(|i| match i {
                AirItem::FallbackCall(c) => Some(c),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn unwrap_or_with_literal_records_literal_default_shape() {
        let items = items_for(indoc! {r#"
            fn run(x: Result<i32, String>) -> i32 {
                x.unwrap_or(0)
            }
        "#});
        let calls = fallback_calls(&items);
        let uo = calls.iter().find(|c| c.callee == "unwrap_or").unwrap();
        assert_eq!(uo.default_shape, ArmBodyShape::Literal);
    }

    #[test]
    fn unwrap_or_with_call_records_call_default_shape() {
        let items = items_for(indoc! {r#"
            fn run(x: Result<Vec<i32>, String>) -> Vec<i32> {
                x.unwrap_or(Vec::new())
            }
        "#});
        let calls = fallback_calls(&items);
        let uo = calls.iter().find(|c| c.callee == "unwrap_or").unwrap();
        assert_eq!(uo.default_shape, ArmBodyShape::Call);
    }

    #[test]
    fn unwrap_or_default_records_empty_default_shape() {
        let items = items_for(indoc! {r#"
            fn run(x: Result<i32, String>) -> i32 {
                x.unwrap_or_default()
            }
        "#});
        let calls = fallback_calls(&items);
        let uod = calls
            .iter()
            .find(|c| c.callee == "unwrap_or_default")
            .unwrap();
        assert_eq!(uod.default_shape, ArmBodyShape::Empty);
    }

    #[test]
    fn or_method_records_fallback_call_too() {
        let items = items_for(indoc! {r#"
            fn run(x: Option<i32>) -> Option<i32> {
                x.or(Some(0))
            }
        "#});
        let calls = fallback_calls(&items);
        assert!(calls.iter().any(|c| c.callee == "or"));
    }

    // ---- RetryLoop emission ----

    fn retry_loops(items: &[AirItem]) -> Vec<&AirRetryLoop> {
        items
            .iter()
            .filter_map(|i| match i {
                AirItem::RetryLoop(l) => Some(l),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn loop_with_propagate_and_break_records_retry_shape() {
        let items = items_for(indoc! {r#"
            fn run() -> Result<i32, String> {
                loop {
                    let v = try_thing()?;
                    if v > 0 {
                        break;
                    }
                }
                Ok(0)
            }
        "#});
        let loops = retry_loops(&items);
        assert_eq!(loops.len(), 1);
        assert_eq!(loops[0].loop_kind, LoopKind::Loop);
        assert!(loops[0].propagates);
        assert!(loops[0].has_break);
    }

    #[test]
    fn for_loop_with_propagate_no_break_still_emits_retry_loop_item() {
        let items = items_for(indoc! {r#"
            fn run() -> Result<(), String> {
                for _ in 0..3 {
                    try_thing()?;
                }
                Ok(())
            }
        "#});
        let loops = retry_loops(&items);
        assert_eq!(loops.len(), 1);
        assert_eq!(loops[0].loop_kind, LoopKind::For);
        assert!(loops[0].propagates);
        assert!(!loops[0].has_break);
    }

    #[test]
    fn while_loop_records_kind() {
        let items = items_for(indoc! {r#"
            fn run() -> Result<(), String> {
                while !done() {
                    try_thing()?;
                    if maybe_break() {
                        break;
                    }
                }
                Ok(())
            }
        "#});
        let loops = retry_loops(&items);
        assert_eq!(loops.len(), 1);
        assert_eq!(loops[0].loop_kind, LoopKind::While);
        assert!(loops[0].propagates);
        assert!(loops[0].has_break);
    }

    // ---- ScrutineeLiteral emission ----

    fn scrutinee_literals(items: &[AirItem]) -> Vec<&AirScrutineeLiteral> {
        items
            .iter()
            .filter_map(|i| match i {
                AirItem::ScrutineeLiteral(l) => Some(l),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn match_arm_string_literal_pattern_records_scrutinee_literal() {
        let items = items_for(indoc! {r#"
            fn run(s: &str) {
                match s {
                    "active" => println!("a"),
                    "inactive" => println!("i"),
                    _ => (),
                }
            }
        "#});
        let lits = scrutinee_literals(&items);
        let strs: Vec<&str> = lits.iter().map(|l| l.value.as_str()).collect();
        assert!(strs.iter().any(|v| v.contains("active")));
        assert!(strs.iter().any(|v| v.contains("inactive")));
        assert!(lits.iter().all(|l| l.kind == LiteralKind::Str));
        assert!(lits.iter().all(|l| l.context == LiteralContext::MatchArm));
    }

    #[test]
    fn match_arm_int_literal_pattern_records_int_kind() {
        let items = items_for(indoc! {r#"
            fn run(n: i32) {
                match n {
                    0 => "zero",
                    42 => "answer",
                    _ => "other",
                }
            }
        "#});
        let lits = scrutinee_literals(&items);
        assert!(
            lits.iter()
                .any(|l| l.value.contains("42") && l.kind == LiteralKind::Int)
        );
    }

    #[test]
    fn binary_eq_with_literal_rhs_records_scrutinee_literal() {
        let items = items_for(indoc! {r#"
            fn run(role: &str) -> bool {
                role == "admin"
            }
        "#});
        let lits = scrutinee_literals(&items);
        let admin = lits.iter().find(|l| l.value.contains("admin")).unwrap();
        assert_eq!(admin.kind, LiteralKind::Str);
        assert_eq!(admin.context, LiteralContext::BinaryCompare);
    }

    #[test]
    fn binary_compare_two_literals_does_not_emit_scrutinee_literal() {
        // `if 1 == 2 { ... }` — both sides are literals; this isn't a
        // decision against runtime data, so we don't record it.
        let items = items_for(indoc! {r#"
            fn run() -> bool {
                1 == 2
            }
        "#});
        let lits = scrutinee_literals(&items);
        assert!(lits.is_empty());
    }
}
