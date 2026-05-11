use std::path::PathBuf;

use anyhow::{Context, Result};

// locus: ot boundary cli.explain cli
#[derive(clap::Args, Debug)]
pub struct ExplainArgs {
    /// Rule id to explain, e.g. `OT004`.
    pub rule_id: String,
    /// Workspace root (containing docs/PARADIGMS.md).
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
}

pub fn run(args: ExplainArgs) -> Result<()> {
    let docs_path = args.workspace.join("docs").join("PARADIGMS.md");
    let body = std::fs::read_to_string(&docs_path)
        .with_context(|| format!("read {}", docs_path.display()))?;
    let Some(section) = extract_rule_section(&body, &args.rule_id) else {
        anyhow::bail!(
            "rule `{}` not found in {}",
            args.rule_id,
            docs_path.display()
        );
    };
    println!("{section}");
    Ok(())
}

pub fn extract_rule_section(markdown: &str, rule_id: &str) -> Option<String> {
    let needle = format!("#### {rule_id} ");
    let lines: Vec<&str> = markdown.lines().collect();
    let start = lines
        .iter()
        .position(|line| line.trim_start().starts_with(&needle))?;
    let end = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find_map(|(idx, line)| line.trim_start().starts_with("#### ").then_some(idx))
        .unwrap_or(lines.len());
    Some(lines[start..end].join("\n").trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_rule_section_returns_exact_heading_block() {
        let md = r#"
## X
#### OT004 — Name
line a
line b

#### DG001 — Next
line c
"#;
        let got = extract_rule_section(md, "OT004").expect("section exists");
        assert!(got.starts_with("#### OT004 — Name"));
        assert!(got.contains("line a"));
        assert!(got.contains("line b"));
        assert!(!got.contains("DG001"));
    }
}
