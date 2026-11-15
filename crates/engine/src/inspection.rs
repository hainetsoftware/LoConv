//! Bounded local still-image and trusted FFprobe audio inspection.

use std::path::Path;
use std::time::Duration;

use loconv_contracts::{InputInspection, MediaKind, StreamInfo};

use crate::backends;
use crate::errors::{self, EngineResult};
use crate::paths;

/// Maximum number of selected inputs in one session.
pub const MAX_INPUTS_PER_SESSION: usize = 100;
/// Maximum UTF-8 byte length of a selected local path.
pub const MAX_SELECTION_PATH_BYTES: usize = 16 * 1024;
/// Hard deadline for one probe operation.
pub const PROBE_DEADLINE: Duration = Duration::from_secs(30);
/// Maximum bytes retained from probe stdout.
pub const MAX_PROBE_STDOUT_BYTES: usize = 4 * 1024 * 1024;
/// Maximum bytes retained from transient probe stderr.
pub const MAX_TRANSIENT_STDERR_BYTES: usize = 64 * 1024;

/// Seam for bounded media inspection.
pub trait Inspector {
    fn inspect(&self, path: &Path) -> EngineResult<InputInspection>;
}

/// Inspector that refuses every request because no trusted probe exists.
#[derive(Debug, Default, Clone, Copy)]
pub struct UnavailableInspector;

impl Inspector for UnavailableInspector {
    fn inspect(&self, _path: &Path) -> EngineResult<InputInspection> {
        Err(errors::tool_unavailable())
    }
}

/// Validate path syntax before attempting to open any file.
pub fn validate_selection_path_syntax(path: &Path) -> EngineResult<()> {
    let text = path.to_str().ok_or_else(errors::invalid_request)?;
    if path.as_os_str().is_empty()
        || text.len() > MAX_SELECTION_PATH_BYTES
        || text.contains('\0')
        || text.contains("://")
        || text.starts_with("file:")
        || text.starts_with("\\\\")
        || text.starts_with("//")
        || !path.is_absolute()
    {
        return Err(errors::invalid_request());
    }
    Ok(())
}

/// Safely inspect a local file, detecting media type from contents rather than extension.
pub fn inspect_local_file(path: &Path) -> EngineResult<InputInspection> {
    inspect_local_file_with_probe(path, None)
}

/// Shared native picker/drop inspection with a host-resolved trusted probe.
pub fn inspect_local_file_with_probe(
    path: &Path,
    probe: Option<&backends::ffmpeg::TrustedProbe>,
) -> EngineResult<InputInspection> {
    validate_selection_path_syntax(path)?;

    let metadata = std::fs::symlink_metadata(path).map_err(|_| errors::inspection_failed())?;
    if !metadata.file_type().is_file() {
        return Err(errors::invalid_request());
    }

    if !paths::is_local_path(path) {
        return Err(errors::invalid_request());
    }

    let len = metadata.len();
    if len == 0 {
        return Err(errors::inspection_failed());
    }
    if len > backends::image::MAX_DECODED_ALLOCATION_BYTES {
        return Err(errors::resource_limit());
    }

    // Read only a bounded signature; extensions never select a demuxer.
    use std::io::{Read, Seek};
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Do not follow a replaced symlink or block on a substituted FIFO.
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let mut file = options
        .open(path)
        .map_err(|_| errors::inspection_failed())?;
    let opened = file.metadata().map_err(|_| errors::inspection_failed())?;
    if !opened.is_file() || opened.len() != len {
        return Err(errors::inspection_failed());
    }
    let mut signature = [0; 12];
    let n = file
        .read(&mut signature)
        .map_err(|_| errors::inspection_failed())?;
    let sig = &signature[..n];
    let demuxer = if sig.starts_with(b"RIFF") && sig.get(8..12) == Some(b"WAVE") {
        Some("wav")
    } else if sig.starts_with(b"fLaC") {
        Some("flac")
    } else if sig.starts_with(b"OggS") {
        Some("ogg")
    } else if sig.starts_with(b"ID3") || (n >= 2 && sig[0] == 0xff && sig[1] & 0xe0 == 0xe0) {
        Some("mp3")
    } else {
        None
    };
    if let Some(demuxer) = demuxer {
        let probe = probe.ok_or_else(errors::tool_unavailable)?;
        file.rewind().map_err(|_| errors::inspection_failed())?;
        let details = probe.inspect(file, demuxer)?;
        return Ok(InputInspection {
            media_id: uuid::Uuid::new_v4().to_string(),
            display_name: path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("audio")
                .to_owned(),
            kind: MediaKind::Audio,
            size_bytes: len.to_string(),
            duration_ms: details.duration_ms,
            container: details.container,
            streams: details.streams,
        });
    }
    let details = backends::image::inspect_image_file(path)?;

    let display_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("image")
        .to_string();

    let stream = StreamInfo {
        index: 0,
        kind: MediaKind::Image,
        codec: details.format.codec_name().to_string(),
        width: Some(details.width),
        height: Some(details.height),
        sample_rate_hz: None,
        channels: None,
    };

    Ok(InputInspection {
        media_id: uuid::Uuid::new_v4().to_string(),
        display_name,
        kind: MediaKind::Image,
        size_bytes: len.to_string(),
        duration_ms: None,
        container: details.format.container_name().to_string(),
        streams: vec![stream],
    })
}

