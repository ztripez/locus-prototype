#!/usr/bin/env python3
"""Run an A/B agent experiment with and without Locus guidance.

The harness treats Locus as the measurement oracle in both arms, but only
surfaces Locus instructions to the `with_locus` arm. This lets us compare the
architectural drift introduced by the same task under two agent prompts.
"""

from __future__ import annotations

import argparse
import datetime as dt
import fnmatch
import json
import os
from pathlib import Path
import shutil
import shlex
import subprocess
import sys
import time
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_EXCLUDES = (
    ".git",
    ".agents",
    ".claude",
    ".codex",
    "target",
    "node_modules",
    "dist",
    "build",
    ".next",
    ".svelte-kit",
    ".turbo",
    ".vite",
    ".cache",
    ".ruff_cache",
    ".pytest_cache",
    ".mypy_cache",
    ".venv",
    "venv",
    "__pycache__",
    "coverage",
    "mutants.out",
)
BASELINE_REF = "locus-harness-baseline"
LOCUS_SNIPPET_START = "<!-- locus:init-snippet:start -->"
LOCUS_SNIPPET_END = "<!-- locus:init-snippet:end -->"


def main() -> int:
    args = parse_args()
    corpus = resolve_corpus(args)
    run_dir = resolve_run_dir(args)
    run_dir.mkdir(parents=True, exist_ok=False)

    task = resolve_task(args)
    locus_bin = resolve_locus_bin(args, run_dir)

    metadata: dict[str, Any] = {
        "schema_version": 1,
        "run_id": run_dir.name,
        "created_at": dt.datetime.now(dt.timezone.utc).isoformat(),
        "corpus": str(corpus),
        "task": task,
        "locus_bin": str(locus_bin),
        "arms": {},
    }

    arms = [
        ("without_locus", False),
        ("with_locus", True),
    ]
    for arm_name, expose_locus in arms:
        arm_dir = run_dir / arm_name
        arm_meta = run_arm(
            args=args,
            arm_name=arm_name,
            corpus=corpus,
            workspace=arm_dir / "workspace",
            run_dir=run_dir,
            task=task,
            locus_bin=locus_bin,
            expose_locus=expose_locus,
        )
        metadata["arms"][arm_name] = arm_meta

    summary = build_summary(metadata)
    write_json(run_dir / "summary.json", metadata)
    (run_dir / "summary.md").write_text(summary, encoding="utf-8")
    print(summary)
    print(f"\nArtifacts written to {run_dir}")
    return 0


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Compare agent behavior on a corpus with and without Locus guidance.",
    )
    parser.add_argument(
        "--corpus",
        type=Path,
        default=None,
        help="Corpus workspace to copy. Defaults to LOCUS_TEST_CORPUS.",
    )
    parser.add_argument(
        "--task",
        default=None,
        help="Task prompt to give both agent arms.",
    )
    parser.add_argument(
        "--task-file",
        type=Path,
        default=None,
        help="File containing the task prompt.",
    )
    parser.add_argument(
        "--agent-command",
        default=None,
        help=(
            "Shell command template to run in each arm. Placeholders: "
            "{workspace}, {prompt_file}, {arm}, {run_dir}, {locus_bin}. "
            "If omitted, the harness prepares worktrees and prompts only."
        ),
    )
    parser.add_argument(
        "--locus-bin",
        type=Path,
        default=None,
        help="Path to a prebuilt locus binary. Defaults to target/debug/locus.",
    )
    parser.add_argument(
        "--run-dir",
        type=Path,
        default=None,
        help="Output directory. Defaults to target/agent-locus-harness/<timestamp>.",
    )
    parser.add_argument(
        "--exclude",
        action="append",
        default=[],
        help=(
            "Additional copy exclude pattern. Defaults always exclude .git and target. "
            "May be repeated."
        ),
    )
    parser.add_argument(
        "--skip-build",
        action="store_true",
        help="Do not build target/debug/locus when --locus-bin is omitted.",
    )
    return parser.parse_args()


def resolve_corpus(args: argparse.Namespace) -> Path:
    corpus = args.corpus or os.environ.get("LOCUS_TEST_CORPUS")
    if corpus is None:
        raise SystemExit("missing --corpus or LOCUS_TEST_CORPUS")
    path = Path(corpus).expanduser().resolve()
    if not path.is_dir():
        raise SystemExit(f"corpus is not a directory: {path}")
    if not (path / "Cargo.toml").exists():
        raise SystemExit(f"corpus does not look like a Rust workspace: {path}")
    return path


