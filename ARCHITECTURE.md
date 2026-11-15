# LoConv architecture — Sprint 1

Status: proposed implementation baseline, IPC version 1. This change supplies data
contracts and a workspace, not a functioning desktop converter.

## Repository baseline and scope

The inspected repository contains a Python/Textual coding-agent orchestrator
(`loconv/`, `pyproject.toml`, `tests/test_core.py`). It is development tooling,
not the converter runtime. Preserve its command and behavior. There was no Tauri,
Rust, React, Vite, FFmpeg integration, or media engine. Existing tracked cache
files are unrelated and left untouched. No user media is needed for this work.

Sprint 1 builds one local workflow: select or drop files, inspect, select a
compatible output preset and destination, enqueue, observe progress, cancel,
and receive a safe result. Exclude editing, previews/thumbnails, accounts,
telemetry, cloud services, plugins, persistent history, and arbitrary FFmpeg flags.

## Structure and dependency direction

```text
apps/desktop/
  src/api/contracts.ts       # current TS wire types + DesktopApi port
  src/api/tauri.ts           # planned invoke/event adapter
  src/api/mock.ts            # planned synthetic in-memory adapter
  src/features/{inputs,queue,output}/  # planned React components
  src-tauri/                 # reserved Tauri 2 host (OpenCode)
crates/contracts/            # current serde wire DTOs, no runtime services
crates/engine/               # planned Rust library, no Tauri dependency
  src/{inspection,formats,jobs,errors,paths}.rs
  src/backends/{ffmpeg,image}.rs
contracts/fixtures/          # synthetic JSON shared by both languages
loconv/                     # existing Python development tooling
```

React → DesktopApi → Tauri command adapter → engine → local backend.
The engine owns paths, validation, subprocesses, output publication, and queue
state. React owns presentation and selection state; it cannot construct a process
command. Tauri owns native dialogs, drop events, application lifecycle, and
mapping engine errors/events into DTOs. Do not add a local HTTP server, database,
message broker, or generic backend plugin framework.

Use React + TypeScript + Vite for bundled assets; Rust for all media operations.
Choose Rust `image` with explicit PNG/JPEG/WebP features for bounded still-image
decode/encode. Use bundled FFprobe and FFmpeg executables for audio/video via
argument arrays, never a shell. Use one concrete engine service with separate
backend modules; introduce traits only at process/filesystem test seams.
Tokio provides process supervision and asynchronous queue coordination; run image
work on a bounded blocking worker. No Python or Node runtime ships with the app.

## IPC contract

`crates/contracts/src/lib.rs` defines serialization; `contracts.ts` mirrors it.
All DTO fields are required; nullable values serialize as JSON `null`, never
omitted. Struct fields use camelCase; enum values use snake_case. IDs are opaque,
random, session-local strings. They confer access only through the host's
in-memory authorized selection table. Never use a path as an ID.

| DesktopApi method | Tauri command | invoke arguments / result |
| --- | --- | --- |
| getApiVersion | `get_api_version` | `{}` → number (1) |
| selectInputs | `select_inputs` | `{}` → InputInspection[] |
| importDroppedInputs | `import_dropped_inputs` | `{paths: string[]}` → InputInspection[] |
| selectDestination | `select_destination` | `{}` → DestinationSelection or null |
| listFormats | `list_formats` | `{}` → FormatDescriptor[] |
| enqueueJob | `enqueue_job` | `{request: ConversionRequest}` → JobSnapshot |
| listJobs | `list_jobs` | `{}` → JobSnapshot[] |
| cancelJob | `cancel_job` | `{jobId: string}` → JobSnapshot |

Commands return Rust `Result<T, AppError>`; Tauri rejects with serialized
AppError. The TS adapter normalizes unexpected failures to a fixed `internal`
message, never echoes raw rejection text. Unknown request fields are rejected.
The drop adapter forwards native Tauri file-drop paths, never a browser File's
contents. Paths are untrusted: host checks canonical local regular files and
rejects URL schemes, devices, FIFOs, directories, remote mounts and UNC paths.
Mount classification requires platform checks; a `file:` URL or local-looking
path alone does not prove locality. Native dialogs execute in Rust; JS receives
only IDs and display labels. Selection is all-or-nothing for Sprint 1: if one
file fails inspection, register none from that command and return AppError.
Dialog cancellation is a successful empty array or null.

