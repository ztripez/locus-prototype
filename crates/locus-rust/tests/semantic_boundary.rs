//! Boundary tests for the Rust adapter (issue #110).
//!
//! Each test pins one aspect of `docs/RUST_ADAPTER.md`. Together they
//! exercise the four adapter layers and lock in *which* cases are
//! supported and which are intentionally unresolved. If you change
//! adapter behaviour and one of these tests fails, update the test
//! and the doc together — the test's job is to keep the doc honest.
//!
//! Sections map to the layers in the doc:
//!   - layer 1: raw source scan (comments → hints, no AST)
//!   - layer 2: `syn` AST scan (items, parse-error containment)
//!   - layer 3: rendered AIR text (whitespace normalisation, lossy bits)
//!   - layer 4: heuristic inference (converter name shape, std-rt path
//!     matching, module-path inference from file location)
//!
//! Where a test pins a *false-positive* shape (e.g. `*::spawn` matches
//! a user-defined `my_spawn::spawn`), the assertion exists to ensure
//! that behaviour stays bounded — not because the heuristic is correct
//! in that case. See "Layer 4" in the doc.

use indoc::indoc;
use locus_air::{
    AirFile, AirFunction, AirItem, AirPackage, AirSpan, AirWorkspace, ConversionMechanism,
    FactKind, HintKind, TypeKind, Visibility,
};
use locus_core::loaders::Loader;
use locus_rust::{StdRtLoader, collect_items, derive_module_path, render_type, scan_hints};

// ─── helpers ────────────────────────────────────────────────────────────

fn parse(source: &str, file_path: &str, module: Option<&str>) -> Vec<AirItem> {
    let file = syn::parse_file(source).expect("source parses");
    collect_items(&file, file_path, module)
}

// ─── layer 1: raw source scan ───────────────────────────────────────────

