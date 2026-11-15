# Sprint 05B — Real offline audio conversion engine

Implemented in this assigned worktree without commits, pushes, merges, remote
changes, user-media access, runtime downloads, or media uploads. IPC v1 DTOs,
commands and event shape remain unchanged. Frontend sources are unchanged.

## Implemented paths and capability results

The existing single FIFO worker now handles both audio and images. Fixed audio
presets are MP3 (`libmp3lame`, 192 kbps, explicit `mp3` muxer) and WAV
(`pcm_s16le`, explicit `wav` muxer). Both map only the first inspected audio
stream, disable video/subtitles/data, remove global and audio-stream metadata
and chapters, and enable bitexact output. MP3 ID3 output is disabled. Artwork
and attachments are never mapped. No frontend-defined codec arguments exist.

Actual real-binary tests passed WAV → MP3, MP3 → WAV, MP3 → MP3, WAV → WAV,
and FLAC, Ogg Vorbis and Ogg Opus → both MP3 and WAV. Other previously allowlisted
PCM variants depend on actual decoder compatibility; they were not individually
matrix-tested. Video, AAC/M4A/MP4, playlists and arbitrary raw audio remain disabled.
PNG/JPEG/WebP conversions retain their existing presets and regression matrix.

Capabilities are read from the exact hash-verified FFmpeg executable using
bounded `-encoders`, `-muxers`, `-demuxers` and `-protocols` queries. Exact table
names are required. Each advertised preset requires its encoder and muxer;
verification additionally needs PCM encoding/null muxing, pipe input and seekable
fd output. Enqueue checks the inspected container against that binary's enabled
demuxers. FFprobe alone never enables audio output. Missing decoder support or
incompatible audio fails safely at actual execution; there is no simulated success.
Executable bytes are rehashed before each conversion and each probe operation.

The tested development installation reports libmp3lame and pcm_s16le encoders,
mp3/wav/null muxers, wav/mp3/flac/ogg demuxers, pipe input and fd output. Both
presets are therefore available with that explicit configuration. A default
unconfigured session exposes only the three image presets. Release pins remain
empty, so production audio inspection and encoding remain unavailable.

## Trust, process supervision and privacy

The Sprint 05A resolver is reused. No PATH lookup, shell, IPC executable path,
runtime service or download is introduced. Development tools require a development
profile, debug assertions, `dev-tools`, `LOCONV_ENABLE_DEV_TOOLS=1`, absolute
executable paths and matching SHA-256 pins. Use this override only for locally
generated synthetic integration tests. It is ignored by release builds.

