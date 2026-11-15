# LoConv

LoConv is an offline desktop media converter built with Tauri, Rust, and React. It converts PNG, JPEG, and WebP locally. The audio and video engines are available for development tests with verified tools. Distribution builds keep those presets disabled until audited FFmpeg and FFprobe binaries compiled without networking are packaged.

## Run the macOS development app

Install Node.js and npm, Rust, and the Xcode command-line tools. Development and verification target macOS on Apple Silicon.

```sh
cd apps/desktop
npm ci
npm run tauri dev
```

## Run tests

From the repository root:

```sh
cargo test --workspace --locked
npm --prefix apps/desktop test
npm --prefix apps/desktop run build
```

Use [Orkestra](https://github.com/hainetsoftware/Orkestra) to run the isolated sprint suite, including the opt-in synthetic FFmpeg tests, and to manage development tasks. Orkestra runs from its own checkout and can target LoConv with its --repo option.

See [Architecture](ARCHITECTURE.md), [Privacy requirements](PRIVACY.md), and the [Sprint 05A](SPRINT-05A-AUDIT.md) and [Sprint 05B](SPRINT-05B-AUDIT.md) audits for background and release constraints.
