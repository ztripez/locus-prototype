//! Diff-aware mode helpers. Wraps `git` invocations so `locus check
//! --changed` can filter diagnostics to files modified since some
//! baseline ref. Independent of the paradigm tier — diagnostics flow
//! through the full check pipeline first, then this module's filter
//! drops anything outside the changed-files set.
//!
//! Baseline resolution falls back through a chain so the default
//! `--changed` flow Just Works in typical CI shapes:
//!   `origin/main` → `origin/master` → `main` → `master` → `HEAD~1`
//!
//! Untracked-but-not-ignored files (`git ls-files --others
//! --exclude-standard`) are included — they're the most common
//! shape of "new code added in a PR" before the first commit.
//!
//! Modified-but-uncommitted files (`git diff --name-only HEAD`) are
//! also included so local development flows match CI behaviour.

// locus: ot canonical

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DiffError {
    #[error("workspace `{path}` is not a git repository (or git is not installed)")]
    NotARepository { path: PathBuf },
    #[error(
        "could not resolve a default baseline ref. Tried `origin/main`, `origin/master`, \
         `main`, `master`, `HEAD~1` — none exist in this repo. Pass `--baseline <ref>` \
         explicitly."
    )]
    NoBaseline,
    #[error("git command `git {args}` failed: {stderr}")]
    GitFailed { args: String, stderr: String },
    #[error("io error invoking git: {source}")]
    Io {
        #[source]
        source: std::io::Error,
    },
}

/// Set of files (workspace-relative paths) that have changed since
/// `baseline`. Combines three git queries:
///
/// - tracked files modified between `baseline` and `HEAD`
/// - tracked files modified between `HEAD` and the working tree
/// - untracked-but-not-ignored files in the working tree
///
/// The returned paths are workspace-relative — the caller is expected
/// to compare against diagnostic spans whose `file` field is whatever
/// the language adapter recorded (typically a workspace-anchored path
/// or absolute). [`paths_match`] handles the matching tolerantly.
pub fn changed_files(
    workspace: &Path,
    baseline: Option<&str>,
) -> Result<HashSet<PathBuf>, DiffError> {
    if !is_git_repo(workspace)? {
        return Err(DiffError::NotARepository {
            path: workspace.to_path_buf(),
        });
    }
    let baseline = match baseline {
        Some(b) => b.to_string(),
        None => resolve_default_baseline(workspace)?,
    };

    let mut out: HashSet<PathBuf> = HashSet::new();

    // Files changed between baseline and HEAD.
    let committed = run_git(
        &["diff", "--name-only", "--no-renames", &baseline, "HEAD"],
        workspace,
    )?;
    extend_from_lines(&mut out, &committed);

    // Files changed between HEAD and the working tree (staged + unstaged).
    let working = run_git(&["diff", "--name-only", "--no-renames", "HEAD"], workspace)?;
    extend_from_lines(&mut out, &working);

    // Untracked-but-not-ignored files (new files in a PR before commit).
    let untracked = run_git(&["ls-files", "--others", "--exclude-standard"], workspace)?;
    extend_from_lines(&mut out, &untracked);

    Ok(out)
}

fn extend_from_lines(out: &mut HashSet<PathBuf>, raw: &str) {
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        out.insert(PathBuf::from(trimmed));
    }
}

fn is_git_repo(workspace: &Path) -> Result<bool, DiffError> {
    match Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(workspace)
        .output()
    {
        Ok(out) => Ok(out.status.success()),
        Err(source) => Err(DiffError::Io { source }),
    }
}

fn resolve_default_baseline(workspace: &Path) -> Result<String, DiffError> {
    const CANDIDATES: &[&str] = &["origin/main", "origin/master", "main", "master", "HEAD~1"];
    for candidate in CANDIDATES {
        if ref_exists(workspace, candidate)? {
            return Ok((*candidate).to_string());
        }
    }
    Err(DiffError::NoBaseline)
}

fn ref_exists(workspace: &Path, refname: &str) -> Result<bool, DiffError> {
    let out = Command::new("git")
        .args(["rev-parse", "--verify", "--quiet", refname])
        .current_dir(workspace)
        .output()
        .map_err(|source| DiffError::Io { source })?;
    Ok(out.status.success())
}

