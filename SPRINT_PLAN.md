# Sprint 1 implementation plan

Goal: one dependable offline selection → inspection → conversion → cancellation
workflow on macOS Apple Silicon. Architecture/contracts precede runtime work.
No agent may access real user media, commit without instruction, push, merge,
change remotes or edit another worktree. The maintainer integrates reviewed
changes; the assignments below do not authorize agents to integrate branches.

## Current foundation

- Rust workspace and `loconv-contracts` serde models, with synthetic fixture tests.
- TS mirror and DesktopApi port under `apps/desktop/src/api/`.
- ARCHITECTURE.md and PRIVACY.md define shared behavior and acceptance gates.
- Existing Python orchestrator preserved. No converter application, FFmpeg binary,
  React dependency, Tauri host, lockfile for JS, or image engine is supplied yet.

## Parallel ownership and handoff

These are assignments for subsequent independently launched agents, not requests
to launch them in this architecture task. Start each from the same reviewed
foundation in its own isolated worktree. Do not recreate a competing root scaffold.

| Owner | Exclusive implementation paths | Deliverable |
| --- | --- | --- |
| Antigravity | `apps/desktop/src/` except existing `api/contracts.ts`; desktop `package.json`, JS lockfile, `index.html`, Vite/TS configs, frontend tests | React/Vite UI and typed adapter/mock |
| OpenCode | `crates/engine/`, `apps/desktop/src-tauri/`, Rust root manifests/lockfile, engine tests and synthetic generators | Engine, native Tauri shell, tool packaging manifest |
| Architecture maintainer | root architecture/privacy/plan, `crates/contracts/`, `api/contracts.ts`, `contracts/fixtures/` | Contract changes and integration review |

Neither implementation agent edits Python tooling or the other's assigned files.
Backend issues requiring wire changes are proposals to the architecture maintainer;
do not silently change command names, nullability or enum spelling. Antigravity
owns npm scripts including `tauri dev/build`; OpenCode provides the required
configuration hooks. Use `apps/desktop` as frontend working directory, Vite port
1420 with strictPort, and Tauri frontendDist `../dist`.

## Antigravity tasks (can start against mock immediately)

1. Initialize React + TypeScript + Vite without replacing contracts. Add pinned
   dependencies/lockfile, strict TS checking and deterministic scripts: `dev`,
   `build`, `typecheck`, `test`, `tauri`. Use `@tauri-apps/api` v2 and CLI v2.
   No remote UI resources or runtime network libraries.
2. Implement DesktopApi mock with invented names/metadata and deterministic queue
   transitions. Build accessible file list, format selector, destination selector,
   output basename, queue/progress, cancel button and fixed error messages.
   Disable unavailable presets; show indeterminate progress and empty states.
   Selecting a format is not equivalent to proving input compatibility.
3. Implement Tauri adapter with the exact ARCHITECTURE command arguments and
   event name. Validate unknown payloads; map unexpected rejections to safe errors.
   Check API version on startup and display a fixed incompatibility state.
   Subscribe before listJobs, reconcile revisions and dispose listeners.
   Native drop forwards only paths; never read File bytes in JS.
4. Test synthetic fixture conformance in TS, form validation, failed selection,
   unavailable formats, cancel, duplicate/stale events, startup resync, event
   disposal and safe rendering of probe strings. Run typecheck/build/tests.

Acceptance: complete workflow demonstrable with a mock while the engine is absent;
switching to native adapter requires no component rewrite. Never use a mock in a
release build silently; production must fail visibly if native integration fails.

## OpenCode tasks (ordered backend increments)

1. Create `loconv-engine` with inspection/formats/jobs/paths/errors and backend
   modules, dependent only on contracts and local libraries. Add Tauri 2 host
   using bundled frontend assets and command signatures from ARCHITECTURE.
   Implement native dialogs, selection registry, minimal capability permissions,
   CSP and scoped main-window events. No generic JS shell/fs access.
