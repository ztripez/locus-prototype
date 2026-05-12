use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use locus_core::Lockfile;
use locus_core::{AcknowledgedEmptyEntry, registry};

const AGENTS_FILE: &str = "AGENTS.md";
const CLAUDE_FILE: &str = "CLAUDE.md";
const LOCUS_BLOCK_START: &str = "<!-- locus:init-snippet:start -->";
const LOCUS_BLOCK_END: &str = "<!-- locus:init-snippet:end -->";

const LOCUS_AGENT_SNIPPET: &str = "<!-- locus:init-snippet:start -->\n\
## Locus\n\
This repo uses Locus for architecture governance.\n\
Before editing code, run `locus check`; treat findings as architecture feedback, not lint noise.\n\
Do not bypass, delete, or weaken Locus rules or policies unless explicitly asked.\n\
Do not add broad exemptions; if one is needed, make it narrow and explain why.\n\
Prefer declared owners/modules over duplicating logic.\n\
When unsure where code belongs, use Locus output and nearby owners as guidance.\n\
Before finishing, run `locus check` again.\n\
If findings remain, explain why or list the required follow-up.\n\
<!-- locus:init-snippet:end -->\n";

// locus: ot boundary cli.init cli
#[derive(clap::Args, Debug)]
pub struct InitArgs {
    /// Workspace root (containing Cargo.toml).
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
    /// Refuse to overwrite an existing .locus/lock.json.
    #[arg(long)]
    pub no_overwrite: bool,
    /// Do not write or update the managed Locus block in AGENTS.md / CLAUDE.md.
    #[arg(long)]
    pub no_agent_instructions: bool,
    /// Comma-separated paradigm prefixes the user explicitly acknowledges
    /// as empty. Each prefix is appended to `Lockfile.acknowledged_empty`
    /// (silencing LOCUS002 for that paradigm). Already-present prefixes
    /// are silently deduped. Example: `--acknowledge-empty RW,DA`.
    #[arg(long, value_name = "PREFIXES")]
    pub acknowledge_empty: Option<String>,
}

pub fn run(args: InitArgs) -> Result<()> {
    use locus_core::lockfile::LOCKFILE_RELATIVE_PATH;

    let lockfile_path = args.workspace.join(LOCKFILE_RELATIVE_PATH);
    if args.no_overwrite && lockfile_path.exists() {
        anyhow::bail!(
            "{} already exists; rerun without --no-overwrite to replace it",
            lockfile_path.display()
        );
    }

    let air = locus_rust::scan(&args.workspace)
        .with_context(|| format!("scan failed: {}", args.workspace.display()))?;
    let registry = registry();

    // Load existing lockfile so previously-acknowledged prefixes and accepted
    // decisions survive a re-run, then refresh paradigm sections.
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    populate_lockfile_sections(
        &mut lockfile,
        &registry,
        &air,
        args.acknowledge_empty.as_deref(),
    );

    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;

    println!("wrote {}", written.display());
    if !args.no_agent_instructions {
        let agent_file = upsert_agent_instructions(&args.workspace)
            .with_context(|| format!("write Locus agent instructions under {}", args.workspace.display()))?;
        println!("updated {}", agent_file.display());
    }
    print_init_sections_summary(&registry, &lockfile);

    let suggestions = collect_init_suggestions(&registry, &air, &lockfile);
    let hints_promoted = count_hint_promotions(&lockfile);
    print!("{}", render_checklist(&suggestions, hints_promoted));

    if !suggestions.is_empty() {
        // Flush before exit; process::exit skips destructors, so a buffered
        // stdout under pipe/redirect would otherwise drop the checklist.
        let _ = io::stdout().lock().flush();
        std::process::exit(1);
    }
    Ok(())
}

fn populate_lockfile_sections(
    lockfile: &mut Lockfile,
    registry: &[Box<dyn locus_core::Paradigm>],
    air: &locus_air::AirWorkspace,
    acknowledge_empty: Option<&str>,
) {
    // Re-run paradigm init to refresh sections from a fresh scan
    // (today only OT writes a non-empty section).
    for paradigm in registry {
        let section = paradigm.init(air);
        if !section_is_empty(&section) {
            lockfile
                .paradigms
                .insert(paradigm.rule_prefix().to_string(), section);
        }
    }
    if let Some(raw) = acknowledge_empty {
        for prefix in parse_prefix_list(raw) {
            if !lockfile
                .acknowledged_empty
                .iter()
                .any(|e| e.prefix() == prefix.as_str())
            {
                lockfile
                    .acknowledged_empty
                    .push(AcknowledgedEmptyEntry::Legacy(prefix));
            }
        }
    }
}