Emit `loconv://job-changed` with JobEvent, scoped to the main app window.
Listen before calling listJobs; reconcile by jobId and increasing revision so
late responses cannot regress state. Events carry full snapshots, not patches.
Ignore equal/older revisions and reject mismatched apiVersion; unsubscribe on
unmount. Snapshot revision starts at 1 and increments for every published state
or progress change. Cap jobs/session so revision never approaches u32 overflow.
Progress events are limited to 4/second; terminal changes are immediate.
listJobs is authoritative after subscription/reconnection. Do not persist jobs
or input selections; reopening starts a new session. IDs expire on exit.

The Rust and TypeScript models are deliberately small manual mirrors. Every
wire change must update both files and shared fixtures in one reviewed change.
Backend owns the canonical Rust definitions; frontend must not fork them.
Gate integration with Rust round-trip tests and TS fixture conformance/typecheck
(the latter is assigned to Antigravity). No generic `invoke<T>` cast counts as
runtime validation: validate events and command results at the TS boundary.

## Inspection and validation

InputInspection contains only normalized facts: mediaId, displayName, kind,
sizeBytes, durationMs, container, and streams. No raw FFprobe JSON, tags, EXIF,
embedded art, GPS, chapters, or thumbnails cross IPC. StreamInfo describes only
audio/video or the single image stream; skip subtitle/data/attachment streams.
Probe content rather than trusting extension. Video plus audio is video; audio
without video is audio; accepted still-image signatures use the image backend.
Reject animated/multi-page images for Sprint 1 rather than silently flattening.

sizeBytes is an unsigned decimal u64 string (JS precision); indices/dimensions
are u32 values, dimensions/sample rates/channels must be positive when present.
durationMs is finite, nonnegative or null; no NaN/Infinity. Missing dimensions or
duration are null, not zero guesses. Strings from probes are bounded and rendered
as text. Snapshot progress is null (indeterminate) or finite in [0,1]; only success
sets 1.0. Errors are present only in failed snapshots. Every semantic rule is
validated by the engine; serde/TS alone do not enforce them.

Start with a maximum 100 inputs/session, 100 jobs/session, 16 KiB selection path,
255 UTF-8 byte output basename, 30-second probe deadline, 4 MiB probe stdout,
64 KiB transient stderr, and image limits of 100 megapixels / 512 MiB decoded
allocation. Enforce limits while streaming, not after unbounded buffering.
Platform filename limits can be stricter; report invalid_request or resource_limit.
Check signature/container and decoder allowlists before conversion. Reject
playlists, concat manifests, SVG, external references and documents. Restrict
demuxers as well as protocols: `file` alone can still read other local files.

## Format registry and presets

Registry entries describe actual input/output combinations, not file extensions
alone. Return unavailable entries with tool_unavailable or unsupported_format;
never advertise a missing encoder. Backend remains authoritative at enqueue.

| FormatId | Extension | Input kinds | Backend / fixed initial preset |
| --- | --- | --- | --- |
| mp4_h264_aac | mp4 | video | FFmpeg libx264, CRF 23, medium, yuv420p; AAC 192k if audio |
| webm_vp9_opus | webm | video | FFmpeg libvpx-vp9, CRF 32, bitrate 0; libopus 128k if audio |
| mp3 | mp3 | audio, video with audio | FFmpeg libmp3lame 192k, no video |
| wav_pcm16 | wav | audio, video with audio | FFmpeg pcm_s16le, no video |
| png | png | image | image, lossless PNG |
| jpeg | jpg | image | image, quality 85; composite alpha onto white |
| webp | webp | image | image, lossless WebP |

For video map the first real video stream and optional first audio; exclude
attached artwork, subtitles and data. For audio outputs require an audio stream.
Retain source dimensions/rate where supported; reject incompatible dimensions
instead of silently cropping. VideoToolbox acceleration is deferred: predictable
software presets first, with explicit encoder capability discovery.
No resize, codec flags, GIF, HEIC, AVIF, HDR/color-management guarantee, or
animation in Sprint 1. Labels should describe these limits clearly.
Strip metadata/chapters from audio/video output; image encoding starts from
pixels without EXIF. Visually embedded information is still part of the pixels.

## Job lifecycle and output safety