/// Image-only inspector when no trusted probe has been configured.
#[derive(Debug, Default, Clone, Copy)]
pub struct LocalInspector;

impl Inspector for LocalInspector {
    fn inspect(&self, path: &Path) -> EngineResult<InputInspection> {
        inspect_local_file(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loconv_contracts::ErrorCode;

    #[test]
    fn unavailable_inspector_fails_closed_without_reading_paths() {
        let result = UnavailableInspector.inspect(Path::new("invented/synthetic-input.bin"));
        let error = result.expect_err("no backend may inspect yet");
        assert_eq!(error.code, ErrorCode::ToolUnavailable);
        assert_eq!(error.message, errors::TOOL_UNAVAILABLE_MESSAGE);
        assert!(!error.retryable);
    }

    #[test]
    fn limits_match_architecture_baseline() {
        assert_eq!(MAX_INPUTS_PER_SESSION, 100);
        assert_eq!(MAX_SELECTION_PATH_BYTES, 16 * 1024);
        assert_eq!(PROBE_DEADLINE, Duration::from_secs(30));
        assert_eq!(MAX_PROBE_STDOUT_BYTES, 4 * 1024 * 1024);
        assert_eq!(MAX_TRANSIENT_STDERR_BYTES, 64 * 1024);
    }

    #[test]
    fn inspects_synthetic_png_successfully() {
        let dir = std::env::temp_dir().join(format!("loconv-insp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file_path = dir.join("test.png");

        let img = image::RgbaImage::from_fn(40, 30, |x, y| {
            image::Rgba([(x * 5) as u8, (y * 5) as u8, 100, 255])
        });
        img.save_with_format(&file_path, image::ImageFormat::Png)
            .unwrap();

        let inspector = LocalInspector;
        let inspection = inspector.inspect(&file_path).unwrap();

        assert_eq!(inspection.kind, MediaKind::Image);
        assert_eq!(inspection.container, "png");
        assert_eq!(inspection.display_name, "test.png");
        assert!(inspection.duration_ms.is_none());
        assert_eq!(inspection.streams.len(), 1);
        assert_eq!(inspection.streams[0].codec, "png");
        assert_eq!(inspection.streams[0].width, Some(40));
        assert_eq!(inspection.streams[0].height, Some(30));
        assert_eq!(inspection.streams[0].kind, MediaKind::Image);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_non_image_content_and_unsupported_formats() {
        let dir = std::env::temp_dir().join(format!("loconv-insp-fail-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file_path = dir.join("fake.png");

        std::fs::write(&file_path, b"not a valid png content").unwrap();

        let inspector = LocalInspector;
        let err = inspector.inspect(&file_path).unwrap_err();
        assert_eq!(err.code, ErrorCode::UnsupportedInput);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_urls_and_protocols_before_io() {
        for path in [
            "https://example.invalid/a",
            "http://example.invalid/a",
            "file:/a",
            "concat:a|b",
            "pipe:0",
            "data:audio/wav,a",
            "ftp://example.invalid/a",
            "//remote/a",
            "relative.wav",
        ] {
            assert_eq!(
                inspect_local_file(Path::new(path)).unwrap_err().code,
                ErrorCode::InvalidRequest
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn playlists_and_missing_probe_fail_closed() {
        let dir =
            std::env::temp_dir().join(format!("loconv-audio-reject-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("invented.wav");
        for bytes in [
            b"#EXTM3U\nhttps://example.invalid/secret".as_slice(),
            b"ffconcat version 1.0\nfile '/private/invented'",
            b"<MPD><BaseURL>https://example.invalid/</BaseURL></MPD>",
            b"#!/bin/sh\necho invented",
        ] {
            std::fs::write(&path, bytes).unwrap();
            assert_eq!(
                inspect_local_file(&path).unwrap_err().code,
                ErrorCode::UnsupportedInput
            );
        }
        std::fs::write(&path, b"RIFF0000WAVE").unwrap();
        assert_eq!(
            inspect_local_file(&path).unwrap_err().code,
            ErrorCode::ToolUnavailable
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rejects_empty_file() {
        let dir = std::env::temp_dir().join(format!("loconv-insp-empty-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file_path = dir.join("empty.jpg");

        std::fs::write(&file_path, b"").unwrap();

        let inspector = LocalInspector;
        let err = inspector.inspect(&file_path).unwrap_err();
        assert_eq!(err.code, ErrorCode::InspectionFailed);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