def resolve_run_dir(args: argparse.Namespace) -> Path:
    if args.run_dir is not None:
        return args.run_dir.expanduser().resolve()
    stamp = dt.datetime.now().strftime("%Y%m%d-%H%M%S")
    return (REPO_ROOT / "target" / "agent-locus-harness" / stamp).resolve()


def resolve_task(args: argparse.Namespace) -> str:
    if args.task and args.task_file:
        raise SystemExit("pass only one of --task or --task-file")
    if args.task_file:
        return args.task_file.read_text(encoding="utf-8").strip()
    if args.task:
        return args.task.strip()
    raise SystemExit("missing --task or --task-file")


def resolve_locus_bin(args: argparse.Namespace, run_dir: Path) -> Path:
    if args.locus_bin is not None:
        path = args.locus_bin.expanduser().resolve()
        if not path.exists():
            raise SystemExit(f"--locus-bin does not exist: {path}")
        return path

    path = (REPO_ROOT / "target" / "debug" / "locus").resolve()
    if not args.skip_build:
        log = run_dir / "build-locus.json"
        run_command(["cargo", "build", "-p", "locus-cli"], REPO_ROOT, log)
    if not path.exists():
        raise SystemExit(f"locus binary not found: {path}")
    return path


def run_arm(
    *,
    args: argparse.Namespace,
    arm_name: str,
    corpus: Path,
    workspace: Path,
    run_dir: Path,
    task: str,
    locus_bin: Path,
    expose_locus: bool,
) -> dict[str, Any]:
    copy_corpus(corpus, workspace, tuple(DEFAULT_EXCLUDES) + tuple(args.exclude))

    init_log = run_dir / f"{arm_name}-locus-init.json"
    init_cmd = [str(locus_bin), "init", "--workspace", str(workspace)]
    if not expose_locus:
        init_cmd.append("--no-agent-instructions")
    init_result = run_command(init_cmd, REPO_ROOT, init_log, check=False)
    if not expose_locus:
        strip_locus_agent_snippets(workspace)

    git_baseline(workspace, run_dir / f"{arm_name}-git-baseline.json")
    baseline_report = run_locus_check(
        locus_bin,
        workspace,
        run_dir / f"{arm_name}-baseline-full",
        changed=False,
    )

    prompt_file = run_dir / f"{arm_name}-prompt.md"
    prompt_file.write_text(render_prompt(task, locus_bin, expose_locus), encoding="utf-8")

    agent_result = None
    if args.agent_command:
        agent_log = run_dir / f"{arm_name}-agent.json"
        command = render_agent_command(
            args.agent_command,
            workspace=workspace,
            prompt_file=prompt_file,
            arm=arm_name,
            run_dir=run_dir,
            locus_bin=locus_bin,
        )
        env = os.environ.copy()
        env.update(
            {
                "LOCUS_HARNESS_ARM": arm_name,
                "LOCUS_HARNESS_WORKSPACE": str(workspace),
                "LOCUS_HARNESS_PROMPT_FILE": str(prompt_file),
                "LOCUS_BIN": str(locus_bin),
                "LOCUS_HARNESS_BASELINE": BASELINE_REF,
            }
        )
        agent_result = run_command(command, workspace, agent_log, shell=True, env=env, check=False)

    after_full_report = run_locus_check(
        locus_bin,
        workspace,
        run_dir / f"{arm_name}-after-full",
        changed=False,
    )
    after_changed_report = run_locus_check(
        locus_bin,
        workspace,
        run_dir / f"{arm_name}-after-changed-strict",
        changed=True,
    )

    diff_stat = run_command(
        ["git", "diff", "--stat", BASELINE_REF],
        workspace,
        run_dir / f"{arm_name}-git-diff-stat.json",
        check=False,
    )
    diff_patch = run_command(
        ["git", "diff", BASELINE_REF],
        workspace,
        run_dir / f"{arm_name}-git-diff.json",
        check=False,
    )
    status = run_command(
        ["git", "status", "--short"],
        workspace,
        run_dir / f"{arm_name}-git-status.json",
        check=False,
    )

    return {
        "workspace": str(workspace),
        "prompt_file": str(prompt_file),
        "locus_exposed_to_agent": expose_locus,
        "locus_init": summarize_command(init_result),
        "agent": summarize_command(agent_result) if agent_result else None,
        "baseline_full": baseline_report,
        "after_full": after_full_report,
        "after_changed_strict": after_changed_report,
        "new_full_findings": diff_findings(
            baseline_report["json"].get("results", []),
            after_full_report["json"].get("results", []),
            added=True,
        ),
        "resolved_full_findings": diff_findings(
            baseline_report["json"].get("results", []),
            after_full_report["json"].get("results", []),
            added=False,
        ),
        "git": {
            "status": status.stdout,
            "diff_stat": diff_stat.stdout,
            "diff_patch_log": str(run_dir / f"{arm_name}-git-diff.json"),
            "diff_patch_bytes": len(diff_patch.stdout.encode("utf-8")),
        },
    }


