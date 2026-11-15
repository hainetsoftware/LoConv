# Native Tauri 2 host

This directory contains the minimal native shell owned by the backend task. It
exposes only `get_api_version` through Tauri IPC and returns the version defined
by `loconv-contracts` (currently `1`).

Current limitations:

- No inspection, conversion, job queue, cancellation, dialogs, or events.
- No FFmpeg/FFprobe or image backend; all format presets are disabled.
- Packaging is intentionally disabled (`bundle.active` is `false`).
- The frontend `/dist` assets and `npm run dev`/`npm run build` scripts are
  owned by the frontend task and are not supplied here.

Security posture:

- The main window capability grants only `allow-get-api-version`.
- Custom commands are restricted by the app permission manifest.
- No shell, filesystem, HTTP, updater, global Tauri API, or remote origin is
  exposed.
- Release CSP has no external connect destinations. Dev CSP only adds the local
  Vite origin and its inline HMR bootstrap; those allowances are not in release
  CSP.

Engine logic lives in `crates/engine` and never depends on Tauri.
