use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root should exist")
}

fn parse_first_u32_on_line_with(haystack: &str, needle: &str) -> Option<u32> {
    let line = haystack.lines().find(|line| line.contains(needle))?;
    let digits: String = line
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

fn parse_second_u32_on_line_with(haystack: &str, needle: &str) -> Option<u32> {
    let line = haystack.lines().find(|line| line.contains(needle))?;
    let nums: Vec<u32> = line
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse::<u32>().ok())
        .collect();
    nums.get(1).copied()
}

fn collect_rule_ids_from_rules_files(root: &Path) -> BTreeSet<String> {
    let paradigms_dir = root.join("crates/locus-core/src/paradigms");
    let mut ids = BTreeSet::new();

    for entry in fs::read_dir(&paradigms_dir).expect("paradigms dir should be readable") {
        let entry = entry.expect("dir entry should be readable");
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        // Collect all files to scan. Rules may live in:
        //   - paradigm/rules.rs                 (legacy flat layout)
        //   - paradigm/rules/mod.rs             (promoted directory, CX001 P2)
        //   - paradigm/rules/<id>.rs            (per-rule split, OT + CX001 P2)
        let mut files_to_scan: Vec<PathBuf> = Vec::new();
        let flat_rules = path.join("rules.rs");
        if flat_rules.exists() {
            files_to_scan.push(flat_rules);
        }
        let rules_subdir = path.join("rules");
        if rules_subdir.is_dir()
            && let Ok(sub_entries) = fs::read_dir(&rules_subdir)
        {
            for sub_entry in sub_entries.flatten() {
                let sub_path = sub_entry.path();
                if sub_path.extension().and_then(|e| e.to_str()) == Some("rs") {
                    files_to_scan.push(sub_path);
                }
            }
        }

        if files_to_scan.is_empty() {
            continue;
        }

        for file_path in &files_to_scan {
            let content = fs::read_to_string(file_path).expect("rules file should be readable");
            for line in content.lines() {
                // Two patterns:
                //   `rule_id: "XX###"` — legacy Diagnostic construction
                //   `RuleId::new("XX###")` — new RuleDefinition impls (post P2 #71)
                for marker in [r#"rule_id: ""#, r#"RuleId::new(""#] {
                    if let Some(start) = line.find(marker) {
                        let suffix = &line[start + marker.len()..];
                        if let Some(end) = suffix.find('"') {
                            let candidate = &suffix[..end];
                            if candidate.len() == 5
                                && candidate[..2].chars().all(|c| c.is_ascii_uppercase())
                                && candidate[2..].chars().all(|c| c.is_ascii_digit())
                            {
                                ids.insert(candidate.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    ids
}

#[test]
fn docs_snapshot_counts_match_registry_and_rule_set() {
    let root = repo_root();
    let agents = fs::read_to_string(root.join("AGENTS.md")).expect("AGENTS.md should exist");
    let paradigms_doc =
        fs::read_to_string(root.join("docs/PARADIGMS.md")).expect("PARADIGMS.md should exist");

    let actual_paradigm_count = locus_core::paradigms::registry().len() as u32;
    let actual_rule_count = collect_rule_ids_from_rules_files(&root).len() as u32;

    let agents_paradigms = parse_first_u32_on_line_with(&agents, "paradigms registered")
        .expect("AGENTS.md should contain paradigm count snapshot");
    let agents_rules = parse_second_u32_on_line_with(&agents, "paradigms registered")
        .expect("AGENTS.md should contain rule count snapshot");

    assert_eq!(
        agents_paradigms, actual_paradigm_count,
        "AGENTS.md paradigm count drift: docs says {agents_paradigms}, code has {actual_paradigm_count}"
    );
    assert_eq!(
        agents_rules, actual_rule_count,
        "AGENTS.md rule count drift: docs says {agents_rules}, code has {actual_rule_count}"
    );

    assert!(
        paradigms_doc.contains("## Implementation status (snapshot)"),
        "PARADIGMS.md should keep an implementation snapshot section"
    );
}
