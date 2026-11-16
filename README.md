# LoConv

[Italiano](#italiano) · [English](#english) · [GitHub Releases](https://github.com/hainetsoftware/LoConv/releases)

## Italiano

LoConv è un convertitore desktop per immagini, audio e video. Elabora i file sul Mac, senza account, caricamenti in cloud o download di codec durante l'uso. È costruito con Tauri, Rust e React.

### Caratteristiche

| Tipo | Ingresso | Uscita |
| --- | --- | --- |
| Immagini | PNG, JPEG, WebP | PNG, JPEG, WebP senza perdita |
| Audio | WAV, MP3, FLAC, Ogg Vorbis e Ogg Opus | MP3, WAV PCM a 16 bit |
| Video | AVI, MKV, MP4, WebM | AVI (MPEG-4 Part 2/MP3), MKV e MP4 (H.264/AAC), WebM (VP9/Opus) |

- Selezione di più file, ispezione del contenuto e scelta di formato, nome e cartella di destinazione per ogni conversione.
- Coda della sessione con avanzamento e annullamento. Gli originali e i file già presenti nella destinazione sono protetti dalla sovrascrittura.
- Le conversioni video usano la prima traccia video e la prima traccia audio, se presente. I metadati sorgente e le tracce ausiliarie vengono esclusi dalle conversioni audio e video.
- FFmpeg e FFprobe 9.0.2 sono inclusi nel pacchetto macOS, compilati con la rete disabilitata e verificati tramite SHA-256. Homebrew serve solo per compilare il pacchetto.

### Scarica il DMG

Il pacchetto v0.1.0 è per **Apple Silicon (arm64) e macOS 26 o successivo**. In [GitHub Releases](https://github.com/hainetsoftware/LoConv/releases), scarica `LoConv_0.1.0_arm64.dmg` dagli allegati della v0.1.0 quando disponibile. Apri il DMG e trascina `LoConv.app` in **Applicazioni**. Il pacchetto usa una firma ad hoc per il collaudo e non è notarizzato da Apple. Per accedere agli allegati occorre avere accesso al repository.

### Compila dai sorgenti

Servono Apple Silicon con macOS 26 o successivo, gli strumenti a riga di comando di Xcode (`xcode-select --install`), Rust stable, Node.js con npm e Homebrew. Scarica il [sorgente ufficiale di FFmpeg 9.0.2](https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz) come file `.tar.xz`, quindi esegui:

```sh
git clone https://github.com/hainetsoftware/LoConv.git
cd LoConv
brew install pkgconf lame mpg123 x264 opus libvpx
npm --prefix apps/desktop ci
npm --prefix apps/desktop run package:macos -- /percorso/assoluto/ffmpeg-9.0.2.tar.xz
```

Lo script verifica l'archivio sorgente, compila e impacchetta i due strumenti e verifica le conversioni con file sintetici. Trovi l'app in `target/release/bundle/macos/LoConv.app` e il DMG in `target/release/bundle/dmg/`. Configurazione, checksum e licenze dei componenti multimediali sono in `Contents/Resources/tools/` dentro l'app.

## English

LoConv is a desktop converter for images, audio and video. It processes files on your Mac without accounts, cloud uploads or runtime codec downloads. It is built with Tauri, Rust and React.

### Features

| Type | Input | Output |
| --- | --- | --- |
| Images | PNG, JPEG, WebP | PNG, JPEG, lossless WebP |
| Audio | WAV, MP3, FLAC, Ogg Vorbis and Ogg Opus | MP3, 16-bit PCM WAV |
| Video | AVI, MKV, MP4, WebM | AVI (MPEG-4 Part 2/MP3), MKV and MP4 (H.264/AAC), WebM (VP9/Opus) |

- Select multiple files, inspect their contents, and choose the output format, filename and destination folder for each conversion.
- A session queue shows progress and supports cancellation. The app preserves originals and never overwrites existing output files.
- Video conversion uses the first video stream and the first audio stream when present. Audio and video conversions exclude source metadata and auxiliary streams.
- The macOS package bundles FFmpeg and FFprobe 9.0.2, built with networking disabled and verified with SHA-256. Homebrew is only needed to build the package.

### Download the DMG

The v0.1.0 package targets **Apple Silicon (arm64) and macOS 26 or later**. In [GitHub Releases](https://github.com/hainetsoftware/LoConv/releases), download `LoConv_0.1.0_arm64.dmg` from the v0.1.0 assets when available. Open the DMG and drag `LoConv.app` into **Applications**. The package uses an ad hoc signature for testing and is not notarized by Apple. Repository access is required to download release assets.

### Build from source

You need Apple Silicon running macOS 26 or later, the Xcode Command Line Tools (`xcode-select --install`), stable Rust, Node.js with npm, and Homebrew. Download the [official FFmpeg 9.0.2 source](https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz) as a `.tar.xz` file, then run:

```sh
git clone https://github.com/hainetsoftware/LoConv.git
cd LoConv
brew install pkgconf lame mpg123 x264 opus libvpx
npm --prefix apps/desktop ci
npm --prefix apps/desktop run package:macos -- /absolute/path/ffmpeg-9.0.2.tar.xz
```

The script checks the source archive, builds and packages both tools, and verifies conversions using synthetic media. Find the app at `target/release/bundle/macos/LoConv.app` and the DMG under `target/release/bundle/dmg/`. Build settings, checksums and media component licenses are in `Contents/Resources/tools/` inside the app.

## Sviluppo e test / Development and tests

```sh
npm --prefix apps/desktop ci
npm --prefix apps/desktop run tauri dev
cargo test --workspace --locked
npm --prefix apps/desktop test
npm --prefix apps/desktop run build
```

Per abilitare audio e video in modalità sviluppo, configura due eseguibili fidati con il comando qui sotto e riavvia l'app. La configurazione locale registra i checksum dei due strumenti.

To enable audio and video in development mode, configure two trusted executables with the command below and restart the app. The local configuration stores both tool checksums.

```sh
npm --prefix apps/desktop run tauri:setup-media -- /absolute/path/to/ffmpeg /absolute/path/to/ffprobe
```

[Architettura / Architecture](ARCHITECTURE.md) · [Privacy](PRIVACY.md) · [Orkestra](https://github.com/hainetsoftware/Orkestra)
