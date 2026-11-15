"""Local task orchestration, Git isolation, and cancellable subprocess execution."""
from __future__ import annotations

import asyncio
import json
import os
import re
import shutil
import signal
import subprocess
import sys
import uuid
from dataclasses import asdict, dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Callable

AGENTS = ("codex", "agy", "opencode", "cline")
DEFAULT_COMMANDS = {
    "codex": ["codex", "exec", "--sandbox", "workspace-write", "{prompt}"],
    "agy": ["agy", "-p", "{prompt}", "--output-format", "stream-json", "--sandbox"],
    "opencode": ["opencode", "run", "--format", "json", "{prompt}"],
    "cline": ["cline", "--json", "--auto-approve", "true", "{prompt}"],
}


def utcnow() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="seconds")


def git(repo: Path, *args: str, check: bool = True) -> subprocess.CompletedProcess[str]:
    return subprocess.run(["git", "-C", str(repo), *args], text=True, capture_output=True, check=check)


def find_git_root(path: Path) -> Path:
    result = git(path.resolve(), "rev-parse", "--show-toplevel")
    return Path(result.stdout.strip()).resolve()


def validate_repo(repo: Path) -> None:
    if not (repo / ".git").exists():
        raise ValueError("Provide the primary checkout of a Git repository (not a worktree).")
    if not git(repo, "rev-parse", "--verify", "HEAD", check=False).returncode == 0:
        raise ValueError("Repository has no initial commit. Create one before starting agents.")
    changes = git(repo, "status", "--porcelain", "--untracked-files=normal").stdout.splitlines()
    if any(not line[3:].startswith(".loconv/") for line in changes):
        raise ValueError("Main checkout has uncommitted changes. Commit or stash them first.")


def safe_branch(task_id: str, agent: str) -> str:
    if agent not in AGENTS or not re.fullmatch(r"[a-f0-9]{8}", task_id):
        raise ValueError("Unsafe branch identifier")
    return f"agent/{agent}/{task_id}"


@dataclass
class Task:
    id: str
    agent: str
    title: str
    prompt: str
    status: str = "queued"
    branch: str = ""
    worktree: str = ""
    created: str = field(default_factory=utcnow)
    started: str = ""
    finished: str = ""
    exit_code: int | None = None
    note: str = ""


