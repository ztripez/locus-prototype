use std::path::PathBuf;

use anyhow::{Context, Result};
use locus_core::{Lockfile, today_utc};

// locus: ot boundary cli.prune cli
#[derive(clap::Args, Debug)]
pub struct PruneArgs {
    /// Workspace root (containing Cargo.toml).
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
}

pub fn run(args: PruneArgs) -> Result<()> {
    let mut lockfile = Lockfile::load_or_empty(&args.workspace)
        .with_context(|| format!("load lockfile from {}", args.workspace.display()))?;
    let today = today_utc();
    let removed = prune_expired(&mut lockfile, &today);
    let written = lockfile
        .save(&args.workspace)
        .with_context(|| format!("write lockfile to {}", args.workspace.display()))?;
    println!("removed {removed} expired lockfile exception(s)");
    println!("updated {}", written.display());
    Ok(())
}

pub fn prune_expired(lockfile: &mut Lockfile, today: &str) -> usize {
    let before = lockfile.exceptions.len();
    lockfile
        .exceptions
        .retain(|ex| ex.expires.as_str() >= today);
    before.saturating_sub(lockfile.exceptions.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use locus_core::lockfile::Exception;

    #[test]
    fn prune_removes_only_expired_lockfile_exceptions() {
        let mut lockfile = Lockfile::empty();
        lockfile.exceptions = vec![
            Exception {
                rule: "OT004".to_string(),
                target: "src/lib.rs:1".to_string(),
                reason: "temporary".to_string(),
                expires: "2026-01-01".to_string(),
            },
            Exception {
                rule: "DG003".to_string(),
                target: "src/lib.rs:1".to_string(),
                reason: "temporary".to_string(),
                expires: "2026-12-31".to_string(),
            },
        ];

        let removed = prune_expired(&mut lockfile, "2026-05-09");
        assert_eq!(removed, 1);
        assert_eq!(lockfile.exceptions.len(), 1);
        assert_eq!(lockfile.exceptions[0].rule, "DG003");
    }
}