One input and one destination per ConversionRequest. Backend resolves IDs,
validates the preset against inspection, rechecks the input file identity/size/
mtime before running, and fails input_unavailable if it changed. Batch UI creates
individual requests. Start with one active conversion and FIFO waiting jobs.
Enqueue performs validation before publishing a queued snapshot.

```text
queued → running → succeeded | failed
queued → cancelled
running → cancelling → cancelled
```

Terminal states are immutable; cancel of a terminal job returns its snapshot
unchanged. Unknown ID → job_not_found. Serialize cancellation and publication
under the job manager: if success has committed first, cancellation sees success;
if cancellation is accepted first, no final output can be published. A cancelled
job has error=null even if process termination exits nonzero. Partial-output
cleanup failure yields failed/internal and a fixed cleanup message. No pause,
resume, retries, or persisted recovery in this sprint. User can enqueue a new job.

Validate outputName as one basename: reject separators, `.`/`..`, controls, NUL,
Windows reserved device names, trailing spaces/dots, and mismatched extensions.
Destination IDs resolve only to explicitly selected local writable directories.
Never overwrite. Reject source/output aliasing including symlinks/hardlinks;
reserve targets against other queued jobs. Use a uniquely created temporary file
in the destination directory, explicit output muxer, then platform-appropriate
atomic publish-without-replace. An exists-check followed by ordinary rename is
insufficient. Preserve existing files if there is a race or failure. Remove only
engine-owned temporary files on cancellation/failure; never delete user inputs.

Do not hold the queue mutex across I/O. Supervisor owns child handles, drains
bounded stdout/stderr concurrently, parses `-progress pipe:1`, and sends typed
updates. On cancellation stop the process group (Windows Job Object later),
wait up to 5 seconds then force termination, await exit and clean up. On app close
ask whether to cancel outstanding work; confirmed close cancels and waits. A
forced crash may leave temporary files: disclose this, do not promise cleanup.
Conversion has no fixed duration deadline; cancellation and resource ceilings
must remain responsive. Image worker cancellation is checked before/after codec
operations; hard interruption inside a decoder is a known limitation.

## Security, packaging, and portability

Apply PRIVACY.md as a release requirement. The bundled renderer has no remote
resources, HTTP client, updater, analytics, or general JS filesystem/shell powers.
Use a restrictive release CSP (`default-src 'self'`, explicit Tauri IPC sources
only, no external connect destinations); dev-only localhost Vite allowances must
not enter release. Capabilities authorize only the main local window's required
commands/events. Custom command registration alone is not authorization: configure
Tauri app permissions and validate all arguments in Rust. Do not grant remote
origins. Process execution is accessible only from Rust.

Resolve packaged FFmpeg/FFprobe by trusted resource paths, never ambient PATH in
release. Disable network protocols at binary build time (`--disable-network`),
also apply per-input protocol/demuxer allowlists to both tools. No fallback
downloads. Missing binaries leave image features usable and audio/video disabled.
Record exact tool version, build configuration, architecture, checksums and license
notices in packaging documentation. Codec/build licensing and project license
need maintainer review before distribution; this sprint selects no project license.

Primary target: macOS aarch64 on M2, signed/notarized bundle in a later packaging
task. Future Windows and Linux use the same engine/contracts with platform
adapters for executable suffixes, path/mount validation, process groups, temp-file
publication, and native dialogs. Avoid shell scripts or Unix-only path assumptions
in engine logic. Packaging per OS is required; compatibility is not yet verified.

## Verification and implementation references

Synthetic fixtures only. Contract tests must cover enum spelling, casing, nulls,
strict requests and round trips. Engine tests must cover state races, output
conflicts, argument injection, stream selection, bounded process output, forbidden
protocols/containers, cancellation and cleanup; fake processes first, tiny
generated media for integration. UI tests use DesktopApi mock and shared JSON.
Release gate includes clean-machine offline launch and an OS network-denied
conversion matrix; static dependency review alone cannot prove offline behavior.

Design references (consulted for this architecture):
- [Tauri 2 commands and errors](https://v2.tauri.app/develop/calling-rust/)
- [Tauri events and channels](https://v2.tauri.app/develop/calling-frontend/)
- [Bundled external binaries](https://v2.tauri.app/develop/sidecar/)
- [FFmpeg protocol restrictions](https://ffmpeg.org/ffmpeg-protocols.html)
- [Rust image reader and decoder limits](https://docs.rs/image/latest/image/struct.ImageReader.html)