class Orchestrator:
    def __init__(self, repo: Path, emit: Callable[[str, str], None]):
        self.repo = repo.resolve()
        validate_repo(self.repo)
        self.emit = emit
        self.state_dir = self.repo / ".loconv"
        self.state_dir.mkdir(exist_ok=True)
        self.worktrees = self.state_dir / "worktrees"
        self.logs = self.state_dir / "logs"
        self.worktrees.mkdir(exist_ok=True)
        self.logs.mkdir(exist_ok=True)
        self.state_path = self.state_dir / "tasks.json"
        self.config_path = self.state_dir / "agents.json"
        self.config = self._load_config()
        self.tasks: list[Task] = self._load_tasks()
        self.running: dict[str, asyncio.subprocess.Process] = {}
        self.workers: dict[str, asyncio.Task[None]] = {}
        self.agent_locks = {name: asyncio.Lock() for name in AGENTS}
        for task in self.tasks:
            if task.status in ("running", "preparing"):
                task.status = "interrupted"
                task.note = "Orchestrator restarted; check worktree and logs."
        self.save()

    def _load_config(self) -> dict[str, list[str]]:
        if not self.config_path.exists():
            self.config_path.write_text(json.dumps(DEFAULT_COMMANDS, indent=2) + "\n")
        raw = json.loads(self.config_path.read_text())
        for agent in AGENTS:
            cmd = raw.get(agent)
            if not isinstance(cmd, list) or not cmd or not all(isinstance(s, str) for s in cmd):
                raise ValueError(f"Invalid command configuration for {agent}")
            if sum(s.count("{prompt}") for s in cmd) != 1:
                raise ValueError(f"Expected exactly one {{prompt}} placeholder for {agent}")
        return raw

    def _load_tasks(self) -> list[Task]:
        if not self.state_path.exists():
            return []
        return [Task(**obj) for obj in json.loads(self.state_path.read_text())]

    def save(self) -> None:
        temp = self.state_path.with_suffix(".tmp")
        temp.write_text(json.dumps([asdict(t) for t in self.tasks], indent=2, ensure_ascii=False) + "\n")
        os.replace(temp, self.state_path)

    def create(self, agent: str, title: str, prompt: str) -> Task:
        if agent not in AGENTS:
            raise ValueError("Unknown agent")
        if not title.strip() or not prompt.strip():
            raise ValueError("Task title and prompt are required")
        task = Task(id=uuid.uuid4().hex[:8], agent=agent, title=title.strip(), prompt=prompt.strip())
        task.branch = safe_branch(task.id, task.agent)
        task.worktree = str(self.worktrees / task.id)
        self.tasks.append(task)
        self.save()
        self.emit(task.id, f"Queued task for {agent}: {title}")
        return task

    def get(self, task_id: str) -> Task:
        return next(t for t in self.tasks if t.id == task_id)

    def command(self, task: Task) -> list[str]:
        prompt = (
            "You are contributing to LoConv, an offline desktop media converter. "
            "Work only within this Git worktree. Never push, merge, or change remote Git settings. "
            "Never access or upload user media. Preserve offline media-processing requirements. "
            "Commit nothing unless expressly requested. Do not modify another agent's worktree. "
            "Explain implemented changes, tests executed, and known limitations.\n\n"
            f"TASK: {task.title}\n{task.prompt}"
        )
        return [part.replace("{prompt}", prompt) for part in self.config[task.agent]]

    def available(self) -> dict[str, bool]:
        return {a: shutil.which(self.config[a][0]) is not None for a in AGENTS}

    async def execute(self, task: Task) -> None:
        if task.status not in ("queued", "failed", "interrupted"):
            self.emit(task.id, f"Cannot start task in status {task.status}")
            return
        async with self.agent_locks[task.agent]:
            if task.status == "cancelled":
                return
            task.status = "preparing"
            self.save()
            worktree = Path(task.worktree)
            try:
                if not worktree.exists():
                    proc = await asyncio.create_subprocess_exec(
                        "git", "-C", str(self.repo), "worktree", "add", "-b", task.branch,
                        str(worktree), "HEAD", stdout=asyncio.subprocess.PIPE,
                        stderr=asyncio.subprocess.PIPE)
                    _, stderr = await proc.communicate()
                    if proc.returncode:
                        raise RuntimeError(stderr.decode(errors="replace"))
                argv = self.command(task)
                if not shutil.which(argv[0]):
                    raise RuntimeError(f"CLI not found: {argv[0]}")
                task.started = utcnow()
                task.finished = ""
                task.status = "running"
                task.exit_code = None
                self.save()
                self.emit(task.id, f"Started {task.agent} in isolated worktree")
                creation = {"start_new_session": True} if os.name == "posix" else {}
                proc = await asyncio.create_subprocess_exec(
                    *argv, cwd=worktree, stdout=asyncio.subprocess.PIPE,
                    stderr=asyncio.subprocess.STDOUT, stdin=asyncio.subprocess.DEVNULL,
                    **creation)
                self.running[task.id] = proc
                logpath = self.logs / f"{task.id}.log"
                with logpath.open("a", encoding="utf-8") as logfile:
                    logfile.write(f"\n[{utcnow()}] {task.agent} started\n")
                    assert proc.stdout is not None
                    while True:
                        line = await proc.stdout.readline()
                        if not line:
                            break
                        decoded = line.decode("utf-8", errors="replace").rstrip()
                        logfile.write(decoded + "\n")
                        logfile.flush()
                        if len(decoded) > 800:
                            decoded = decoded[:800] + "…"
                        self.emit(task.id, decoded)
                await proc.wait()
                task.exit_code = proc.returncode
                if task.status != "cancelled":
                    task.status = "completed" if proc.returncode == 0 else "failed"
                self.emit(task.id, f"Process exited with code {proc.returncode}")
            except asyncio.CancelledError:
                task.status = "cancelled"
                raise
            except Exception as exc:
                task.status = "failed"
                task.note = str(exc)
                self.emit(task.id, f"ERROR: {exc}")
            finally:
                task.finished = utcnow()
                self.running.pop(task.id, None)
                self.save()

    def launch(self, task: Task) -> None:
        if task.id in self.workers and not self.workers[task.id].done():
            return
        self.workers[task.id] = asyncio.create_task(self.execute(task))

    async def cancel(self, task: Task) -> None:
        if task.status == "queued":
            task.status = "cancelled"
        elif task.status == "running":
            task.status = "cancelled"
            proc = self.running.get(task.id)
            if proc and proc.returncode is None:
                if sys.platform != "win32":
                    try:
                        os.killpg(proc.pid, signal.SIGTERM)
                    except ProcessLookupError:
                        pass
                else:
                    proc.terminate()
                try:
                    await asyncio.wait_for(proc.wait(), timeout=5)
                except asyncio.TimeoutError:
                    if sys.platform != "win32":
                        try:
                            os.killpg(proc.pid, signal.SIGKILL)
                        except ProcessLookupError:
                            pass
                    else:
                        proc.kill()
                    await proc.wait()
        self.save()
        self.emit(task.id, "Cancellation requested")

    def diffstat(self, task: Task) -> str:
        worktree = Path(task.worktree)
        if not worktree.exists():
            return "Worktree not created yet."
        result = git(worktree, "status", "--short", check=False)
        return result.stdout.strip() or "No uncommitted changes."

    async def shutdown(self) -> None:
        for task in self.tasks:
            if task.status == "running":
                await self.cancel(task)
