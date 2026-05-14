# Agent/Locus Comparison Harness

Use `scripts/compare-agent-with-locus.py` to run the same coding task against
two isolated copies of a corpus:

- `without_locus` initializes Locus only for hidden measurement. The agent prompt
  explicitly tells the agent not to run or read Locus.
- `with_locus` initializes Locus, writes the normal agent instructions, and tells
  the agent to use `locus check` before and after the edit.

Both arms are measured with `locus check --format json`. The final changed-code
gate uses:

```bash
locus check --workspace . --changed --baseline locus-harness-baseline --agent-strict --format json
```

The harness copies the corpus into `target/agent-locus-harness/<timestamp>/`.
When the corpus is a git checkout, it copies the source snapshot reported by
`git ls-files --cached --others --exclude-standard`; ignored caches are skipped
by construction. It also excludes common VCS/agent/dependency/build/cache
directories by default:
`.git`, `.agents`, `.claude`, `.codex`, `target`, `node_modules`, `dist`,
`build`, `.next`, `.svelte-kit`, `.turbo`, `.vite`, `.cache`, `.ruff_cache`,
`.pytest_cache`, `.mypy_cache`, `.venv`, `venv`, `__pycache__`, `coverage`,
and `mutants.out`. It creates a fresh git baseline commit and tags it as
`locus-harness-baseline`. The original corpus is never mutated.

## Prepare Worktrees Only

```bash
LOCUS_TEST_CORPUS=/mnt/code/projects/sides/lors \
scripts/compare-agent-with-locus.py \
  --task "Add the requested feature here"
```

This writes prompts and worktrees but does not invoke an agent. Use it to inspect
the exact setup before running an expensive agent command. The harness builds the
current `locus` binary by default; pass `--skip-build` only when you know
`target/debug/locus` is already current.

## Run An Agent

Pass any shell command template with placeholders. The command runs with its
current directory set to that arm's workspace.

```bash
LOCUS_TEST_CORPUS=/mnt/code/projects/sides/lors \
scripts/compare-agent-with-locus.py \
  --task-file /tmp/locus-agent-task.md \
  --agent-command 'your-agent --workspace {workspace} --prompt-file {prompt_file}'
```

Available placeholders:

- `{workspace}`: the copied corpus workspace for this arm.
- `{prompt_file}`: the generated prompt for this arm.
- `{arm}`: `without_locus` or `with_locus`.
- `{run_dir}`: the harness artifact directory.
- `{locus_bin}`: the Locus binary path used for setup and measurement.

The same values are also exposed through environment variables:
`LOCUS_HARNESS_WORKSPACE`, `LOCUS_HARNESS_PROMPT_FILE`,
`LOCUS_HARNESS_ARM`, `LOCUS_BIN`, and `LOCUS_HARNESS_BASELINE`.

For CLIs that read a prompt from stdin, use shell redirection:

```bash
--agent-command 'your-agent < {prompt_file}'
```

## Artifacts

Each run writes:

- `summary.md`: compact comparison table plus first new findings per arm.
- `summary.json`: machine-readable run metadata and diagnostic counts.
- `<arm>-prompt.md`: exact prompt given to that arm.
- `<arm>-baseline-full.json`: full-workspace baseline Locus report.
- `<arm>-after-full.json`: full-workspace post-agent Locus report.
- `<arm>-after-changed-strict.json`: changed-code strict report.
- `<arm>-git-diff.json`: captured `git diff locus-harness-baseline`.
- `<arm>-agent.json`: agent command, exit code, stdout, stderr, and duration.

Use changed-strict counts to compare agent-introduced architectural drift. Use
full-workspace new findings when the task moves code enough that existing debt
changes line numbers or ownership context.