2. Add trusted tool resolver and capability discovery. Missing tools produce
   disabled A/V presets rather than download prompts. Record pinned build
   versions/checksums/configuration/licenses and target-triple naming. Verify
   network-disabled binaries for both FFmpeg and FFprobe before distribution.
   An explicit development-only resolver may accept a developer-configured tool
   path; release must not search PATH or accept arbitrary binary paths from IPC.
3. Implement bounded inspection with deadlines, local path/mount validation,
   signature/container allowlists and minimal normalized metadata. Add `image`
   with explicit codec features and decoder limits. Verify animated input rejection.
   Test stream selection, malformed JSON, timeouts, pipe limits, unavailable tools
   and access errors with synthetic data/fake subprocesses.
4. Implement fixed registry presets, enqueue validation, FIFO single-worker
   manager, monotonic revisions and progress. Add target reservations, unique temp
   files, no-replace publication and explicit metadata stripping. Test changed
   inputs, missing streams, basename injection, symlink/hardlink aliases,
   duplicate targets and target-creation races. No overwrite feature.
5. Implement cancellation/shutdown with supervised child termination, output
   cleanup, terminal idempotency and success/cancel ordering. Test fake hung child,
   partial writes, failure cleanup and cancellation at each lifecycle boundary.
6. Generate tiny video/audio/image samples locally; run real inspection and each
   available preset on macOS aarch64. Decode/inspect synthetic outputs to verify
   formats and stripped tags; verify existing inputs/outputs are untouched.

Acceptance: cargo fmt/check/test/clippy, fake-process tests, synthetic integration
matrix and network-denied smoke test pass. Record unavailable encoders explicitly;
do not report skipped cases as passing. Do not claim Windows/Linux support until
platform adapters and CI tests exist.

## Maintainer integration and release gates

1. Review contracts first. Both agents report owned paths, commands executed,
   test results and limitations. Resolve contract proposals centrally.
2. Review backend and frontend diffs independently; integrate only by an explicit
   maintainer action, outside this task. Resolve npm/Tauri hooks in that review.
3. Run Python regression tests, Rust checks, TS checks, frontend build/tests and
   synthetic native end-to-end tests from the combined result. Contract fixture
   parity across Rust/TS is mandatory before connecting adapters.
4. Execute PRIVACY.md release evidence on packaged macOS aarch64 build with
   networking denied. Audit bundled assets, capabilities, CSP, binaries and licenses.
5. Manually review source preservation, conflicts, cancellation/close behavior,
   safe error messages and accessible UI using synthetic inputs only.

Deferred: presets editor, arbitrary FFmpeg flags, thumbnails, playback, batch
profiles, resize/trim, persistent history, acceleration, auto-update, store
distribution and verified Windows/Linux packaging. Signing/notarization and
FFmpeg/project licensing are distribution blockers, not reasons to broaden the
converter implementation in this architecture task.

## Foundation verification

Executed for this change using Rust 1.99.1:

- `cargo test --workspace --offline`: 3 integration tests passed (synthetic JSON
  round trips, invalid request fields/formats, job-state wire spellings).
- `cargo fmt --all -- --check`: passed after formatting.
- `cargo clippy --workspace --all-targets --offline -- -D warnings`: passed.
- `UV_CACHE_DIR="$PWD/.test-cache/uv" UV_PROJECT_ENVIRONMENT="$PWD/.test-cache/venv" uv run --offline --no-project --with pytest --with pytest-asyncio python -m pytest -q`:
  4 existing orchestrator tests passed; existing pytest-asyncio default-loop-scope
  deprecation warning remains. Temporary local cache removed after verification.
- `git diff --check`: passed.

The initial plain `python3 -m pytest -q` could not find pytest; an initial uv
attempt could not write its default cache in the sandbox. The offline command
above succeeded using available Python packages. No dependencies were downloaded.
The declared minimum Rust version 1.85 has not been tested. TS compilation and
cross-language fixture validation await frontend tooling. No actual media was
accessed. Native processing, runtime semantic validation, packaged network denial,
and Windows/Linux behavior remain unimplemented/unverified.