#[test]
fn layer1_hint_binds_to_next_non_blank_line() {
    let src = indoc! {r#"
        // locus: ot canonical
        pub struct User {
            id: u32,
        }
    "#};
    let hints = scan_hints(src, "t.rs");
    assert_eq!(hints.len(), 1, "expected one hint, got {hints:?}");
    assert!(matches!(hints[0].kind, HintKind::Canonical));
    let target = hints[0].target_span.as_ref().expect("target bound");
    // `pub struct User` is on line 2.
    assert_eq!(target.line_start, 2);
}

#[test]
fn layer1_hint_skips_attributes_to_reach_the_item() {
    // The scanner deliberately skips `#[...]` attribute lines so a hint
    // placed above `#[derive(...)] pub struct X` still binds to the
    // struct itself, not to the derive line.
    let src = indoc! {r#"
        // locus: ot canonical
        #[derive(Debug, Clone)]
        pub struct User;
    "#};
    let hints = scan_hints(src, "t.rs");
    let target = hints[0].target_span.as_ref().expect("target bound");
    assert_eq!(
        target.line_start, 3,
        "hint must bind past the #[derive] line"
    );
}

#[test]
fn layer1_hints_inside_raw_strings_are_ignored() {
    // Hints appearing inside multi-line raw strings (common in this
    // crate's own unit tests via `indoc!`) must not be promoted.
    let src = indoc! {r##"
        fn doc() -> &'static str {
            r#"
            // locus: ot canonical
            this is documentation, not a hint
            "#
        }
    "##};
    let hints = scan_hints(src, "t.rs");
    assert!(
        hints.is_empty(),
        "hints inside raw-string blocks must be ignored; got {hints:?}"
    );
}

#[test]
fn layer1_unknown_hint_form_does_not_panic() {
    // Forward-compatibility: an unknown hint subform should produce a
    // hint with `HintKind::Unknown` (or similar) rather than aborting.
    // This is a smoke test — the exact unknown variant is part of the
    // AIR schema and tested in locus-air.
    let src = "// locus: future-subform with-args\n";
    let hints = scan_hints(src, "t.rs");
    assert_eq!(hints.len(), 1, "scanner should still emit a hint entry");
}

// ─── layer 2: syn AST scan ──────────────────────────────────────────────

#[test]
fn layer2_emits_typed_items_for_struct_enum_function() {
    let src = indoc! {r#"
        pub struct A { x: u32 }
        pub enum B { One, Two }
        pub fn c() -> u32 { 0 }
    "#};
    let items = parse(src, "t.rs", Some("pkg"));
    let kinds: Vec<&'static str> = items
        .iter()
        .map(|i| match i {
            AirItem::Type(t) => match t.kind {
                TypeKind::Struct => "struct",
                TypeKind::Enum => "enum",
                _ => "other-type",
            },
            AirItem::Function(_) => "fn",
            AirItem::Impl(_) => "impl",
            AirItem::Conversion(_) => "conv",
            AirItem::Import(_) => "import",
            _ => "other",
        })
        .collect();
    assert!(kinds.contains(&"struct"));
    assert!(kinds.contains(&"enum"));
    assert!(kinds.contains(&"fn"));
}

#[test]
fn layer2_macro_invocation_is_observed_not_expanded() {
    // The visitor captures statement-level macros as a CallSite with
    // `CallKind::Meta`. It does NOT expand them — so the body of
    // `println!(...)` is not visible as further code, and a macro that
    // expands to a struct definition is invisible at item level.
    let src = indoc! {r#"
        pub fn f() {
            println!("hello");
        }
    "#};
    let items = parse(src, "t.rs", Some("pkg"));
    // The println! macro shows up as a CallSite, not as further items.
    let has_call_site = items
        .iter()
        .any(|i| matches!(i, AirItem::CallSite(c) if c.callee.contains("println")));
    assert!(
        has_call_site,
        "expected a CallSite for `println!`; got {items:#?}"
    );
}

#[test]
fn layer2_parse_error_does_not_abort_the_scanner() {
    // End-to-end pin of `scan_file`'s parse-error containment: a file
    // with broken Rust must produce an `AirFile` with
    // `parse_error: Some(...)` and `items: []` while the rest of the
    // workspace scans normally. A weaker test that only exercised
    // `syn::parse_file` would still pass if `scan_file` regressed to
    // panicking or dropping `parse_error`.
    //
    // Uses `CARGO_TARGET_TMPDIR` so no extra dev-dep is needed.
    use std::fs;
    let tmp_root = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let crate_dir = tmp_root.join("semantic_boundary_parse_error");
    // Clean up any prior run's directory so the test is hermetic.
    let _ = fs::remove_dir_all(&crate_dir);
    let src_dir = crate_dir.join("src");
    fs::create_dir_all(&src_dir).expect("mkdir src");
    fs::write(
        crate_dir.join("Cargo.toml"),
        indoc! {r#"
            # Empty [workspace] keeps this temp crate out of the outer
            # locus workspace so `cargo metadata` doesn't complain.
            [workspace]

            [package]
            name = "brokencrate"
            version = "0.0.0"
            edition = "2024"

            [lib]
            path = "src/lib.rs"
        "#},
    )
    .expect("Cargo.toml");
    fs::write(src_dir.join("lib.rs"), "pub fn ok() {}\n").expect("lib.rs");
    fs::write(src_dir.join("bad.rs"), "pub fn !!! not rust\n").expect("bad.rs");

    let air = locus_rust::scan_raw(&crate_dir).expect("scan_raw must not abort");

    let pkg = air
        .packages
        .iter()
        .find(|p| p.name == "brokencrate")
        .expect("brokencrate package scanned");
    let bad = pkg
        .files
        .iter()
        .find(|f| f.path.ends_with("bad.rs"))
        .expect("bad.rs included in scan");
    assert!(
        bad.parse_error.is_some(),
        "parse_error must be set on broken source; got {bad:?}"
    );
    assert!(
        bad.items.is_empty(),
        "items must be empty on parse failure; got {:?}",
        bad.items
    );
    let ok = pkg
        .files
        .iter()
        .find(|f| f.path.ends_with("lib.rs"))
        .expect("lib.rs included in scan");
    assert!(
        ok.parse_error.is_none(),
        "valid file's parse_error must stay None"
    );
}

// ─── layer 3: rendered AIR text ─────────────────────────────────────────

#[test]
fn layer3_render_type_normalises_whitespace() {
    // `syn` produces tokens with spacy separation (`Result < User , E >`);
    // the renderer must collapse that to a stable, comparable form.
    let ty: syn::Type = syn::parse_str("Result<User, Error>").unwrap();
    assert_eq!(render_type(&ty), "Result<User, Error>");

    let ty2: syn::Type = syn::parse_str("Vec<Result<User, Error>>").unwrap();
    assert_eq!(render_type(&ty2), "Vec<Result<User, Error>>");
}

#[test]
fn layer3_lifetimes_are_string_only_not_separately_tracked() {
    // A type with a lifetime renders as part of the string. The adapter
    // does not extract lifetimes into their own field; rules comparing
    // two `&'a T` and `&'b T` see two different *strings* even though
    // the language treats the parameters as distinct binders. This test
    // pins the rendered form — it does NOT assert any kind of
    // lifetime resolution.
    let ty: syn::Type = syn::parse_str("&'a User").unwrap();
    assert_eq!(
        render_type(&ty),
        "&'a User",
        "lifetime is part of the rendered string"
    );
}

#[test]
fn layer3_generic_aliases_are_not_resolved() {
    // `type MyResult<T> = Result<T, MyError>;` does not unify with
    // `Result<T, E>` at the AIR level — the renderer emits whatever
    // type name appears in the source. Rules wanting "is this a Result"
    // must check the literal `Result<...>` form, not chase aliases.
    let ty: syn::Type = syn::parse_str("MyResult<User>").unwrap();
    assert_eq!(render_type(&ty), "MyResult<User>");
}

// ─── layer 4: heuristic inference ───────────────────────────────────────

#[test]
fn layer4_inherent_to_with_unit_return_still_emits_conversion() {
    // The inherent-method converter heuristic fires on `to_*` / `into_*`
    // when the return type is distinct from `Self`. It does NOT verify
    // that the return type is a meaningful conversion target — so an
    // `fn to_inert(&self) -> ()` is still emitted as an AirConversion
    // because `()` != `Self`. This is a documented false-positive
    // shape; the test ensures the behaviour stays bounded.
    let src = indoc! {r#"
        pub struct S;
        impl S {
            pub fn to_inert(&self) -> () {}
        }
    "#};
    let items = parse(src, "t.rs", Some("pkg"));
    let conversions: Vec<&_> = items
        .iter()
        .filter_map(|i| match i {
            AirItem::Conversion(c) => Some(c),
            _ => None,
        })
        .collect();
    assert_eq!(
        conversions.len(),
        1,
        "expected one heuristic conversion despite the empty-tuple return; \
         got {conversions:?}"
    );
    assert_eq!(conversions[0].from, "S");
    assert_eq!(conversions[0].to, "()");
    assert_eq!(
        conversions[0].mechanism,
        ConversionMechanism::InstanceMethod
    );
}

#[test]
fn layer4_free_fn_converter_requires_exactly_one_param() {
    // The free-function converter heuristic requires exactly one
    // parameter. A `fn from_x()` with zero or two parameters is NOT
    // emitted as a conversion. This is an intentional guard against
    // the most obvious false positives.
    let src = indoc! {r#"
        pub fn from_zero() -> u32 { 0 }
        pub fn from_two(_a: u32, _b: u32) -> u64 { 0 }
        pub fn from_one(_a: u32) -> u64 { 0 }
    "#};
    let items = parse(src, "t.rs", Some("pkg"));
    let conversions: Vec<&_> = items
        .iter()
        .filter_map(|i| match i {
            AirItem::Conversion(c) => Some(c),
            _ => None,
        })
        .collect();
    assert_eq!(
        conversions.len(),
        1,
        "only `from_one` should emit a conversion; got {conversions:?}"
    );
    assert_eq!(conversions[0].from, "u32");
    assert_eq!(conversions[0].to, "u64");
}

#[test]
fn layer4_strip_result_or_option_keeps_intermediate_wrapper() {
    // `strip_result_or_option` is internal but its observable effect is
    // visible on a converter's `to` field: `Result<Option<U>, E>` strips
    // the outermost `Result<>` only, leaving `Option<U>` — not `U`. This
    // is a known string-slicing limitation, pinned here.
    let src = indoc! {r#"
        pub fn from_x(_a: u32) -> Result<Option<User>, Error> { unimplemented!() }
    "#};
    let items = parse(src, "t.rs", Some("pkg"));
    let conv = items
        .iter()
        .find_map(|i| match i {
            AirItem::Conversion(c) => Some(c),
            _ => None,
        })
        .expect("conversion emitted");
    assert_eq!(
        conv.to, "Option<User>",
        "outer Result<> is stripped; inner Option<> is preserved"
    );
}

#[test]
fn layer4_std_rt_loader_matches_user_defined_spawn() {
    // The std-rt loader recognises `*::spawn` by trailing-segment match.
    // A user-defined `my_executor::spawn` therefore matches as well —
    // this is a known false-positive surface because the loader cannot
    // see the receiver type without name resolution. Test pins the
    // bounded behaviour.
    let workspace = synthesize_workspace_with_call(
        "pkg",
        "src/lib.rs",
        "pkg::lib",
        "fn caller",
        "my_executor::spawn",
    );
    let facts = StdRtLoader.enrich(&workspace);
    assert!(
        facts
            .iter()
            .any(|f| matches!(f.kind, FactKind::SpawnedWork)),
        "std-rt's `*::spawn` pattern matches user paths; got facts {facts:?}"
    );
}

#[test]
fn layer4_std_rt_loader_skips_method_calls() {
    // Method-form call sites (`x.spawn(…)`) cannot be classified by the
    // loader without receiver-type information. The visitor emits them
    // as `CallKind::Method`, and `StdRtLoader::classify` skips them.
    let workspace = synthesize_workspace_with_method_call(
        "pkg",
        "src/lib.rs",
        "pkg::lib",
        "fn caller",
        "spawn",
    );
    let facts = StdRtLoader.enrich(&workspace);
    assert!(
        !facts
            .iter()
            .any(|f| matches!(f.kind, FactKind::SpawnedWork)),
        "method-form `.spawn(...)` must not produce a SpawnedWork fact; \
         got facts {facts:?}"
    );
}

#[test]
fn layer4_module_path_inferred_from_filesystem_only() {
    // `derive_module_path` is a heuristic: it builds the module path
    // from the file's location relative to `src/`. It does not honour
    // `#[path = "..."]` attributes or `#[cfg(...)]`-gated module
    // arrangements. Test pins the filesystem-only behaviour.
    let pkg_root = std::path::Path::new("/tmp/fake-pkg");
    let mod_path = derive_module_path(pkg_root, &pkg_root.join("src/a/b/c.rs"), "fake_pkg");
    assert_eq!(
        mod_path.as_deref(),
        Some("fake_pkg::a::b::c"),
        "module path comes from filesystem layout"
    );

    // lib.rs collapses to the crate name only.
    let lib_path = derive_module_path(pkg_root, &pkg_root.join("src/lib.rs"), "fake_pkg");
    assert_eq!(lib_path.as_deref(), Some("fake_pkg"));
}

// ─── test-only AIR builders ─────────────────────────────────────────────

fn synthesize_workspace_with_call(
    pkg_name: &str,
    file_path: &str,
    module_path: &str,
    function_symbol: &str,
    callee: &str,
) -> AirWorkspace {
    use locus_air::{AirCallSite, CallKind};
    let span = AirSpan::new(file_path, 1, 1);
    AirWorkspace {
        schema_version: locus_air::AIR_SCHEMA_VERSION,
        packages: vec![AirPackage {
            name: pkg_name.into(),
            version: "0".into(),
            root_dir: "/".into(),
            files: vec![AirFile {
                path: file_path.into(),
                module_path: Some(module_path.into()),
                items: vec![
                    AirItem::Function(AirFunction {
                        name: "caller".into(),
                        symbol: function_symbol.into(),
                        visibility: Visibility::Public,
                        params: vec![],
                        return_type: None,
                        span: span.clone(),
                        line_count: 1,
                        decorators: vec![],
                        symbol_segments: vec![],
                        doc: None,
                    }),
                    AirItem::CallSite(AirCallSite {
                        callee: callee.into(),
                        kind: CallKind::Function,
                        function: Some(function_symbol.into()),
                        span,
                    }),
                ],
                hints: vec![],
                parse_error: None,
                line_count: 1,
            }],
        }],
        facts: vec![],
    }
}

fn synthesize_workspace_with_method_call(
    pkg_name: &str,
    file_path: &str,
    module_path: &str,
    function_symbol: &str,
    method: &str,
) -> AirWorkspace {
    use locus_air::{AirCallSite, CallKind};
    let span = AirSpan::new(file_path, 1, 1);
    AirWorkspace {
        schema_version: locus_air::AIR_SCHEMA_VERSION,
        packages: vec![AirPackage {
            name: pkg_name.into(),
            version: "0".into(),
            root_dir: "/".into(),
            files: vec![AirFile {
                path: file_path.into(),
                module_path: Some(module_path.into()),
                items: vec![
                    AirItem::Function(AirFunction {
                        name: "caller".into(),
                        symbol: function_symbol.into(),
                        visibility: Visibility::Public,
                        params: vec![],
                        return_type: None,
                        span: span.clone(),
                        line_count: 1,
                        decorators: vec![],
                        symbol_segments: vec![],
                        doc: None,
                    }),
                    AirItem::CallSite(AirCallSite {
                        callee: method.into(),
                        kind: CallKind::Method,
                        function: Some(function_symbol.into()),
                        span,
                    }),
                ],
                hints: vec![],
                parse_error: None,
                line_count: 1,
            }],
        }],
        facts: vec![],
    }
}
