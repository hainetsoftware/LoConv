//! Fixed, bounded FFprobe execution. No shell, PATH lookup or output URLs.
use crate::{
    errors::{self, EngineResult},
    inspection::{MAX_PROBE_STDOUT_BYTES, MAX_TRANSIENT_STDERR_BYTES, PROBE_DEADLINE},
};
use loconv_contracts::{MediaKind, StreamInfo};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

/// Encoding is deliberately unavailable until Sprint 05B.
pub const fn available() -> bool {
    false
}

/// Release pins belong in source, reviewed alongside the actual packaged build.
/// Empty until audited binaries, build configurations and licenses are supplied.
const RELEASE_PINS: &[(&str, &str, &str, &str)] = &[];

/// Tool identity is fixed by Rust, never selected by UI arguments.
#[derive(Clone, Copy)]
pub enum Tool {
    Ffprobe,
    Ffmpeg,
}

/// Verified executable infrastructure for later fixed FFmpeg encoding commands.
/// No generic execution method is exposed.
#[derive(Debug, Clone)]
pub struct TrustedFfmpeg {
    _executable: TrustedProbe,
}
impl TrustedFfmpeg {
    pub fn resolve(resources: &Path) -> EngineResult<Self> {
        Ok(Self {
            _executable: TrustedProbe::resolve_tool(resources, Tool::Ffmpeg)?,
        })
    }
}

#[derive(Debug, Clone)]
pub struct TrustedProbe {
    path: PathBuf,
    digest: String,
}

impl TrustedProbe {
    /// `resources` must come from Tauri's application resource resolver, never IPC.
    pub fn resolve(resources: &Path) -> EngineResult<Self> {
        Self::resolve_tool(resources, Tool::Ffprobe)
    }

    fn resolve_tool(resources: &Path, tool: Tool) -> EngineResult<Self> {
        if !resources.is_absolute() {
            return Err(errors::tool_unavailable());
        }
        let base = match tool {
            Tool::Ffprobe => "ffprobe",
            Tool::Ffmpeg => "ffmpeg",
        };
        #[cfg(all(loconv_local_development, debug_assertions, feature = "dev-tools"))]
        if std::env::var_os("LOCONV_ENABLE_DEV_TOOLS").as_deref() == Some(std::ffi::OsStr::new("1"))
        {
            let path = std::env::var_os(format!("LOCONV_DEV_{}", base.to_ascii_uppercase()))
                .ok_or_else(errors::tool_unavailable)?;
            let digest = std::env::var(format!("LOCONV_DEV_{}_SHA256", base.to_ascii_uppercase()))
                .map_err(|_| errors::tool_unavailable())?;
            return Self::verified(Path::new(&path), &digest);
        }
        let name = if cfg!(windows) {
            format!("{base}.exe")
        } else {
            base.to_owned()
        };
        let digest = RELEASE_PINS
            .iter()
            .find(|(os, arch, name, _)| {
                *os == std::env::consts::OS && *arch == std::env::consts::ARCH && *name == base
            })
            .map(|(_, _, _, hash)| *hash)
            .ok_or_else(errors::tool_unavailable)?;
        Self::verified(&resources.join("tools").join(name), digest)
    }

