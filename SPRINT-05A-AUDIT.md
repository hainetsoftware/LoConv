# Sprint 05A — FFprobe infrastructure and audio inspection

Implemented in the assigned worktree, without commits, remote changes, personal
media access, runtime downloads, telemetry, or media transmission. IPC v1 and
frontend sources are unchanged. Image codec/conversion implementation is unchanged.
This document supersedes the older inspection/backend status in Sprint 03A notes.

## Implemented inputs and output availability

The shared native picker/drop import path now accepts ordinary WAV, FLAC, MP3,
and Ogg audio, subject to successful actual FFprobe parsing. A bounded content
signature selects one forced demuxer; neither extension nor signature alone is
proof of valid audio. Acceptable normalized codecs: MP3, FLAC, Vorbis, Opus,
PCM u8, s16le, s24le, s32le, f32le, f64le. Other codecs fail closed. Attached
artwork is omitted; real video, subtitle/data/attachment streams fail closed.
At least one audio stream is required. M4A/MP4/AAC, WebM, playlists, manifests,
scripts, arbitrary raw audio, and video inspection are outside this increment.

Normalized IPC includes only opaque UUID, local display basename, decimal byte
size, audio kind, fixed allowlisted container/codec labels, bounded numeric stream
facts, and finite nonnegative duration milliseconds or null. No tags, chapters,
artwork or arbitrary probe strings are forwarded. Invalid/unknown duration and
invalid optional sample rate/channel values become null. At most 32 streams
are accepted. Duplicate indices reject. Probe JSON is discarded after parsing.

Image formats remain available. All FFmpeg output presets remain unavailable
with tool_unavailable, irrespective of executable/encoder presence. No audio
encoding implementation or generic FFmpeg command API has been added.

## Executable trust and development usage

Tauri resolves the absolute application resource directory during setup and gives
the session a trusted FFprobe handle. Resolution never uses PATH, cwd, IPC flags,
a shell or downloads. The same provenance resolver is available for future
FFmpeg commands, without exposing execution. Release candidates are
`<resource_dir>/tools/ffprobe[.exe]` and `tools/ffmpeg[.exe]`, verified against
source-controlled OS/architecture/tool-specific SHA-256 pins. The pin table is
intentionally empty: no reviewed release binary has been supplied. Consequently
production audio inspection fails closed with tool_unavailable; images still work.
Packaging is still disabled in tauri.conf.json. Supplying files alone cannot
activate them. Packaging must supply resources plus reviewed pins and build evidence.

Local development requires all three gates:

1. Cargo development profile and debug assertions enabled (release ignores
   overrides even with the feature or release debug assertions enabled).
2. Explicit Cargo `dev-tools` feature on desktop/engine.
3. `LOCONV_ENABLE_DEV_TOOLS=1`, absolute canonical regular executable path in
   `LOCONV_DEV_FFPROBE`, and its independently reviewed hex SHA-256 in
   `LOCONV_DEV_FFPROBE_SHA256`.

Use `npm run tauri -- dev --features dev-tools` from apps/desktop after setting
those variables. Symlink executable selections, relative/remote paths, missing,
non-executable, oversized and hash-mismatched binaries reject. A conventional
installation often uses a symlink: select its canonical executable explicitly.
FFmpeg uses equivalent `LOCONV_DEV_FFMPEG` / `LOCONV_DEV_FFMPEG_SHA256` variables
when the future encoding service resolves it. No conventional installation is
claimed to be network-disabled. macOS mount-locality validation is reused;
Windows/Linux currently fail closed pending their locality adapters.

Each inspection rehashes the executable before running capability discovery and
probing. Executable files are capped at 256 MiB; hashing is bounded even if a file
grows. Capability discovery checks enabled demuxers using that executable's
`-demuxers` output. Actual codec identity comes from successful stream parsing;
no assumption about available encoders is made or advertised.

## Restrictions and threat model

Arguments are separate values owned by Rust. Input bytes come from an opened
local regular file through child stdin; no selected filename reaches FFprobe argv.
On Unix, O_NOFOLLOW/O_NONBLOCK and post-open regular-file/length checks reject
symlink/FIFO substitutions. Existing native selection syntax, locality and 512 MiB
input limits apply. No input/output URL is supplied by UI. Fixed input URL is
pipe:0. Child environment is cleared; stdin/stdout/stderr are private pipes.

Each probe forces exactly wav/flac/mp3/ogg with `-f` and `-format_whitelist`, and
allows only `pipe` via `-protocol_whitelist`. File, HTTP(S), TCP, UDP, concat,
subfile, crypto, cache and other indirect protocols are excluded. HLS, DASH,
concat and other playlist/script demuxers are never selected. A file-protocol
whitelist alone was deliberately avoided: it would permit nested local reads.
A playlist with an ordinary audio extension fails the signature gate. Even a
polyglot with a valid audio prefix is parsed only as the forced ordinary demuxer,
never autodetected as a manifest. No unsafe indirect input flags are enabled.