fn run_git(args: &[&str], workspace: &Path) -> Result<String, DiffError> {
    let out = Command::new("git")
        .args(args)
        .current_dir(workspace)
        .output()
        .map_err(|source| DiffError::Io { source })?;
    if !out.status.success() {
        return Err(DiffError::GitFailed {
            args: args.join(" "),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Read the baseline `locus.lock` via `git show <baseline>:locus.lock`.
/// Returns `None` (silently) when:
/// - the workspace isn't a git repo
/// - no baseline ref resolves
/// - the baseline ref doesn't carry a `locus.lock` (e.g., first commit
///   before the lockfile existed)
/// - the file at the baseline ref fails to parse as a `Lockfile`
///
/// This silent-skip behaviour is what keeps Policy Guard from firing
/// on first-onboarding repos: with no baseline, there's nothing to
/// compare against and no false alarms.
pub fn read_baseline_lockfile(
    workspace: &Path,
    baseline: Option<&str>,
) -> Option<locus_core::Lockfile> {
    if !is_git_repo(workspace).ok()? {
        return None;
    }
    let baseline = match baseline {
        Some(b) => b.to_string(),
        None => resolve_default_baseline(workspace).ok()?,
    };
    let arg = format!("{baseline}:{}", locus_core::LOCKFILE_NAME);
    let out = Command::new("git")
        .args(["show", &arg])
        .current_dir(workspace)
        .output()
        .ok()?;
    if !out.status.success() {
        // Common case: baseline predates the lockfile or the file isn't tracked.
        return None;
    }
    serde_json::from_slice(&out.stdout).ok()
}

/// Tolerant path match between a diagnostic span's `file` field and
/// the changed-files set. The visitor records spans with whatever path
/// shape it received — typically workspace-anchored absolute paths.
/// Git emits workspace-relative paths. We normalise both sides to
/// avoid a strict-equality mismatch causing false negatives.
///
/// Match rules (any one of these is enough):
/// - exact equality
/// - `span_file` ends with the changed file's relative path (covers
///   the common "absolute span vs. relative git output" case)
/// - the changed file's absolute form (`workspace.join(rel)`) equals
///   `span_file`
pub fn paths_match(span_file: &str, changed_rel: &Path, workspace: &Path) -> bool {
    if span_file == changed_rel.to_string_lossy() {
        return true;
    }
    let abs = workspace.join(changed_rel);
    if span_file == abs.to_string_lossy() {
        return true;
    }
    // Suffix match — handle e.g. `/abs/path/to/workspace/src/foo.rs`
    // span_file vs. `src/foo.rs` changed_rel.
    let rel_str = changed_rel.to_string_lossy();
    if !rel_str.is_empty() && span_file.ends_with(rel_str.as_ref()) {
        // Confirm the boundary is a path separator so `foo/src/bar.rs`
        // doesn't accidentally match `r/bar.rs`.
        let prefix_len = span_file.len() - rel_str.len();
        if prefix_len == 0 {
            return true;
        }
        let prev = span_file.as_bytes()[prefix_len - 1];
        if prev == b'/' || prev == b'\\' {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_match_exact_relative() {
        let ws = Path::new("/abs/workspace");
        assert!(paths_match("src/foo.rs", Path::new("src/foo.rs"), ws));
    }

    #[test]
    fn paths_match_absolute_span_against_relative_changed() {
        let ws = Path::new("/abs/workspace");
        assert!(paths_match(
            "/abs/workspace/src/foo.rs",
            Path::new("src/foo.rs"),
            ws,
        ));
    }

    #[test]
    fn paths_match_via_suffix_when_workspace_prefix_unknown() {
        let ws = Path::new("/different/path");
        // Even when the workspace prefix doesn't line up, a suffix
        // match with a path-separator boundary still works.
        assert!(paths_match(
            "/some/other/abs/workspace/src/foo.rs",
            Path::new("src/foo.rs"),
            ws,
        ));
    }

    #[test]
    fn paths_match_rejects_non_separator_suffix_collision() {
        let ws = Path::new("/abs/workspace");
        // span_file ends with `r.rs` but the boundary isn't a `/`.
        assert!(!paths_match("src/bar.rs", Path::new("r.rs"), ws,));
    }

    #[test]
    fn paths_match_distinct_files() {
        let ws = Path::new("/abs/workspace");
        assert!(!paths_match("src/foo.rs", Path::new("src/bar.rs"), ws,));
    }

    #[test]
    fn extend_from_lines_skips_blank_lines_and_trims() {
        let mut out = HashSet::new();
        extend_from_lines(&mut out, "src/foo.rs\n\n  src/bar.rs\nsrc/baz.rs\n   \n");
        assert_eq!(out.len(), 3);
        assert!(out.contains(&PathBuf::from("src/foo.rs")));
        assert!(out.contains(&PathBuf::from("src/bar.rs")));
        assert!(out.contains(&PathBuf::from("src/baz.rs")));
    }
}
