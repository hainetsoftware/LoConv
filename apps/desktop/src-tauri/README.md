# Native Tauri 2 host — Sprint 03A

The host implements all eight IPC v1 command names and exact argument/result
shapes from `ARCHITECTURE.md`. Shared Rust/TypeScript contracts and the React UI
are unchanged. All state is session-only; closing the process discards it.

## Startup error diagnosis

`App.tsx` calls these operations in order:

1. `get_api_version` → `1`.
2. `list_formats` → the seven descriptors from the existing Rust registry.
3. `plugin:event|listen` for `loconv://job-changed`.
4. `list_jobs` → `[]` in a fresh session.

The previous host registered/authorized only `get_api_version`. Startup therefore
failed first at the unknown/unauthorized `list_formats` command; the frontend
correctly normalized Tauri's rejection to “An unexpected internal error occurred”.
After adding that command alone, missing event permissions and `list_jobs` would
have been the next failures. All three gaps are addressed. Invoke integration
tests exercise this sequence using the production context, ACL and handlers.
No startup IPC error remains in those tests. A live WebView launch has not been
verified in this sprint; no claim of full packaged/offline OS acceptance is made.

## Commands and safety

- `select_inputs`: Rust opens the native macOS file picker on the main thread.
  Cancel returns `[]`; selecting files returns fixed `tool_unavailable` because
  inspection is still unavailable. No selected file content is read, registered,
  or returned to JavaScript.
- `import_dropped_inputs`: empty input returns `[]`; invalid path strings/limits
  are rejected. Nonempty otherwise valid selections return `tool_unavailable`
  before filesystem access. No input capability is granted without inspection
  and locality validation. Multi-file failure registers nothing.
- `select_destination`: Rust opens the native macOS folder picker. Cancel returns
  `null`. Selected directories are canonicalized, checked with `statfs/MNT_LOCAL`
  and write/search permission checks, then stored under random UUIDs in a bounded
  table (100 selections). JS receives only the opaque ID and basename label.
  Filesystem checks happen outside the session mutex. These are selection-time
  checks; future conversion must revalidate before publication.
- `list_formats`: FFmpeg presets report `available=false` / `tool_unavailable`;
  image presets report `available=false` / `unsupported_format`, exactly as the
  existing registry specifies. No ambient PATH lookup or download is performed.
- `enqueue_job`: validates the v1 DTO/basename and returns the registry's explicit
  unavailable error. It creates no job/output and never simulates conversion.
- `list_jobs`: returns snapshots copied from the session registry, initially `[]`.
- `cancel_job`: unknown ID returns `job_not_found`. Queued snapshots can transition
  once to `cancelled`, incrementing revision and emitting a full v1 event only to
  `main`. Terminal cancellation is idempotent. Registry insertion is an engine
  seam for future validated jobs and synthetic tests, not an exposed JS command.

The registry is bounded to 100 jobs and serialized by the native session mutex.
It has no worker, success publication, disk persistence or media access. Callers
of the engine's `register_queued` seam must validate selection capabilities and
backend availability before insertion; the current native enqueue path never
calls it because all backends are unavailable.

Entire invoke envelopes reject unknown/missing fields with fixed `AppError`
objects. No raw serde, OS or tool error text crosses IPC. Only the local `main`
window receives app command and event listen/unlisten permissions. No generic
filesystem, shell, HTTP, dialog plugin API or remote origin is enabled. Release
CSP remains unchanged. Native picker cloud placeholder downloads are disabled.

## Remaining functionality

Inspection, FFprobe/FFmpeg execution, image codecs, active worker cancellation,
output reservation/publication/cleanup and shutdown coordination remain
unimplemented. No real conversion is advertised or attempted. Inputs cannot yet
be successfully imported. The existing frontend HTML drop handler also relies
on browser `File.path`/name rather than native Tauri drop events; that frontend
limitation is unchanged, and a bare filename is rejected by the native command.

Native dialogs/locality checks are implemented for the primary macOS target.
Windows/Linux dialog and mount adapters return explicit `tool_unavailable` and
are untested. The actual macOS picker interaction is not automated; tests replace
only the picker seam and exercise its IPC cancellation/selection results.
Remote mounts, ACL edge cases and cloud filesystem providers have not undergone
OS-level acceptance testing. `MNT_LOCAL` is a mount check, not proof that
independent sync software cannot observe a local directory. Packaging remains
disabled. The network-denied release conversion matrix remains a release gate.

## Verification (2026-11-15, macOS aarch64)

- `cargo test --workspace --offline`: 26 tests passed, including six native
  invoke integration tests using production permissions, three shared wire
  tests, registry bounds, concurrent cancellation and terminal immutability.
- `cargo clippy --workspace --all-targets --offline -- -D warnings`: passed.
- `npm test`: 46 tests passed. Existing React `act(...)` warnings appear in
  `App.test.tsx`; they do not fail the suite.
- `npm run build`: TypeScript check and Vite production build passed.
- `cargo build -p loconv-desktop --features custom-protocol --offline`: passed
  with bundled frontend assets (development profile; no installer/signing).
- `cargo fmt --all -- --check` and `git diff --check`: passed.
- Synthetic invented paths and one test-owned empty temporary directory only.
  No user media was accessed; no media or metadata was uploaded.

See `SPRINT-03A-AUDIT.md` for dependency findings and audit limitations.