    fn verified(path: &Path, digest: &str) -> EngineResult<Self> {
        if crate::inspection::validate_selection_path_syntax(path).is_err()
            || !crate::paths::is_local_path(path)
            || digest.len() != 64
            || !digest.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(errors::tool_unavailable());
        }
        #[cfg(windows)]
        if path.extension() != Some(std::ffi::OsStr::new("exe")) {
            return Err(errors::tool_unavailable());
        }
        let meta = std::fs::symlink_metadata(path).map_err(|_| errors::tool_unavailable())?;
        if !meta.is_file() || meta.len() > 256 * 1024 * 1024 {
            return Err(errors::tool_unavailable());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if meta.permissions().mode() & 0o111 == 0 {
                return Err(errors::tool_unavailable());
            }
        }
        let mut file = File::open(path).map_err(|_| errors::tool_unavailable())?;
        let mut hash = Sha256::new();
        let mut total = 0usize;
        let mut buf = [0; 32768];
        loop {
            let n = file
                .read(&mut buf)
                .map_err(|_| errors::tool_unavailable())?;
            if n == 0 {
                break;
            }
            total += n;
            if total > 256 * 1024 * 1024 {
                return Err(errors::tool_unavailable());
            }
            hash.update(&buf[..n]);
        }
        if format!("{:x}", hash.finalize()) != digest.to_ascii_lowercase() {
            return Err(errors::tool_unavailable());
        }
        Ok(Self {
            path: path
                .canonicalize()
                .map_err(|_| errors::tool_unavailable())?,
            digest: digest.to_owned(),
        })
    }

    pub(crate) fn inspect(&self, file: File, demuxer: &'static str) -> EngineResult<AudioDetails> {
        Self::verified(&self.path, &self.digest)?;
        // Discover enabled demuxers from this executable instead of assuming a build.
        let caps = run(
            &self.path,
            &["-v", "error", "-demuxers"],
            None,
            PROBE_DEADLINE,
            MAX_PROBE_STDOUT_BYTES,
            MAX_TRANSIENT_STDERR_BYTES,
        )?;
        if !String::from_utf8_lossy(&caps).lines().any(|line| {
            let mut cols = line.split_whitespace();
            cols.next().is_some_and(|flag| flag.contains('D')) && cols.next() == Some(demuxer)
        }) {
            return Err(errors::tool_unavailable());
        }
        let bytes = run(
            &self.path,
            &probe_args(demuxer),
            Some(file),
            PROBE_DEADLINE,
            MAX_PROBE_STDOUT_BYTES,
            MAX_TRANSIENT_STDERR_BYTES,
        )?;
        normalize(&bytes, demuxer)
    }
}

fn probe_args(demuxer: &str) -> Vec<&str> {
    vec!["-v", "error", "-max_alloc", "67108864", "-protocol_whitelist", "pipe", "-format_whitelist", demuxer,
        "-f", demuxer, "-probesize", "1048576", "-analyzeduration", "5000000", "-threads", "1",
        "-show_entries", "format=format_name,duration:stream=index,codec_type,codec_name,sample_rate,channels:stream_disposition=attached_pic",
        "-of", "json", "-i", "pipe:0"]
}

fn read_bounded(
    mut pipe: impl Read,
    limit: usize,
    exceeded: Arc<AtomicBool>,
) -> EngineResult<Vec<u8>> {
    let mut out = Vec::new();
    let mut buf = [0; 8192];
    loop {
        let n = pipe
            .read(&mut buf)
            .map_err(|_| errors::inspection_failed())?;
        if n == 0 {
            return Ok(out);
        }
        if out.len() + n > limit {
            exceeded.store(true, Ordering::SeqCst);
            return Err(errors::resource_limit());
        }
        out.extend_from_slice(&buf[..n]);
    }
}

