//! `locus query <kind>` — oracle command over AIR items and facts.
//!
//! Answers "where does Locus see <kind> in this workspace?" Reuses the
//! existing AIR scan path; does NOT load the lockfile or run governance.
//! This is the architectural lookup surface, distinct from `check`
//! (rules + gating) and `observe` (advisory survey).
//!
//! Kinds are kebab-case architectural names — not raw enum variants —
//! per #24's "do not expose internal enum names as the public interface."

// locus: ot boundary cli.query cli

use std::collections::BTreeSet;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use locus_air::{
    AirFact, AirFile, AirHint, AirItem, AirSpan, AirWorkspace, FactKind, FactTarget, HintKind,
};
use serde::Serialize;

// locus: ot boundary cli.query cli
#[derive(clap::Args, Debug)]
pub struct QueryArgs {
    /// What to query for (e.g. `canonical`, `converter`, `hot-path`).
    /// Run `locus query <unknown>` to see the full kind list.
    pub kind: String,
    /// Workspace root (default: current directory).
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
    /// Emit JSON instead of human-readable rows.
    #[arg(long)]
    pub json: bool,
}

/// Supported query kinds. Keep in lockstep with `fact_kind_for_kebab` and the
/// hint-kind branch in `run`.
const SUPPORTED_KINDS: &[&str] = &[
    "canonical",
    "boundary",
    "converter",
    "spawned-work",
    "config-read",
    "logging",
    "external-io",
    "persistence-write",
    "blocking-call",
    "hot-path",
    "request-context",
    "boundary-entry",
    "runtime-state-owner",
    "background-worker",
];

/// One emitted row of `locus query` output. Not architecturally a
/// canonical type — it's CLI-local serialization shape sitting on the
/// `cli.query` boundary alongside `QueryArgs`.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
struct Row {
    kind: String,
    symbol: String,
    path: String,
    line: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    evidence: Option<String>,
}

pub fn run(args: QueryArgs) -> Result<()> {
    let air = locus_rust::scan(&args.workspace)
        .with_context(|| format!("scan failed: {}", args.workspace.display()))?;

    let workspace_root = args.workspace.as_path();
    let rows = match args.kind.as_str() {
        "canonical" | "boundary" => query_hint(&air, &args.kind, workspace_root),
        "converter" => query_converter(&air, workspace_root),
        kebab => match fact_kind_for_kebab(kebab) {
            Some(fk) => query_fact(&air, fk, &args.kind, workspace_root),
            None => {
                print_unknown_kind(&args.kind)?;
                std::process::exit(2);
            }
        },
    };

    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    if args.json {
        serde_json::to_writer_pretty(&mut out, &rows)?;
        writeln!(out)?;
    } else {
        write_human(&mut out, &args.kind, &rows)?;
    }
    out.flush()?;
    Ok(())
}

// ---------------------------------------------------------------------
// Hint-derived queries: canonical, boundary
// ---------------------------------------------------------------------

fn query_hint(air: &AirWorkspace, kind: &str, workspace_root: &Path) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    for pkg in &air.packages {
        for file in &pkg.files {
            for hint in &file.hints {
                if !hint_matches(kind, &hint.kind) {
                    continue;
                }
                let (symbol, line) = resolve_hint_target(file, hint);
                rows.push(Row {
                    kind: kind.to_string(),
                    symbol,
                    path: relative_path(workspace_root, &file.path),
                    line,
                    source: None,
                    evidence: None,
                });
            }
        }
    }
    sort_rows(&mut rows);
    rows
}

fn hint_matches(kind: &str, hint_kind: &HintKind) -> bool {
    match kind {
        "canonical" => matches!(hint_kind, HintKind::Canonical),
        "boundary" => matches!(hint_kind, HintKind::Boundary { .. }),
        "converter" => matches!(hint_kind, HintKind::Converter),
        _ => false,
    }
}

