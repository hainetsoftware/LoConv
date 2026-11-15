# LoConv privacy requirements

These are mandatory design and release requirements. The current repository is
not yet a working converter; this document does not claim they are implemented.

## Absolute boundary

The finished application processes media entirely on the user's machine, without
runtime network dependencies. Actual user media, file contents, thumbnails,
metadata, filenames, paths, output names and derived media information must never
be transmitted to external services. This includes crash reports, analytics,
remote model prompts, bug attachments, URLs, build logs, and support requests.
No telemetry, account, cloud conversion, remote fonts, CDN assets, runtime codec
downloads, automatic update checks, or license-validation calls are permitted.

Development agents may use the internet for documentation and dependencies.
This permission does not extend to any user media or information derived from it.
Development agents must not open personal media folders, inspect user media,
or copy media details into prompts or logs. Git worktrees are not a security
sandbox; configure process/OS access accordingly. The Python orchestrator is
development tooling and may invoke cloud coding services; never treat it as a
private media-processing path.

## Local data lifecycle

| Data | Location and retention |
| --- | --- |
| Input bytes | Read locally by Rust/tools only after explicit selection; never modified |
| Paths, names, normalized metadata | Session memory and local UI only; no history persistence |
| FFprobe output / FFmpeg stderr | Bounded transient backend memory; discard after parsing; never log or forward raw text |
| Progress and errors | Local IPC only; fixed safe errors; no raw command lines or tool output |
| Temporary conversion output | Unique engine-owned file in selected output directory; remove on normal failure/cancel |
| Completed outputs | User-selected local directory; retained until the user removes them |
| Thumbnails/previews | Not implemented or cached in Sprint 1 |

Log only allowlisted event codes, durations, and opaque session IDs if diagnostic
logging is added. Never log whole DTOs, argv, filenames, metadata or exception
strings. Diagnostics are off by default and never uploaded. Do not store media
data in localStorage, IndexedDB, a job database, test snapshots or clipboard
automatically. Clear session tables and UI state on exit. Memory clearing does
not guarantee forensic erasure; OS swap/crash facilities are outside this promise.

Media is untrusted input. Reject URL/remote selections, playlists, external
references, unsupported formats and resource-limit violations. Use bundled
network-disabled tools and restricted protocols/demuxers; never interpret a
filename as a command or URL. Packaging must enforce the same policy for FFprobe
as for FFmpeg. Native file paths on remote mounts are disallowed. The app cannot
control independent backup/sync tools on the machine; disclose that selected
destinations may be observed by other local software.

Output metadata is stripped by default. This removes auxiliary tags, not
identifying information visible/audible in the media. Conversion results are not
guaranteed anonymized. A crash or forced termination may leave an engine-owned
temporary output in the selected directory. Do not implement broad automatic
directory cleanup; remove only files the current process demonstrably owns.

## Development and test policy

Automated tests use only synthetic mock media: generated color frames, generated
tones, generated pixels, fabricated JSON probe responses and invented filenames.
Do not use downloaded sample media, repository contributors' personal files, or
production captures. Generate binary fixtures into ignored `tests/generated/`
or test-owned temp directories. Commit small synthetic JSON fixtures freely with
their provenance. Never scan home directories looking for test inputs.

Dependencies and documentation may be fetched during development/build, without
media attached. Ship all executable tools/assets in the installer so installation
and first conversion do not require downloads. Audit transitive runtime behavior;
absence of an HTTP import is not sufficient proof of offline operation.

## Release acceptance evidence

1. Build and install on a clean target machine without development toolchains.
2. Deny outbound networking at OS level; select, inspect, convert and cancel
   synthetic video/audio/image inputs successfully. Verify startup has no network
   dependency and observe no attempted outbound connections from app or children.
3. Test rejected HTTP/HTTPS/file URLs, remote mounts and playlists containing
   local/remote references. No referenced files may be read as a side effect.
4. Check packaged CSP/capabilities and network-disabled FFmpeg/FFprobe build flags.
5. Exercise failure/cancel/shutdown paths; confirm no content, paths, metadata or
   names appear in application logs, persisted state, generated reports or IPC errors.
6. Confirm source files and pre-existing outputs remain byte-identical. Only
   test-owned synthetic files may be hashed/inspected by development agents.

Record platform, tool build, test procedure and outcome in release evidence.
Fail closed when locality, executable provenance or a safe format cannot be
established. Document OS-specific gaps; do not claim verified offline operation
on untested platforms.
