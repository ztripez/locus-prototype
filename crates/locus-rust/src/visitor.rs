//! `syn::File` → `Vec<AirItem>`.
//!
//! Phase 1 emits direct extractions only. Inference (concept overlap,
//! boundary roles) lives in `locus-core`.

use crate::type_render::{render_path, render_type};
use locus_air::{
    ActionKind, AirCallSite, AirConversion, AirField, AirFunction, AirImpl, AirImport, AirItem,
    AirPartialIfLet, AirSilentDiscard, AirSpan, AirTruthAction, AirType, AirVariant, CallKind,
    ConversionMechanism, DiscardKind, TypeKind, Visibility,
};
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
    let fields = collect_named_fields(&s.fields);
    let (derives, attrs, doc) = split_attrs(&s.attrs);
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
        doc,
    }));
}

fn emit_enum(e: &ItemEnum, module: &str, file_path: &str, out: &mut Vec<AirItem>) {
    let name = e.ident.to_string();
    let symbol = format!("{module}::{name}");
    let (derives, attrs, doc) = split_attrs(&e.attrs);
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
        doc,
    }));
}

fn emit_alias(a: &ItemType, module: &str, file_path: &str, out: &mut Vec<AirItem>) {
    let name = a.ident.to_string();
    let symbol = format!("{module}::{name}");
    let (derives, attrs, doc) = split_attrs(&a.attrs);
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
        doc,
    }));
}

fn emit_union(u: &ItemUnion, module: &str, file_path: &str, out: &mut Vec<AirItem>) {
    let name = u.ident.to_string();
    let symbol = format!("{module}::{name}");
    let (derives, attrs, doc) = split_attrs(&u.attrs);
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
        doc,
    }));
}

fn emit_trait(t: &ItemTrait, module: &str, file_path: &str, out: &mut Vec<AirItem>) {
    let name = t.ident.to_string();
    let symbol = format!("{module}::{name}");
    let (derives, attrs, doc) = split_attrs(&t.attrs);
    out.push(AirItem::Type(AirType {
        kind: TypeKind::Trait,
        name,
        symbol,
        visibility: vis_of(&t.vis),
        fields: Vec::new(),
        variants: Vec::new(),
        derives,
        attrs,
        span: span_of(file_path, t.span()),
        doc,
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

    let span = span_of(file_path, f.span());
    let line_count = span.line_end.saturating_sub(span.line_start) + 1;
    let (_derives, _attrs, doc) = split_attrs(&f.attrs);

    out.push(AirItem::Function(AirFunction {
        name: name.clone(),
        symbol: symbol.clone(),
        visibility: vis_of(&f.vis),
        params: params.clone(),
        return_type: return_type.clone(),
        span: span.clone(),
        line_count,
        doc,
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

    // Emit a summary AirItem::Impl for every impl block. Conversion-shaped
    // impls additionally produce AirItem::Conversion below; that overlap is
    // intentional — different paradigms (PA, AB) consume the impl summary,
    // while OT consumes the conversion view.
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
    out.push(AirItem::Impl(AirImpl {
        trait_path: trait_path_text,
        self_ty: self_ty.clone(),
        method_names,
        span: span_of(file_path, i.span()),
    }));

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

/// Returns `(derives, other_attrs, doc)`. Doc attrs (both `///` and
/// `#[doc = "..."]`, which `syn` normalizes into the same shape) are
/// extracted into `doc` and removed from `other_attrs` so they don't double-
/// count under `AirType.attrs` / `AirFunction` attrs.
fn split_attrs(attrs: &[syn::Attribute]) -> (Vec<String>, Vec<String>, Option<String>) {
    let mut derives = Vec::new();
    let mut others = Vec::new();
    let mut doc_lines: Vec<String> = Vec::new();
    for a in attrs {
        if a.path().is_ident("derive") {
            let _ = a.parse_nested_meta(|meta| {
                derives.push(meta.path.to_token_stream().to_string());
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
            others.push(a.to_token_stream().to_string());
        }
    }
    let doc = if doc_lines.is_empty() {
        None
    } else {
        Some(doc_lines.join("\n"))
    };
    (derives, others, doc)
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
                kind: CallKind::Macro,
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
        Expr::Macro(m) => (Some(render_path(&m.mac.path)), DiscardKind::Macro),
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
            // `if let Ok(...) = expr { ... }` or `if let Err(...) = expr
            // { ... }` *without* an `else` branch. The unmatched arm is
            // silent — the failure (or success) just falls through. FL005
            // consumes this signal.
            if i.else_branch.is_none()
                && let Expr::Let(let_expr) = &*i.cond
                && let Some(variant) = result_variant_of_pat(&let_expr.pat)
            {
                out.push(AirItem::PartialIfLet(AirPartialIfLet {
                    variant: variant.to_string(),
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
        Expr::Call(c) => {
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
        Expr::Macro(m) => {
            out.push(AirItem::CallSite(AirCallSite {
                callee: render_path(&m.mac.path),
                kind: CallKind::Macro,
                function: Some(function.to_string()),
                span: span_of(file_path, m.mac.span()),
            }));
        }
        Expr::MethodCall(m) => {
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

    fn partial_if_lets(items: &[AirItem]) -> Vec<&AirPartialIfLet> {
        items
            .iter()
            .filter_map(|i| match i {
                AirItem::PartialIfLet(p) => Some(p),
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
        assert_eq!(discards[0].kind, DiscardKind::Macro);
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
        assert_eq!(parts[0].variant, "Ok");
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
        assert_eq!(parts[0].variant, "Err");
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
        assert_eq!(parts[0].variant, "Ok");
    }
}