/// Resolve a hint to the symbol it decorates. Hints carry a `target_span`
/// pointing at the next syntactic binding below them. We look that span
/// up in `file.items` to recover the symbol. When the target doesn't land
/// on a tracked item (module-level `// locus: ot canonical` above
/// `use` statements, for example), fall back to the file's module path.
fn resolve_hint_target(file: &AirFile, hint: &AirHint) -> (String, u32) {
    if let Some(target) = hint.target_span.as_ref() {
        if let Some(symbol) = item_symbol_at(file, target) {
            return (symbol, target.line_start);
        }
        return (module_fallback_symbol(file), target.line_start);
    }
    (module_fallback_symbol(file), hint.span.line_start)
}

/// Symbol to use when a hint doesn't bind to a tracked AIR item. Prefers
/// the file's module path; falls back to the raw file path so the row
/// is never empty.
fn module_fallback_symbol(file: &AirFile) -> String {
    file.module_path
        .clone()
        .unwrap_or_else(|| file.path.clone())
}

fn item_symbol_at(file: &AirFile, target: &AirSpan) -> Option<String> {
    for item in &file.items {
        let (item_span, symbol) = match item {
            AirItem::Type(t) => (&t.span, Some(t.symbol.clone())),
            AirItem::Function(f) => (&f.span, Some(f.symbol.clone())),
            AirItem::Conversion(c) => (&c.span, Some(c.symbol.clone())),
            AirItem::Impl(i) => (&i.span, Some(i.target_type.clone())),
            _ => continue,
        };
        if span_contains(item_span, target) {
            return symbol;
        }
    }
    None
}

fn span_contains(outer: &AirSpan, inner: &AirSpan) -> bool {
    inner.line_start >= outer.line_start && inner.line_end <= outer.line_end
}

// ---------------------------------------------------------------------
// Converter — hint-derived OR AirItem::Conversion (union + dedupe)
// ---------------------------------------------------------------------

fn query_converter(air: &AirWorkspace, workspace_root: &Path) -> Vec<Row> {
    let mut seen: BTreeSet<(String, String, u32)> = BTreeSet::new();
    let mut rows: Vec<Row> = Vec::new();
    let hint_rows = query_hint(air, "converter", workspace_root);
    for row in hint_rows {
        if seen.insert((row.symbol.clone(), row.path.clone(), row.line)) {
            rows.push(row);
        }
    }
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                let AirItem::Conversion(c) = item else {
                    continue;
                };
                let path = relative_path(workspace_root, &file.path);
                let key = (c.symbol.clone(), path.clone(), c.span.line_start);
                if seen.insert(key) {
                    rows.push(Row {
                        kind: "converter".to_string(),
                        symbol: c.symbol.clone(),
                        path,
                        line: c.span.line_start,
                        source: None,
                        evidence: None,
                    });
                }
            }
        }
    }
    sort_rows(&mut rows);
    rows
}

// ---------------------------------------------------------------------
// Fact-derived queries
// ---------------------------------------------------------------------

fn query_fact(air: &AirWorkspace, fk: FactKind, kind: &str, workspace_root: &Path) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    for fact in &air.facts {
        if fact.kind != fk {
            continue;
        }
        let (symbol, path, line) = locate_fact(air, fact, workspace_root);
        rows.push(Row {
            kind: kind.to_string(),
            symbol,
            path,
            line,
            source: Some(fact.source.clone()),
            evidence: fact.evidence.clone(),
        });
    }
    sort_rows(&mut rows);
    rows
}

/// Resolve an `AirFact` to `(symbol, workspace-relative path, line)`.
/// `AirFact` doesn't carry its own span; we derive location from the
/// target. For `Function` targets, we look up the function's recorded
/// span. For `File`/`Span`, the path is in the target itself.
fn locate_fact(air: &AirWorkspace, fact: &AirFact, workspace_root: &Path) -> (String, String, u32) {
    match &fact.target {
        FactTarget::Function { symbol } => {
            if let Some((path, line)) = lookup_function_span(air, symbol) {
                return (symbol.clone(), relative_path(workspace_root, &path), line);
            }
            // Symbol recorded but not found in current AIR — surface the
            // symbol anyway so the row isn't silently dropped.
            (symbol.clone(), String::new(), 1)
        }
        FactTarget::File { path } => (path.clone(), relative_path(workspace_root, path), 1),
        FactTarget::Span(span) => (
            span.file.clone(),
            relative_path(workspace_root, &span.file),
            span.line_start,
        ),
    }
}

