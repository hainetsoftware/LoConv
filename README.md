# LoConv Orchestrator v0.1

## Converter architecture foundation

This repository currently contains the development orchestrator below and a
Sprint 1 converter foundation, not a runnable media converter. Start with
[ARCHITECTURE.md](ARCHITECTURE.md), [SPRINT_PLAN.md](SPRINT_PLAN.md) and
[PRIVACY.md](PRIVACY.md). Shared IPC types live in `crates/contracts` and
`apps/desktop/src/api/contracts.ts`. Run `cargo test --workspace --offline` for
contract checks when Rust dependencies are cached. The planned converter uses
Tauri 2, React/TypeScript/Vite, Rust, bundled FFmpeg/FFprobe and Rust image codecs.

A **local Textual TUI** for coordinating four coding CLIs in isolated Git worktrees. Designed to bootstrap the **LoConv** offline media converter project.

## Get started on your Mac

```bash
git clone https://github.com/hainetsoftware/LoConv.git
cd LoConv
python3 -m venv .venv
source .venv/bin/activate
python -m pip install -e '.[dev]'
loconv --repo .
```

> If using the ZIP download instead of cloning, place its contents in the local clone first. Git must have an initial commit, and the primary checkout must be clean. If the repository is empty, commit these orchestrator files as the first commit.

### Features

- Keyboard-driven task queue: **N** new, **R** approve/run, **X** cancel, **D** git status, **L** logs, **Q** exit.
- Codex, `agy`, OpenCode, and Cline adapters through editable argument templates.
- Separate Git worktree and branch for each task (`agent/<agent>/<id>`).
- Live combined stdout/stderr and persistent per-task logs.
- Task state saved in `.loconv/tasks.json` (ignored by Git).
- No automatic pushes, merges, or remote operations.
- One concurrent task per agent (different agents may run simultaneously).
- Agent presence detection from the active terminal `PATH`.

### Important security limitations

**Git worktrees isolate version-control working directories, not processes.** Agents can access other directories, credentials and networks unless restricted by their own sandbox/permissions or by the OS. Cline is configured with auto-approval **inside its worktree** by default, which is potentially dangerous; inspect `.loconv/agents.json` first and change its configuration if necessary. Headless `agy` permissions can soft-deny edits while still returning exit code zero; inspect logs and Git changes. This program does not guarantee an offline development environment: cloud coding models transmit prompts and source code to their providers. The **resulting media converter**, not its development environment, is intended to operate fully offline.

Do not feed actual personal media into the coding agents. Use synthetic samples. Keep agent processes away from sensitive local folders. Never expose credentials in prompts.

### Adapter configuration

On first launch, `.loconv/agents.json` is created. Every agent must have an array of command arguments with exactly one `{prompt}` placeholder. Change command flags to match your installed CLI versions and permission preferences. For instance:

```json
{
  "codex": ["codex", "exec", "--sandbox", "workspace-write", "{prompt}"],
  "agy": ["agy", "-p", "{prompt}", "--output-format", "stream-json", "--sandbox"],
  "opencode": ["opencode", "run", "--format", "json", "{prompt}"],
  "cline": ["cline", "--json", "--auto-approve", "true", "{prompt}"]
}
```

Configuration is reloaded on application startup. You must authenticate each CLI separately before running non-interactive tasks.

### Earlier bootstrap example (superseded)

Use SPRINT_PLAN.md for current agent ownership and contracts. The following was
the earlier bootstrap suggestion, before the shared architecture was established:

**Codex**: “Initialize the Tauri 2 + Rust backend and implement local FFprobe media inspection with tests. Do not modify the React frontend.”

**Antigravity**: “Build the React + TypeScript UI skeleton with a drag-and-drop file list and output format controls. Do not modify the Rust backend.”

Review the changes in each worktree before merging. Do not run both on the same initial repository scaffolding without agreeing on a directory/API contract first.

### Run tests

```bash
python -m pytest -q
```

### Notes

Task status `completed` means the CLI returned exit code zero, **not that code quality, tests, or all requested features were verified**. Review logs and actual Git status. Phase 2 can add dependency-aware task scheduling, build/test gates, PR management, structured event parsing, model quotas, and explicit merge review.