fn run(
    path: &Path,
    args: &[&str],
    input: Option<File>,
    deadline: Duration,
    stdout_limit: usize,
    stderr_limit: usize,
) -> EngineResult<Vec<u8>> {
    let mut command = Command::new(path);
    command
        .args(args)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(|_| errors::tool_unavailable())?;
    let mut stdin = child.stdin.take().ok_or_else(errors::internal)?;
    let writer = std::thread::spawn(move || {
        if let Some(file) = input {
            let mut bounded = file.take(crate::backends::image::MAX_DECODED_ALLOCATION_BYTES);
            let _ = std::io::copy(&mut bounded, &mut stdin);
        }
        let _ = stdin.flush();
    });
    let exceeded = Arc::new(AtomicBool::new(false));
    let stdout = child.stdout.take().ok_or_else(errors::internal)?;
    let stderr = child.stderr.take().ok_or_else(errors::internal)?;
    let flag = exceeded.clone();
    let reader = std::thread::spawn(move || read_bounded(stdout, stdout_limit, flag));
    let flag = exceeded.clone();
    let err_reader = std::thread::spawn(move || read_bounded(stderr, stderr_limit, flag));
    let start = Instant::now();
    let result = loop {
        if exceeded.load(Ordering::SeqCst) {
            break Err(errors::resource_limit());
        }
        if start.elapsed() >= deadline {
            break Err(errors::inspection_failed());
        }
        match child.try_wait() {
            Ok(Some(status))
                if reader.is_finished() && err_reader.is_finished() && writer.is_finished() =>
            {
                break if status.success() {
                    Ok(())
                } else {
                    Err(errors::inspection_failed())
                }
            }
            Err(_) => break Err(errors::inspection_failed()),
            _ => std::thread::sleep(Duration::from_millis(5)),
        }
    };
    #[cfg(unix)]
    {
        // SAFETY: child has its own process group; terminate only that group.
        if result.is_err() {
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    let _ = writer.join();
    let stdout = reader.join().map_err(|_| errors::internal())?;
    let stderr = err_reader.join().map_err(|_| errors::internal())?;
    if exceeded.load(Ordering::SeqCst) {
        return Err(errors::resource_limit());
    }
    result?;
    stderr?;
    stdout
}

pub(crate) struct AudioDetails {
    pub container: String,
    pub duration_ms: Option<f64>,
    pub streams: Vec<StreamInfo>,
}

fn normalize(bytes: &[u8], expected: &str) -> EngineResult<AudioDetails> {
    if bytes.len() > MAX_PROBE_STDOUT_BYTES {
        return Err(errors::resource_limit());
    }
    let root: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| errors::inspection_failed())?;
    let format = root.get("format").ok_or_else(errors::inspection_failed)?;
    if !matches!(expected, "wav" | "flac" | "mp3" | "ogg")
        || format["format_name"].as_str() != Some(expected)
    {
        return Err(errors::unsupported_input());
    }
    let streams = root["streams"]
        .as_array()
        .ok_or_else(errors::inspection_failed)?;
    if streams.is_empty() || streams.len() > 32 {
        return Err(errors::resource_limit());
    }
    let mut normalized = Vec::new();
    for stream in streams {
        let kind = stream["codec_type"]
            .as_str()
            .ok_or_else(errors::inspection_failed)?;
        if kind == "video" && stream["disposition"]["attached_pic"].as_u64() == Some(1) {
            continue;
        }
        if kind != "audio" {
            return Err(errors::unsupported_input());
        }
        let codec = stream["codec_name"]
            .as_str()
            .ok_or_else(errors::inspection_failed)?;
        if !matches!(
            codec,
            "mp3"
                | "flac"
                | "vorbis"
                | "opus"
                | "pcm_s16le"
                | "pcm_s24le"
                | "pcm_s32le"
                | "pcm_u8"
                | "pcm_f32le"
                | "pcm_f64le"
        ) {
            return Err(errors::unsupported_input());
        }
        let index = stream["index"]
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(errors::inspection_failed)?;
        if normalized.iter().any(|s: &StreamInfo| s.index == index) {
            return Err(errors::inspection_failed());
        }
        let rate = stream["sample_rate"]
            .as_str()
            .and_then(|s| s.parse::<u32>().ok())
            .filter(|v| *v > 0 && *v <= 768000);
        let channels = stream["channels"]
            .as_u64()
            .filter(|v| *v > 0 && *v <= 64)
            .map(|v| v as u32);
        normalized.push(StreamInfo {
            index,
            kind: MediaKind::Audio,
            codec: codec.to_owned(),
            width: None,
            height: None,
            sample_rate_hz: rate,
            channels,
        });
    }
    if normalized.is_empty() {
        return Err(errors::unsupported_input());
    }
    let duration_ms = format["duration"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .map(|v| v * 1000.0)
        .filter(|v| v.is_finite() && *v >= 0.0);
    Ok(AudioDetails {
        container: expected.to_owned(),
        duration_ms,
        streams: normalized,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use loconv_contracts::ErrorCode;
    use serde_json::json;

    fn response() -> serde_json::Value {
        json!({"format":{"format_name":"wav","duration":"1.25","tags":{"title":"PRIVATE / invented"}},
            "streams":[{"index":0,"codec_type":"audio","codec_name":"pcm_s16le","sample_rate":"48000","channels":2,"tags":{"secret":"PRIVATE"}}]})
    }
    fn parse(value: &serde_json::Value) -> EngineResult<AudioDetails> {
        normalize(&serde_json::to_vec(value).unwrap(), "wav")
    }
    #[test]
    fn normalization_excludes_metadata_and_artwork() {
        let mut value = response();
        value["streams"].as_array_mut().unwrap().push(json!({"index":1,"codec_type":"video","codec_name":"mjpeg","disposition":{"attached_pic":1}}));
        let result = parse(&value).unwrap();
        assert_eq!(result.container, "wav");
        assert_eq!(result.duration_ms, Some(1250.0));
        assert_eq!(result.streams.len(), 1);
        assert!(!serde_json::to_string(&result.streams)
            .unwrap()
            .contains("PRIVATE"));
        value["streams"][1]["disposition"]["attached_pic"] = json!(0);
        let err = parse(&value).err().unwrap();
        assert_eq!(err.code, ErrorCode::UnsupportedInput);
        assert!(!serde_json::to_string(&err).unwrap().contains("PRIVATE"));
    }
    #[test]
    fn durations_and_numeric_fields_are_bounded() {
        for duration in [
            json!(null),
            json!("N/A"),
            json!("NaN"),
            json!("inf"),
            json!("-1"),
            json!("1e308"),
            json!(42),
        ] {
            let mut value = response();
            value["format"]["duration"] = duration;
            assert_eq!(parse(&value).unwrap().duration_ms, None);
        }
        let mut value = response();
        value["streams"][0]["sample_rate"] = json!("9999999999999");
        value["streams"][0]["channels"] = json!(0);
        let result = parse(&value).unwrap();
        assert_eq!(result.streams[0].sample_rate_hz, None);
        assert_eq!(result.streams[0].channels, None);
    }
    #[test]
    fn invalid_truncated_excessive_and_unsafe_responses_fail() {
        for bytes in [b"{".as_slice(), b"null", b"{}", b"[]", b"{\"streams\":["] {
            assert!(normalize(bytes, "wav").is_err());
        }
        assert_eq!(
            normalize(&vec![b' '; MAX_PROBE_STDOUT_BYTES + 1], "wav")
                .err()
                .unwrap()
                .code,
            ErrorCode::ResourceLimit
        );
        let mut value = response();
        value["streams"] = json!(vec![value["streams"][0].clone(); 33]);
        assert_eq!(parse(&value).err().unwrap().code, ErrorCode::ResourceLimit);
        for format in [
            "concat",
            "hls",
            "dash",
            "mov,mp4,m4a,3gp,3g2,mj2",
            "PRIVATE",
        ] {
            let mut value = response();
            value["format"]["format_name"] = json!(format);
            assert_eq!(
                parse(&value).err().unwrap().code,
                ErrorCode::UnsupportedInput
            );
        }
        let mut value = response();
        value["streams"][0]["codec_name"] = json!("PRIVATE");
        assert!(parse(&value).is_err());
    }
    #[test]
    fn supported_audio_containers_normalize_from_probe_results() {
        for (container, codec) in [
            ("wav", "pcm_s16le"),
            ("flac", "flac"),
            ("mp3", "mp3"),
            ("ogg", "opus"),
            ("ogg", "vorbis"),
        ] {
            let mut value = response();
            value["format"]["format_name"] = json!(container);
            value["streams"][0]["codec_name"] = json!(codec);
            let details = normalize(&serde_json::to_vec(&value).unwrap(), container).unwrap();
            assert_eq!(details.container, container);
            assert_eq!(details.streams[0].codec, codec);
        }
        let mut value = response();
        value["streams"]
            .as_array_mut()
            .unwrap()
            .push(json!({"index":2,"codec_type":"audio","codec_name":"pcm_s24le"}));
        assert_eq!(parse(&value).unwrap().streams.len(), 2);
        value["streams"][1]["index"] = json!(0);
        assert!(parse(&value).is_err());
        assert!(normalize(&[vec![b'['; 200], vec![b']'; 200]].concat(), "wav").is_err());
    }

    #[test]
    fn command_contains_only_fixed_pipe_input_and_allowlists() {
        for demuxer in ["wav", "flac", "mp3", "ogg"] {
            let args = probe_args(demuxer);
            assert!(args
                .windows(2)
                .any(|v| v == ["-protocol_whitelist", "pipe"]));
            assert!(args.windows(2).any(|v| v == ["-format_whitelist", demuxer]));
            assert!(args.windows(2).any(|v| v == ["-f", demuxer]));
            assert_eq!(&args[args.len() - 2..], ["-i", "pipe:0"]);
            assert!(!args.join(" ").contains("tags"));
        }
    }
    #[test]
    fn resolver_rejects_unpinned_missing_relative_and_invalid_executables() {
        assert!(TrustedProbe::resolve(Path::new("relative")).is_err());
        let path = std::env::current_exe().unwrap();
        assert!(TrustedProbe::verified(&path, &"0".repeat(64)).is_err());
        assert!(TrustedProbe::verified(Path::new("ffprobe"), &"0".repeat(64)).is_err());
        assert!(TrustedProbe::verified(
            &path.with_file_name("missing-synthetic-ffprobe"),
            &"0".repeat(64)
        )
        .is_err());
        assert!(TrustedProbe::verified(&path, "invalid").is_err());
        let bytes = std::fs::read(&path).unwrap();
        let hash = format!("{:x}", Sha256::digest(&bytes));
        #[cfg(target_os = "macos")]
        assert!(TrustedProbe::verified(&path, &hash).is_ok());
        let dir = std::env::temp_dir().join(format!("loconv-tool-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let invalid = dir.join("invalid");
        std::fs::write(&invalid, b"not executable").unwrap();
        assert!(TrustedProbe::verified(
            &invalid,
            &format!("{:x}", Sha256::digest(b"not executable"))
        )
        .is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&path, dir.join("link")).unwrap();
            assert!(TrustedProbe::verified(&dir.join("link"), &hash).is_err());
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
    // Native test executable fixtures: no shell scripts or external test media.
    #[test]
    #[ignore = "subprocess fixture only"]
    fn fixture_stdout() {
        let _ = std::io::stdout().write_all(&vec![b'x'; 100_000]);
    }
    #[test]
    #[ignore = "subprocess fixture only"]
    fn fixture_stderr() {
        let _ = std::io::stderr().write_all(&vec![b'x'; 100_000]);
    }
    #[test]
    #[ignore = "subprocess fixture only"]
    fn fixture_hang() {
        std::thread::sleep(Duration::from_secs(60));
    }
    #[test]
    fn supervised_process_limits_and_deadlines() {
        let exe = std::env::current_exe().unwrap();
        for fixture in ["fixture_stdout", "fixture_stderr", "fixture_hang"] {
            let name = format!("backends::ffmpeg::tests::{fixture}");
            let start = Instant::now();
            let err = run(
                &exe,
                &["--ignored", "--exact", &name, "--nocapture"],
                None,
                Duration::from_millis(300),
                4096,
                4096,
            )
            .unwrap_err();
            assert_eq!(
                err.code,
                if fixture == "fixture_hang" {
                    ErrorCode::InspectionFailed
                } else {
                    ErrorCode::ResourceLimit
                }
            );
            assert!(start.elapsed() < Duration::from_secs(5));
            assert!(!err.message.contains("xxxxxxxx"));
        }
        assert_eq!(
            run(
                &exe.with_file_name("missing-synthetic-tool"),
                &[],
                None,
                Duration::from_secs(1),
                100,
                100
            )
            .unwrap_err()
            .code,
            ErrorCode::ToolUnavailable
        );
    }

    #[cfg(all(debug_assertions, feature = "dev-tools", target_os = "macos"))]
    #[test]
    #[ignore = "real binary integration; requires explicit pinned development FFprobe"]
    fn real_synthetic_wav_inspection() {
        let probe = TrustedProbe::resolve(Path::new("/unused-resource-root")).unwrap();
        let dir = std::env::temp_dir().join(format!("loconv-audio-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        // 0.1 seconds of generated silence, PCM mono 8 kHz, no metadata.
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&1636u32.to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&8000u32.to_le_bytes());
        wav.extend_from_slice(&16000u32.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&1600u32.to_le_bytes());
        wav.resize(1644, 0);
        let path = dir.join("generated.wrong-extension");
        std::fs::write(&path, wav).unwrap();
        let result = crate::inspection::inspect_local_file_with_probe(&path, Some(&probe)).unwrap();
        assert_eq!(result.kind, MediaKind::Audio);
        assert_eq!(result.container, "wav");
        assert_eq!(result.streams[0].sample_rate_hz, Some(8000));
        assert_eq!(result.streams[0].channels, Some(1));
        assert!(!available());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