fn lookup_function_span(air: &AirWorkspace, symbol: &str) -> Option<(String, u32)> {
    for pkg in &air.packages {
        for file in &pkg.files {
            for item in &file.items {
                if let AirItem::Function(f) = item
                    && f.symbol == symbol
                {
                    return Some((file.path.clone(), f.span.line_start));
                }
            }
        }
    }
    None
}

fn fact_kind_for_kebab(kebab: &str) -> Option<FactKind> {
    match kebab {
        "spawned-work" => Some(FactKind::SpawnedWork),
        "config-read" => Some(FactKind::ConfigRead),
        "logging" => Some(FactKind::Logging),
        "external-io" => Some(FactKind::ExternalIo),
        "persistence-write" => Some(FactKind::PersistenceWrite),
        "blocking-call" => Some(FactKind::BlockingCall),
        "hot-path" => Some(FactKind::HotPath),
        "request-context" => Some(FactKind::RequestContext),
        "boundary-entry" => Some(FactKind::BoundaryEntry),
        "runtime-state-owner" => Some(FactKind::RuntimeStateOwner),
        "background-worker" => Some(FactKind::BackgroundWorker),
        _ => None,
    }
}

// ---------------------------------------------------------------------
// Output: human / unknown-kind / shared helpers
// ---------------------------------------------------------------------

fn write_human<W: Write>(out: &mut W, kind: &str, rows: &[Row]) -> io::Result<()> {
    if rows.is_empty() {
        writeln!(out, "{kind} (no matches detected)")?;
        return Ok(());
    }
    writeln!(out, "{} ({} matches)", kind, rows.len())?;
    let width = rows.iter().map(|r| r.symbol.len()).max().unwrap_or(0);
    for row in rows {
        writeln!(
            out,
            "  {symbol:<width$}  {path}:{line}",
            symbol = row.symbol,
            width = width,
            path = row.path,
            line = row.line,
        )?;
    }
    Ok(())
}

fn print_unknown_kind(input: &str) -> io::Result<()> {
    let stderr = io::stderr();
    let mut err = BufWriter::new(stderr.lock());
    writeln!(err, "unknown query kind: `{input}`")?;
    writeln!(err, "supported kinds:")?;
    // Group across four lines for readability, matching the docstring sample.
    let groups: &[&[&str]] = &[
        &SUPPORTED_KINDS[0..3],   // canonical, boundary, converter
        &SUPPORTED_KINDS[3..8],   // spawned-work .. persistence-write
        &SUPPORTED_KINDS[8..12],  // blocking-call .. boundary-entry
        &SUPPORTED_KINDS[12..14], // runtime-state-owner, background-worker
    ];
    for (i, group) in groups.iter().enumerate() {
        let is_last = i + 1 == groups.len();
        let suffix = if is_last { "" } else { "," };
        writeln!(err, "  {}{}", group.join(", "), suffix)?;
    }
    err.flush()?;
    Ok(())
}

fn sort_rows(rows: &mut [Row]) {
    rows.sort_by(|a, b| a.symbol.cmp(&b.symbol).then_with(|| a.path.cmp(&b.path)));
}

/// Render `path` relative to `workspace_root`. Falls back to the
/// original path string when stripping fails (path outside workspace).
fn relative_path(workspace_root: &Path, path: &str) -> String {
    let candidate = Path::new(path);
    // Canonicalize the workspace root to handle "." and relative inputs.
    let root = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    if let Ok(rel) = candidate.strip_prefix(&root) {
        return rel.to_string_lossy().into_owned();
    }
    if let Ok(rel) = candidate.strip_prefix(workspace_root) {
        return rel.to_string_lossy().into_owned();
    }
    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_kinds_matches_fact_kind_for_kebab_and_hints() {
        let hint_kinds: &[&str] = &["canonical", "boundary", "converter"];
        for k in SUPPORTED_KINDS {
            if hint_kinds.contains(k) {
                continue;
            }
            assert!(
                fact_kind_for_kebab(k).is_some(),
                "supported kind `{k}` has no fact_kind_for_kebab mapping"
            );
        }
    }
}