Input is an already-opened, read-only source handle supplied through child stdin.
Forced wav/mp3/flac/ogg demuxers and pipe-only input protocols prohibit arbitrary
file/network references. Output is a preopened, exclusively created temporary
file inherited as fd 3; the fd output protocol permits seeking to finalize WAV
headers. Selected source/output filenames do not appear in production FFmpeg argv.
See the [FFmpeg fd protocol documentation](https://www.ffmpeg.org/ffmpeg-protocols.html#fd).
The child environment is cleared. Raw probe output and stderr remain transient
and bounded; only normalized allowlisted facts and fixed errors reach IPC.

A blocking supervisor owns each child without holding the session/queue mutex.
Independent readers drain stdout/stderr, capped at 4 MiB cumulative progress and
64 KiB stderr. Progress lines are capped at 1 KiB and passed through a bounded
16-entry channel, with excess updates dropped. `out_time_us` yields monotonically
increasing progress only with finite positive known duration, at intervals of at
least 250 ms. Unknown duration stays null; malformed values are ignored. Progress
never reaches 1 until a successful publication. Long conversions that exceed the
cumulative progress budget fail closed rather than buffer indefinitely.

Unix process handling lives in its own module. Each encoder has its own process
group. Cancellation is polled every 5 ms; SIGTERM is followed by SIGKILL after
500 ms if needed. The leader is waited/reaped, remaining group members are killed,
and pipe readers are joined. There is no fixed encoding wall-time deadline.
A 512 MiB child file-size rlimit and output-size monitoring bound temporary output.
Probe/capability operations retain the existing 30-second deadlines; verification
probes now also observe cancellation. A complete strict decode to the null muxer
must succeed and report positive decoded time before output may publish.

## Queue, identity, cancellation and publication

Opaque selections remain in session memory. Import captures source device/inode,
size and mtime, checks identity after inspection, and stores a canonical path.
Enqueue and worker startup reopen with O_NOFOLLOW/O_NONBLOCK and compare the
opened regular file's identity/size/mtime. Missing, replaced or changed sources
fail with fixed errors. Existing basename and extension validation is preserved.
Existing targets, including dangling symlinks and hardlinks, are rejected, and
queued targets remain reserved until terminal handling.

The host exclusively creates a unique temporary file in the output directory
with Unix mode 0600 before either backend receives its handle. It records whether
creation succeeded, so a failed create does not authorize deletion of an existing
file. Image encoding gained an owned-file entry point without changing its codecs.
Only owned temporary paths are cleaned. The completed temporary file's captured
identity is rechecked before publication. macOS uses `renamex_np(RENAME_EXCL)`,
which atomically publishes and consumes the temporary name without replacing an
existing target. A target appearing after enqueue produces output_conflict.
Cleanup failures produce failed/internal with a fixed cleanup message.

Cancellation and publication are serialized under the same session lock. Accepted
cancellation prevents publication; a committed success remains immutable. Repeated
cancellation does not invent revisions. Registry updates ignore stale revisions
and cannot mutate terminal snapshots. Audio and images use the same worker gate;
its idle transition now occurs under the enqueue lock, fixing a lost-wakeup race.
Reservations are released on terminal handling, and the queue continues after
failed and cancelled jobs. Running begins with null progress rather than an
invented percentage.

## Test evidence

Platform: Darwin arm64; Rust 1.99.1. Synthetic media only: locally generated PCM
sine/silence, generated image pixels, fabricated JSON and native fake processes.
No downloaded fixtures, recordings or user libraries were accessed.

Required commands passed:

- `cargo fmt --all -- --check`
- `cargo check --workspace`
- `cargo test --workspace`: 60 passed, 0 failed; four subprocess fixtures ignored
  at the top level and invoked by their owning supervisor tests.
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace --features dev-tools`: 60 passed, 0 failed; eight
  top-level ignores (four exercised child fixtures and four real tests run separately).
- Feature-enabled workspace clippy with `--features dev-tools` also passed.
- `git diff --check` passed.

Frontend typecheck/tests/build were not run because frontend files did not change.

The opt-in real test command was `cargo test --workspace --features dev-tools
real_ -- --ignored`, with the five explicit development environment variables
configured. Four real tests passed: the existing WAV inspection test, native
picker/drop inspection test, native mixed audio/image queue test, and expanded
real encoding matrix. These are actual FFmpeg/FFprobe runs, not mocks.

Both tools report version 9.0.2, development installation package 9.0.2_1:

- FFmpeg SHA-256: `7f6028378534aa10a2dfa07d807ddcc0d18c869bf5c2f64c89efcc18ee88c4ee`
- FFprobe SHA-256: `bfe676bfc3a889bebe63af07c71a926a6167f568c7a932a79c2f1fe51ae6d5a1`

The core four-path matrix decodes each output to PCM and checks one-second duration
sanity (90,000–110,000 decoded bytes for mono 48 kHz s16le). Extra FLAC/Vorbis/Opus
paths pass production output verification. A generated tagged MP3 with generated
JPEG artwork is checked to contain tags/two streams before conversion; the result
has one audio stream, no source marker and no format tags. All input byte arrays
are preserved. The real test also cancels a deliberately throttled FFmpeg process
and removes its partial output. Native IPC tests demonstrate available presets,
opaque import, mixed FIFO jobs, terminal success at 1.0, released reservations and
immutable success on cancellation. An actual WebView/dialog UI session was not run.

Deterministic/fake tests cover exact capability parsing, missing/hash-invalid tools,
malformed/oversized progress, unknown duration, monotonic/rate-limited progress,
stdout/stderr budgets, hung children, forced group termination and descendant
cleanup, temporary cleanup failure, symlink/hardlink publication conflicts, replaced
sources, malicious basenames, concurrent enqueue/cancel, queued cancellation,
reservation conflicts, worker restart and queue continuation after a failed source.
All existing image inspection/conversion/color-model and native regressions pass.
Earlier failing development runs found import error compatibility, nonseekable WAV
headers and a nonseekable tagged fixture; these were corrected before the final runs.
The local tool lacks libvorbis; the test-only Vorbis source generator uses the
native experimental Vorbis encoder in stereo. Production presets do not use it.

## Release blockers and limits before Sprint 06

No fully reviewed, pinned network-disabled executable pair is supplied. Production
therefore stays unavailable. No release encoding or OS-network-denied acceptance
test is counted as passed. Development binary hashing proves the configured bytes,
not network-disabled compilation or supply-chain review. The runtime protocol
restrictions are defense in depth, not an OS sandbox or proof against decoder bugs.

Release still needs audited architecture-specific FFmpeg/FFprobe builds with
network disabled, recorded versions/configurations/checksums/licenses, reviewed
source pins, bundled resources, signing/notarization, clean-machine installation,
and network-denied/file-access tracing with synthetic malicious fixtures. Packaging
is still disabled in the existing Tauri configuration. Dynamic dependencies and
codec/project licensing also need review.

Known boundaries: input file mutation during encoding, hostile same-user filesystem
or executable replacement between verification and use, decoder exploitation,
resource-directory compromise and process-group escape by a malicious executable
are outside this supervisor's protection. CPU/total memory are not OS sandboxed.
Unknown pipe-probed durations remain indeterminate. A crash/forced app termination
may leave a temporary file or child; normal host close/shutdown coordination is
still a separate integration requirement. No broad directory cleanup is added.
Image codec calls remain cooperatively cancellable between decode/encode phases.

macOS arm64 is the tested encoding platform. Windows needs Job Objects, bounded
termination, handle inheritance, destination locality and atomic publication adapters.
Linux needs locality/packaging acceptance and process/publication verification.
Neither future platform is claimed as supported. The non-macOS hardlink publication
fallback can leave a published target if unlink cleanup fails; it reports failure
and audio resolution remains disabled there.

Before Sprint 06, finish release/offline/shutdown gates, add platform CI and stronger
OS isolation, then implement tightly inspected video containers, real video stream
selection, fixed video encoder/muxer discovery, explicit pixel/audio mapping and
synthetic video conversion/cancel/metadata tests. Keep video presets disabled until
those implementations and capabilities exist.

## Exact modified files

- `apps/desktop/src-tauri/src/lib.rs`
- `apps/desktop/src-tauri/src/session.rs`
- `apps/desktop/src-tauri/src/tests.rs`
- `crates/engine/src/backends/ffmpeg.rs`
- `crates/engine/src/backends/ffmpeg_audio.rs` (new)
- `crates/engine/src/backends/ffmpeg_process.rs` (new)
- `crates/engine/src/backends/image.rs`
- `crates/engine/src/backends/mod.rs`
- `crates/engine/src/errors.rs`
- `crates/engine/src/formats.rs`
- `crates/engine/src/jobs.rs`
- `crates/engine/src/paths.rs`
- `SPRINT-05B-AUDIT.md` (new)