def copy_corpus(corpus: Path, workspace: Path, excludes: tuple[str, ...]) -> None:
    if (corpus / ".git").exists():
        copy_git_worktree(corpus, workspace, excludes)
        return

    def ignore(_dir: str, names: list[str]) -> set[str]:
        ignored: set[str] = set()
        for name in names:
            if any(fnmatch.fnmatch(name, pattern) for pattern in excludes):
                ignored.add(name)
        return ignored

    shutil.copytree(corpus, workspace, symlinks=True, ignore=ignore)


def copy_git_worktree(corpus: Path, workspace: Path, excludes: tuple[str, ...]) -> None:
    proc = subprocess.run(
        ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"],
        cwd=corpus,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if proc.returncode != 0:
        raise SystemExit(
            "git ls-files failed while copying corpus:\n"
            + proc.stderr.decode("utf-8", errors="replace")
        )

    workspace.mkdir(parents=True, exist_ok=False)
    raw_paths = [p for p in proc.stdout.split(b"\0") if p]
    for raw in raw_paths:
        rel = Path(raw.decode("utf-8", errors="surrogateescape"))
        if path_is_excluded(rel, excludes):
            continue
        src = corpus / rel
        dst = workspace / rel
        if src.is_dir():
            continue
        dst.parent.mkdir(parents=True, exist_ok=True)
        if src.is_symlink():
            os.symlink(os.readlink(src), dst)
        else:
            shutil.copy2(src, dst)


def path_is_excluded(path: Path, excludes: tuple[str, ...]) -> bool:
    parts = path.parts
    return any(
        fnmatch.fnmatch(path.name, pattern)
        or any(fnmatch.fnmatch(part, pattern) for part in parts)
        or fnmatch.fnmatch(path.as_posix(), pattern)
        for pattern in excludes
    )


def strip_locus_agent_snippets(workspace: Path) -> None:
    for filename in ("AGENTS.md", "CLAUDE.md"):
        path = workspace / filename
        if not path.exists():
            continue
        text = path.read_text(encoding="utf-8")
        stripped = strip_between_markers(text, LOCUS_SNIPPET_START, LOCUS_SNIPPET_END)
        if stripped != text:
            path.write_text(stripped, encoding="utf-8")


def strip_between_markers(text: str, start_marker: str, end_marker: str) -> str:
    out = text
    while True:
        start = out.find(start_marker)
        if start == -1:
            break
        end_rel = out[start:].find(end_marker)
        if end_rel == -1:
            break
        end = start + end_rel + len(end_marker)
        out = (out[:start].rstrip() + "\n\n" + out[end:].lstrip()).strip() + "\n"
    return out


def git_baseline(workspace: Path, log: Path) -> None:
    commands = [
        ["git", "init"],
        ["git", "add", "-A"],
        [
            "git",
            "-c",
            "user.name=Locus Harness",
            "-c",
            "user.email=locus-harness@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "locus harness baseline",
        ],
        ["git", "tag", "-f", BASELINE_REF],
    ]
    results = []
    for index, cmd in enumerate(commands):
        result = run_command(cmd, workspace, log.with_name(f"{log.stem}-{index}.json"))
        results.append(summarize_command(result))
    write_json(log, {"commands": results})


def run_locus_check(
    locus_bin: Path,
    workspace: Path,
    artifact_prefix: Path,
    *,
    changed: bool,
) -> dict[str, Any]:
    cmd = [str(locus_bin), "check", "--workspace", str(workspace), "--format", "json"]
    if changed:
        cmd.extend(["--changed", "--baseline", BASELINE_REF, "--agent-strict"])
    result = run_command(cmd, workspace, artifact_prefix.with_suffix(".command.json"), check=False)
    json_path = artifact_prefix.with_suffix(".json")
    json_text = result.stdout.strip()
    parsed = parse_json_or_empty(json_text)
    write_json(json_path, parsed)
    return {
        "command": result.command,
        "returncode": result.returncode,
        "stdout_path": str(artifact_prefix.with_suffix(".command.json")),
        "json_path": str(json_path),
        "json": parsed,
        "summary": parsed.get("summary", {}),
        "by_rule": count_by_rule(parsed.get("results", [])),
    }


def render_prompt(task: str, locus_bin: Path, expose_locus: bool) -> str:
    if expose_locus:
        return f"""# Task
{task}

# Harness Arm
This is the with-Locus arm. Use Locus as the local architecture oracle.

Before editing, run:

```bash
{shlex.quote(str(locus_bin))} check --workspace . --format text
```

When choosing where code belongs, prefer the accepted owners, boundaries,
converters, and runtime owners implied by Locus output. Do not weaken
`.locus/lock.json`, add broad exceptions, raise budgets, or silence findings
unless the task explicitly asks for an architecture-policy change.

Before finishing, run:

```bash
{shlex.quote(str(locus_bin))} check --workspace . --changed --baseline {BASELINE_REF} --agent-strict --format text
```

If findings remain, fix the code first. If a finding is intentional, explain the
specific architectural decision and the narrow Locus mutator that should record
it; do not hand-edit `.locus/lock.json`.
"""

    return f"""# Task
{task}

# Harness Arm
This is the without-Locus arm. Do not run `locus`, read `.locus`, or use Locus
diagnostics while implementing this task. Use the repository's normal source
context, compiler, tests, and local reasoning.
"""


def render_agent_command(
    template: str,
    *,
    workspace: Path,
    prompt_file: Path,
    arm: str,
    run_dir: Path,
    locus_bin: Path,
) -> str:
    replacements = {
        "workspace": shlex.quote(str(workspace)),
        "prompt_file": shlex.quote(str(prompt_file)),
        "arm": shlex.quote(arm),
        "run_dir": shlex.quote(str(run_dir)),
        "locus_bin": shlex.quote(str(locus_bin)),
    }
    return template.format(**replacements)


class CommandResult:
    def __init__(
        self,
        *,
        command: str,
        cwd: Path,
        returncode: int,
        stdout: str,
        stderr: str,
        duration_seconds: float,
    ) -> None:
        self.command = command
        self.cwd = cwd
        self.returncode = returncode
        self.stdout = stdout
        self.stderr = stderr
        self.duration_seconds = duration_seconds


def run_command(
    command: list[str] | str,
    cwd: Path,
    log_path: Path,
    *,
    shell: bool = False,
    env: dict[str, str] | None = None,
    check: bool = True,
) -> CommandResult:
    started = time.monotonic()
    proc = subprocess.run(
        command,
        cwd=cwd,
        shell=shell,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    duration = time.monotonic() - started
    rendered = command if isinstance(command, str) else " ".join(shlex.quote(part) for part in command)
    result = CommandResult(
        command=rendered,
        cwd=cwd,
        returncode=proc.returncode,
        stdout=proc.stdout,
        stderr=proc.stderr,
        duration_seconds=duration,
    )
    write_json(log_path, command_result_to_json(result))
    if check and proc.returncode != 0:
        raise SystemExit(f"command failed ({proc.returncode}): {rendered}\nlog: {log_path}")
    return result


def command_result_to_json(result: CommandResult) -> dict[str, Any]:
    return {
        "command": result.command,
        "cwd": str(result.cwd),
        "returncode": result.returncode,
        "duration_seconds": round(result.duration_seconds, 3),
        "stdout": result.stdout,
        "stderr": result.stderr,
    }


def summarize_command(result: CommandResult | None) -> dict[str, Any] | None:
    if result is None:
        return None
    return {
        "command": result.command,
        "cwd": str(result.cwd),
        "returncode": result.returncode,
        "duration_seconds": round(result.duration_seconds, 3),
        "stdout_bytes": len(result.stdout.encode("utf-8")),
        "stderr_bytes": len(result.stderr.encode("utf-8")),
    }


def parse_json_or_empty(text: str) -> dict[str, Any]:
    if not text:
        return {
            "parse_error": "empty stdout",
            "summary": {"fatal": 0, "warning": 0, "advisory": 0},
            "results": [],
        }
    try:
        return json.loads(text)
    except json.JSONDecodeError as exc:
        return {
            "parse_error": str(exc),
            "raw_stdout": text,
            "summary": {"fatal": 0, "warning": 0, "advisory": 0},
            "results": [],
        }


def count_by_rule(results: list[dict[str, Any]]) -> dict[str, int]:
    counts: dict[str, int] = {}
    for result in results:
        rule = str(result.get("rule_id", "UNKNOWN"))
        counts[rule] = counts.get(rule, 0) + 1
    return dict(sorted(counts.items()))


def diff_findings(
    before: list[dict[str, Any]],
    after: list[dict[str, Any]],
    *,
    added: bool,
) -> list[dict[str, Any]]:
    before_keys = {finding_key(item) for item in before}
    after_keys = {finding_key(item) for item in after}
    wanted = after_keys - before_keys if added else before_keys - after_keys
    source = after if added else before
    return [compact_finding(item) for item in source if finding_key(item) in wanted]


def finding_key(item: dict[str, Any]) -> tuple[Any, ...]:
    loc = item.get("location", {})
    return (
        item.get("rule_id"),
        item.get("severity"),
        loc.get("file"),
        loc.get("line_start"),
        loc.get("line_end"),
        item.get("message"),
    )


def compact_finding(item: dict[str, Any]) -> dict[str, Any]:
    loc = item.get("location", {})
    return {
        "rule_id": item.get("rule_id"),
        "severity": item.get("severity"),
        "file": loc.get("file"),
        "line_start": loc.get("line_start"),
        "message": item.get("message"),
    }


def build_summary(metadata: dict[str, Any]) -> str:
    lines = [
        "# Agent/Locus Harness Summary",
        "",
        f"Corpus: `{metadata['corpus']}`",
        f"Task: {metadata['task']}",
        "",
        "| Arm | Agent Exit | Changed Fatal | Changed Warning | Changed Advisory | New Full Findings | Diff Bytes |",
        "| --- | ---: | ---: | ---: | ---: | ---: | ---: |",
    ]
    for arm_name in ("without_locus", "with_locus"):
        arm = metadata["arms"][arm_name]
        changed = arm["after_changed_strict"]["summary"]
        agent = arm["agent"]
        agent_exit = "n/a" if agent is None else agent["returncode"]
        diff_bytes = arm["git"]["diff_patch_bytes"]
        lines.append(
            "| {arm} | {agent_exit} | {fatal} | {warning} | {advisory} | {new} | {diff_bytes} |".format(
                arm=arm_name,
                agent_exit=agent_exit,
                fatal=changed.get("fatal", 0),
                warning=changed.get("warning", 0),
                advisory=changed.get("advisory", 0),
                new=len(arm["new_full_findings"]),
                diff_bytes=diff_bytes,
            )
        )
    lines.append("")
    for arm_name in ("without_locus", "with_locus"):
        arm = metadata["arms"][arm_name]
        lines.append(f"## {arm_name}")
        lines.append("")
        lines.append("Changed strict findings by rule:")
        by_rule = arm["after_changed_strict"]["by_rule"]
        if by_rule:
            for rule, count in by_rule.items():
                lines.append(f"- `{rule}`: {count}")
        else:
            lines.append("- none")
        lines.append("")
        new_findings = arm["new_full_findings"][:10]
        lines.append("First new full-workspace findings:")
        if new_findings:
            for finding in new_findings:
                loc = f"{finding.get('file')}:{finding.get('line_start')}"
                lines.append(
                    f"- `{finding.get('rule_id')}` {finding.get('severity')} {loc} - {finding.get('message')}"
                )
        else:
            lines.append("- none")
        lines.append("")
        diff_stat = arm["git"]["diff_stat"].strip()
        lines.append("Git diff stat:")
        lines.append("```text")
        lines.append(diff_stat if diff_stat else "(no diff)")
        lines.append("```")
        lines.append("")
    return "\n".join(lines).rstrip() + "\n"


def write_json(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except KeyboardInterrupt:
        print("interrupted", file=sys.stderr)
        raise SystemExit(130)