Only selected format/stream numeric and allowlisted identity fields are requested
with show_entries. Probe work has a 1 MiB probing budget, 5-second analysis budget,
64 MiB single-allocation limit and one requested decoder thread. These FFmpeg
options are defense in depth, not total process memory/CPU quotas. Each capability
or probe command has a 30-second wall deadline, 4 MiB stdout limit and 64 KiB
stderr limit enforced during reads. Both pipes drain concurrently. On overflow,
error or deadline the Unix child process group is killed and child reaped; reader
and writer threads are joined. Errors use fixed engine messages only. Native
imports run on blocking workers rather than the UI thread. Session imports remain
serialized and all-or-nothing; a large batch can take multiple probe deadlines.

These are restrictions on a trusted executable, not an OS sandbox. Malicious
executables, a compromised decoder, same-user mutation of trusted executable
files between hash and spawn, and hostile filesystem/mount changes are outside
this protection. Install resources in an application-controlled protected bundle;
signing, notarization and hardened binary loading remain packaging work. No
end-to-end sandbox/network-denial test or file-access tracing was performed.
Nonseekable stdin can omit duration (including WAV); null is intentional. No
accurate-duration scan, metadata/artwork decoding, or audio playback is added.
Windows process tree supervision needs a Job Object before it can be enabled;
Windows/Linux locality/dialog adapters remain unavailable.

Release requires audited, pinned FFmpeg AND FFprobe builds with network support
disabled at build time, recorded version/configuration/architecture/checksums and
license notices, bundle resources, signing/notarization, clean-machine testing,
and OS-denied network/file-access verification with synthetic malicious inputs.
The runtime whitelist does not prove complete offline isolation. See PRIVACY.md.

Reference semantics: [FFprobe options](https://ffmpeg.org/ffprobe.html),
[format whitelists and demuxers](https://ffmpeg.org/ffmpeg-formats.html), and
[protocol whitelists / pipe](https://ffmpeg.org/ffmpeg-protocols.html).

## Verification

Synthetic fixtures only. Deterministic tests use fabricated JSON and the native
Rust test executable as a fake child (large stdout, large stderr, hung process),
without shell scripts. Coverage includes executable hash/provenance errors,
URL/protocol syntax, playlists, missing tool, output caps, deadline/reaping,
malformed/truncated/deep/excessive JSON, mixed streams/artwork, numeric nulls,
fixed errors, allowed containers, and image regressions. The three ignored child
fixture functions are invoked by the supervisor test; they are not skipped checks.

Separate opt-in real-binary tests generated only 0.1 seconds of PCM silence in
test-owned temporary directories, with a deliberately incorrect extension. The
engine test and native Tauri invoke test passed with a locally hash-pinned FFprobe
9.0.2. The native test exercises both picker and drop commands, all-or-nothing
registration, no input modification, and refusal to enqueue audio output. It uses
Tauri MockRuntime and a synthetic picker; an actual WebView/macOS picker/drop UI
session was not exercised. FLAC/MP3/Ogg currently have synthetic normalization
coverage, not real-binary integration evidence. The local FFprobe configuration
has no --disable-network flag; it is development evidence only.

Required workspace fmt/check/test/clippy checks pass: 52 tests pass, with three
child fixtures ignored at top level and exercised by the supervisor test. The
feature-enabled workspace suite also passes, with two real-binary integration
tests ignored there and run separately (one pass each). Existing image inspection/conversion tests include the complete image
format/color-model matrix and native queue/cancellation/publication regressions.
Frontend checks are not required because no frontend files changed. Release
profile engine checking with dev-tools enabled verifies the compile-time override
gate builds. A feature-enabled workspace clippy check also covers opt-in tests.

## Exact Sprint 05B work

- Provide/review packaged FFmpeg and FFprobe builds, pins, resources, licenses and
  network-disabled build evidence; complete offline OS acceptance before release.
- Discover required encoders (libmp3lame and pcm_s16le), validate input/preset
  compatibility, then enable only implemented audio presets in the format registry.
- Add fixed audio encoding argument arrays with explicit stream mapping, metadata/
  chapter/artwork removal, explicit output muxers and no caller-defined flags.
- Integrate FFmpeg workers into the existing bounded job queue, source revalidation,
  progress, cancellation/process-group shutdown, engine-owned temporary output
  cleanup and atomic no-overwrite publication. The current worker is image-only.
- Add synthetic real-binary encoding/output verification, metadata stripping,
  missing encoder, hostile-input, conflict, timeout/cancellation and shutdown tests.
- Update UI/mock behavior only as necessary for validated audio presets; preserve
  IPC v1 unless a separately reviewed contract change becomes necessary.

Files changed: Cargo.lock; crates/engine/{Cargo.toml,build.rs}; engine src/backends/ffmpeg.rs,
src/inspection.rs, src/formats.rs (documentation), src/lib.rs (documentation);
apps/desktop/src-tauri/Cargo.toml and src/{lib,session,tests}.rs; this audit file.