fn upsert_agent_instructions(workspace: &Path) -> Result<PathBuf> {
    let path = agent_instruction_path(workspace);
    let existing = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).with_context(|| format!("read {}", path.display())),
    };
    let updated = upsert_locus_agent_block(&existing);
    std::fs::write(&path, updated).with_context(|| format!("write {}", path.display()))?;
    Ok(path)
}

fn agent_instruction_path(workspace: &Path) -> PathBuf {
    let agents = workspace.join(AGENTS_FILE);
    if agents.exists() {
        return agents;
    }
    let claude = workspace.join(CLAUDE_FILE);
    if claude.exists() {
        return claude;
    }
    agents
}

fn upsert_locus_agent_block(existing: &str) -> String {
    if let Some(start) = existing.find(LOCUS_BLOCK_START) {
        if let Some(end_rel) = existing[start..].find(LOCUS_BLOCK_END) {
            let end = start + end_rel + LOCUS_BLOCK_END.len();
            let mut out = String::new();
            out.push_str(&existing[..start]);
            out.push_str(LOCUS_AGENT_SNIPPET.trim_end());
            out.push_str(&existing[end..]);
            if !out.ends_with('\n') {
                out.push('\n');
            }
            return out;
        }
    }

    let mut out = existing.trim_end().to_string();
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str(LOCUS_AGENT_SNIPPET);
    out
}

fn print_init_sections_summary(registry: &[Box<dyn locus_core::Paradigm>], lockfile: &Lockfile) {
    for paradigm in registry {
        let count = lockfile
            .paradigms
            .get(paradigm.rule_prefix())
            .map(summarize_section)
            .unwrap_or_else(|| "(empty)".to_string());
        println!(
            "  {} {}: {}",
            paradigm.rule_prefix(),
            paradigm.name(),
            count
        );
    }
}

fn collect_init_suggestions(
    registry: &[Box<dyn locus_core::Paradigm>],
    air: &locus_air::AirWorkspace,
    lockfile: &Lockfile,
) -> Vec<locus_core::init::Suggestion> {
    let mut suggestions: Vec<locus_core::init::Suggestion> = Vec::new();
    for paradigm in registry {
        suggestions.extend(paradigm.suggest(air, lockfile));
    }
    suggestions.extend(locus_core::init::cross_paradigm_suggestions(air, lockfile));
    let seeds = locus_core::init::default_vacancy_seeds();
    suggestions.extend(locus_core::init::vacancy_seeds(
        air,
        lockfile,
        seeds,
        &suggestions,
    ));
    locus_core::init::aggregate(suggestions)
}

fn count_hint_promotions(lockfile: &Lockfile) -> usize {
    use locus_core::paradigms::one_truth::lockfile_schema::{OtSection, Source};

    let section: OtSection = match lockfile.paradigm_section("OT") {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let mut count = 0usize;
    for entry in section.concepts.values() {
        if entry.canonical.source == Source::Hint {
            count += 1;
        }
        for b in &entry.boundaries {
            if b.source == Source::Hint {
                count += 1;
            }
        }
    }
    count
}

fn section_is_empty(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Null => true,
        serde_json::Value::Object(m) => m.is_empty() || m.values().all(section_is_empty),
        serde_json::Value::Array(a) => a.is_empty(),
        _ => false,
    }
}

fn summarize_section(v: &serde_json::Value) -> String {
    // Best-effort summary; specific paradigms can override later by exposing
    // their own renderer when there's enough variety to justify it.
    if let Some(concepts) = v.get("concepts").and_then(|c| c.as_object()) {
        let canonicals = concepts.len();
        let boundaries: usize = concepts
            .values()
            .filter_map(|c| c.get("boundaries").and_then(|b| b.as_array()))
            .map(|a| a.len())
            .sum();
        let converters: usize = concepts
            .values()
            .filter_map(|c| c.get("converters").and_then(|b| b.as_array()))
            .map(|a| a.len())
            .sum();
        return format!(
            "{canonicals} concept(s), {boundaries} boundary(ies), {converters} converter(s)"
        );
    }
    "section recorded".to_string()
}

pub fn render_checklist(
    suggestions: &[locus_core::init::Suggestion],
    hints_promoted: usize,
) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    let _ = writeln!(out, "auto-applied: {hints_promoted} source hints promoted");
    let _ = writeln!(out, "unresolved: {}", suggestions.len());
    if suggestions.is_empty() {
        return out;
    }
    for s in suggestions {
        out.push('\n');
        out.push_str(&s.render());
        out.push('\n');
    }
    out.push('\n');
    out.push_str("re-run `locus init` after applying changes.\n");
    out
}

pub fn parse_prefix_list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_uppercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use locus_core::lockfile::LOCKFILE_RELATIVE_PATH;

    #[test]
    fn parse_prefix_list_splits_and_uppercases() {
        assert_eq!(parse_prefix_list("rw,da"), vec!["RW", "DA"]);
    }

    #[test]
    fn parse_prefix_list_trims_whitespace_and_drops_empties() {
        assert_eq!(parse_prefix_list("  RW , , FO  "), vec!["RW", "FO"]);
    }

    #[test]
    fn parse_prefix_list_empty_input_returns_empty() {
        assert!(parse_prefix_list("").is_empty());
        assert!(parse_prefix_list(" , ").is_empty());
    }

    #[test]
    fn render_empty_checklist_says_zero_unresolved() {
        let suggestions: Vec<locus_core::init::Suggestion> = Vec::new();
        let out = render_checklist(&suggestions, /*hints_promoted=*/ 4);
        assert!(out.contains("auto-applied: 4 source hints promoted"));
        assert!(out.contains("unresolved: 0"));
        assert!(!out.contains("re-run"));
    }

    #[test]
    fn locus_agent_snippet_stays_under_twenty_lines() {
        assert!(
            LOCUS_AGENT_SNIPPET.lines().count() < 20,
            "managed agent snippet must stay compact"
        );
    }

    #[test]
    fn upsert_locus_agent_block_preserves_existing_content() {
        let existing = "# Agent guide\n\nKeep this intro.\n";
        let updated = upsert_locus_agent_block(existing);
        assert!(updated.contains("# Agent guide"));
        assert!(updated.contains("Keep this intro."));
        assert!(updated.contains("This repo uses Locus for architecture governance."));
        assert_eq!(updated.matches(LOCUS_BLOCK_START).count(), 1);
        assert_eq!(updated.matches(LOCUS_BLOCK_END).count(), 1);
    }

    #[test]
    fn upsert_locus_agent_block_is_idempotent() {
        let once = upsert_locus_agent_block("# Agent guide\n");
        let twice = upsert_locus_agent_block(&once);
        assert_eq!(once, twice);
        assert_eq!(twice.matches(LOCUS_BLOCK_START).count(), 1);
    }

    #[test]
    fn upsert_locus_agent_block_replaces_managed_block() {
        let existing = format!(
            "# Agent guide\n\n{LOCUS_BLOCK_START}\nold text\n{LOCUS_BLOCK_END}\n\nAfter.\n"
        );
        let updated = upsert_locus_agent_block(&existing);
        assert!(updated.contains("# Agent guide"));
        assert!(updated.contains("After."));
        assert!(!updated.contains("old text"));
        assert!(updated.contains("Treat findings as architecture feedback"));
        assert_eq!(updated.matches(LOCUS_BLOCK_START).count(), 1);
    }

    #[test]
    fn upsert_agent_instructions_prefers_existing_claude_when_agents_is_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(dir.join(CLAUDE_FILE), "# Claude\n").unwrap();
        let path = upsert_agent_instructions(dir).unwrap();
        assert_eq!(path.file_name().and_then(|s| s.to_str()), Some(CLAUDE_FILE));
        let text = std::fs::read_to_string(dir.join(CLAUDE_FILE)).unwrap();
        assert!(text.contains("This repo uses Locus for architecture governance."));
    }

    #[test]
    fn acknowledge_empty_persists_into_lockfile() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        // Minimal cargo workspace so `locus_rust::scan` succeeds.
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"x\"\nversion = \"0.0.1\"\nedition = \"2024\"\n[lib]\npath = \"src/lib.rs\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/lib.rs"), "").unwrap();

        // Ack every paradigm that emits a vacancy seed in `init`; otherwise
        // `init` calls `process::exit(1)` and aborts the test runner.
        let seed_prefixes: Vec<String> = locus_core::init::default_vacancy_seeds()
            .iter()
            .map(|(p, _, _)| (*p).to_string())
            .collect();
        let ack_input = seed_prefixes
            .iter()
            .map(|p| p.to_lowercase())
            .collect::<Vec<_>>()
            .join(",");

        let args = InitArgs {
            workspace: dir.to_path_buf(),
            no_overwrite: false,
            no_agent_instructions: false,
            acknowledge_empty: Some(ack_input),
        };
        run(args).unwrap();

        let lockfile_bytes = std::fs::read(dir.join(LOCKFILE_RELATIVE_PATH)).unwrap();
        let lf: Lockfile = serde_json::from_slice(&lockfile_bytes).unwrap();
        let actual_prefixes: Vec<String> = lf
            .acknowledged_empty
            .iter()
            .map(|e| e.prefix().to_string())
            .collect();
        assert_eq!(actual_prefixes, seed_prefixes);

        let agent_text = std::fs::read_to_string(dir.join(AGENTS_FILE)).unwrap();
        assert!(agent_text.contains("This repo uses Locus for architecture governance."));
    }
}
