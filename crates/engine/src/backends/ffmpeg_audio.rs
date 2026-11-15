use super::*;
use loconv_contracts::{FormatId, InputInspection};
use std::{
    io::{BufRead, BufReader},
    sync::mpsc,
};

#[derive(Debug, Clone, Default)]
pub(super) struct Capabilities {
    mp3: bool,
    wav: bool,
    demuxers: Vec<String>,
}

fn has(bytes: &[u8], name: &str, flag: char) -> bool {
    String::from_utf8_lossy(bytes).lines().any(|line| {
        let mut cols = line.split_whitespace();
        cols.next().is_some_and(|v| v.contains(flag))
            && cols.next().is_some_and(|v| v.split(',').any(|v| v == name))
    })
}
pub(super) fn discover(tool: &TrustedProbe) -> EngineResult<Capabilities> {
    TrustedProbe::verified(&tool.path, &tool.digest)?;
    let query = |arg| {
        run(
            &tool.path,
            &["-v", "error", arg],
            None,
            PROBE_DEADLINE,
            MAX_PROBE_STDOUT_BYTES,
            MAX_TRANSIENT_STDERR_BYTES,
        )
    };
    let encoders = query("-encoders")?;
    let muxers = query("-muxers")?;
    let demuxers = query("-demuxers")?;
    let protocols = query("-protocols")?;
    let protocol_text = String::from_utf8_lossy(&protocols);
    let output_protocols = protocol_text.split("Output:").nth(1).unwrap_or("");
    let input_protocols = protocol_text.split("Output:").next().unwrap_or("");
    let common = output_protocols.lines().any(|v| v.trim() == "fd")
        && input_protocols.lines().any(|v| v.trim() == "pipe")
        && has(&muxers, "null", 'E')
        && has(&encoders, "pcm_s16le", 'A');
    Ok(Capabilities {
        mp3: common && has(&encoders, "libmp3lame", 'A') && has(&muxers, "mp3", 'E'),
        wav: common && has(&encoders, "pcm_s16le", 'A') && has(&muxers, "wav", 'E'),
        demuxers: ["wav", "mp3", "flac", "ogg"]
            .into_iter()
            .filter(|d| has(&demuxers, d, 'D'))
            .map(str::to_owned)
            .collect(),
    })
}

pub struct AudioRequest<'a> {
    pub input: File,
    pub output: &'a Path,
    pub inspection: &'a InputInspection,
    pub format: &'a FormatId,
    pub probe: &'a TrustedProbe,
    pub cancel: &'a AtomicBool,
}

impl TrustedFfmpeg {
    pub fn supports(&self, format: &FormatId) -> bool {
        match format {
            FormatId::Mp3 => self.capabilities.mp3,
            FormatId::WavPcm16 => self.capabilities.wav,
            _ => false,
        }
    }

    pub fn supports_input(&self, format: &FormatId, input: &InputInspection) -> bool {
        self.supports(format)
            && input.kind == MediaKind::Audio
            && self.capabilities.demuxers.contains(&input.container)
            && input
                .streams
                .iter()
                .any(|stream| stream.kind == MediaKind::Audio)
    }

    pub fn convert(
        &self,
        request: AudioRequest<'_>,
        mut progress: impl FnMut(f64),
    ) -> EngineResult<()> {
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let owned = options
            .open(request.output)
            .map_err(|_| errors::conversion_failed())?;
        let output = request.output;
        match self.convert_to_file(request, owned, &mut progress) {
            Ok(()) => Ok(()),
            Err(err) => crate::paths::cleanup_temp(output).and(Err(err)),
        }
    }

    /// Host supplies an exclusively created engine-owned file; no path is reopened for writing.
    pub fn convert_to_file(
        &self,
        request: AudioRequest<'_>,
        owned: File,
        mut progress: impl FnMut(f64),
    ) -> EngineResult<()> {
        let AudioRequest {
            input,
            output: _,
            inspection,
            format,
            probe,
            cancel,
        } = request;
        // Rehash immediately before execution; capabilities belong to these exact bytes.
        TrustedProbe::verified(&self.executable.path, &self.executable.digest)?;
        let caps = &self.capabilities;
        let ready = match format {
            FormatId::Mp3 => caps.mp3,
            FormatId::WavPcm16 => caps.wav,
            _ => false,
        };
        if !ready
            || inspection.kind != MediaKind::Audio
            || !caps.demuxers.contains(&inspection.container)
        {
            return Err(errors::tool_unavailable());
        }
        let stream = inspection
            .streams
            .iter()
            .find(|s| s.kind == MediaKind::Audio)
            .ok_or_else(errors::unsupported_input)?;
        if cancel.load(Ordering::SeqCst) {
            return Err(errors::conversion_failed());
        }
        let args = encode_args(&inspection.container, stream.index, format);
        supervise(
            &self.executable.path,
            &args,
            input,
            &owned,
            cancel,
            inspection.duration_ms,
            &mut progress,
        )?;
        owned.sync_all().map_err(|_| errors::conversion_failed())?;
        if owned
            .metadata()
            .map_err(|_| errors::conversion_failed())?
            .len()
            == 0
        {
            return Err(errors::conversion_failed());
        }
        let muxer = if *format == FormatId::Mp3 {
            "mp3"
        } else {
            "wav"
        };
        use std::io::Seek;
        owned
            .try_clone()
            .map_err(|_| errors::internal())?
            .rewind()
            .map_err(|_| errors::internal())?;
        let details = probe.inspect_cancellable(
            owned.try_clone().map_err(|_| errors::internal())?,
            muxer,
            Some(cancel),
        )?;
        let codec = if *format == FormatId::Mp3 {
            "mp3"
        } else {
            "pcm_s16le"
        };
        if details.streams.len() != 1 || details.streams[0].codec != codec {
            return Err(errors::conversion_failed());
        }
        // Decode the complete result with strict error handling before publication.
        use std::io::SeekFrom;
        let mut verification = owned.try_clone().map_err(|_| errors::internal())?;
        verification
            .seek(SeekFrom::Start(0))
            .map_err(|_| errors::internal())?;
        let args = vec![
            "-v".into(),
            "error".into(),
            "-xerror".into(),
            "-protocol_whitelist".into(),
            "pipe".into(),
            "-format_whitelist".into(),
            muxer.into(),
            "-f".into(),
            muxer.into(),
            "-i".into(),
            "pipe:0".into(),
            "-map".into(),
            "0:a:0".into(),
            "-f".into(),
            "null".into(),
            "-progress".into(),
            "pipe:1".into(),
            "-nostats".into(),
            "pipe:3".into(),
        ];
        // null muxer writes no bytes; use the same already owned descriptor.
        let decoded_us = supervise(
            &self.executable.path,
            &args,
            verification,
            &owned,
            cancel,
            None,
            &mut |_| {},
        )?;
        if decoded_us == 0 {
            return Err(errors::conversion_failed());
        }
        Ok(())
    }
}

fn encode_args(demuxer: &str, stream: u32, format: &FormatId) -> Vec<String> {
    let (encoder, muxer) = if *format == FormatId::Mp3 {
        ("libmp3lame", "mp3")
    } else {
        ("pcm_s16le", "wav")
    };
    let mut args: Vec<String> = [
        "-v",
        "error",
        "-nostdin",
        "-xerror",
        "-max_alloc",
        "67108864",
        "-protocol_whitelist",
        "pipe",
        "-format_whitelist",
        demuxer,
        "-f",
        demuxer,
        "-probesize",
        "1048576",
        "-analyzeduration",
        "5000000",
        "-threads",
        "1",
        "-i",
        "pipe:0",
        "-map",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    args.push(format!("0:{stream}"));
    args.extend(
        [
            "-vn",
            "-sn",
            "-dn",
            "-map_metadata",
            "-1",
            "-map_metadata:s:a",
            "-1",
            "-map_chapters",
            "-1",
            "-c:a",
            encoder,
            "-threads",
            "1",
        ]
        .into_iter()
        .map(str::to_owned),
    );
    if *format == FormatId::Mp3 {
        args.extend(
            ["-b:a", "192k", "-id3v2_version", "0", "-write_id3v1", "0"]
                .into_iter()
                .map(str::to_owned),
        );
    }
    args.extend(
        [
            "-fflags",
            "+bitexact",
            "-flags:a",
            "+bitexact",
            "-progress",
            "pipe:1",
            "-stats_period",
            "0.25",
            "-nostats",
            "-f",
            muxer,
            "-protocol_whitelist",
            "fd",
            "-fd",
            "3",
            "fd:",
        ]
        .into_iter()
        .map(str::to_owned),
    );
    args
}

#[derive(Default)]
struct Progress {
    last: f64,
    emitted: Option<Instant>,
}
impl Progress {
    fn parse(&mut self, line: &str, duration: Option<f64>, now: Instant) -> Option<f64> {
        let duration = duration.filter(|v| v.is_finite() && *v > 0.0)?;
        let us = line
            .trim()
            .strip_prefix("out_time_us=")?
            .parse::<u64>()
            .ok()?;
        let value = (us as f64 / (duration * 1000.0)).min(0.999_999);
        if value <= self.last
            || self
                .emitted
                .is_some_and(|at| now.duration_since(at) < Duration::from_millis(250))
        {
            return None;
        }
        self.last = value;
        self.emitted = Some(now);
        Some(value)
    }
}

// One blocking worker owns the child; no queue mutex is held during this call.
#[cfg(unix)]
fn supervise(
    path: &Path,
    args: &[String],
    input: File,
    output: &File,
    cancel: &AtomicBool,
    duration: Option<f64>,
    progress: &mut impl FnMut(f64),
) -> EngineResult<u64> {
    let mut command = Command::new(path);
    command
        .args(args)
        .env_clear()
        .stdin(Stdio::from(input))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Keep the inherited source descriptor away from the child's stdio and fd 3.
    use std::os::fd::{AsRawFd, FromRawFd};
    // SAFETY: duplicate a live file; immediately transfer exclusive ownership to File.
    let fd = unsafe { libc::fcntl(output.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 10) };
    if fd < 0 {
        return Err(errors::internal());
    }
    let inherited = unsafe { File::from_raw_fd(fd) };
    super::process::configure(&mut command, &inherited);
    let mut child = command.spawn().map_err(|_| errors::tool_unavailable())?;
    drop(inherited);
    let stdout = child.stdout.take().ok_or_else(errors::internal)?;
    let stderr = child.stderr.take().ok_or_else(errors::internal)?;
    let exceeded = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::sync_channel(16);
    let flag = exceeded.clone();
    let reader = std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut total = 0usize;
        loop {
            let mut line = Vec::new();
            match reader.by_ref().take(1025).read_until(b'\n', &mut line) {
                Ok(0) => break,
                Ok(n) => {
                    total += n;
                    if n > 1024 || total > MAX_PROBE_STDOUT_BYTES {
                        flag.store(true, Ordering::SeqCst);
                        break;
                    }
                    if let Ok(line) = String::from_utf8(line) {
                        let _ = tx.try_send(line);
                    }
                }
                Err(_) => {
                    flag.store(true, Ordering::SeqCst);
                    break;
                }
            }
        }
    });
    let flag = exceeded.clone();
    let err_reader =
        std::thread::spawn(move || read_bounded(stderr, MAX_TRANSIENT_STDERR_BYTES, flag));
    let mut tracker = Progress::default();
    let mut elapsed_us = 0u64;
    let mut stopping = None;
    let mut failed = false;
    let result = loop {
        while let Ok(line) = rx.try_recv() {
            if let Some(value) = line
                .trim()
                .strip_prefix("out_time_us=")
                .and_then(|v| v.parse::<u64>().ok())
            {
                elapsed_us = elapsed_us.max(value);
            }
            if let Some(value) = tracker.parse(&line, duration, Instant::now()) {
                progress(value);
            }
        }
        let overflow = exceeded.load(Ordering::SeqCst)
            || output
                .metadata()
                .map(|m| m.len() > 512 * 1024 * 1024)
                .unwrap_or(true);
        if (cancel.load(Ordering::SeqCst) || overflow) && stopping.is_none() {
            failed = true;
            super::process::signal(&child, false);
            stopping = Some(Instant::now());
        }
        if stopping.is_some_and(|at: Instant| at.elapsed() >= Duration::from_millis(500)) {
            super::process::signal(&child, true);
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                // Kill any descendants which inherited pipes, even if the leader exited.
                super::process::signal(&child, true);
                break if status.success() && !failed {
                    Ok(())
                } else {
                    Err(errors::conversion_failed())
                };
            }
            Err(_) => {
                super::process::signal(&child, true);
                break Err(errors::conversion_failed());
            }
            _ => std::thread::sleep(Duration::from_millis(5)),
        }
    };
    let _ = child.wait();
    reader.join().map_err(|_| errors::internal())?;
    err_reader.join().map_err(|_| errors::internal())??;
    if exceeded.load(Ordering::SeqCst) {
        return Err(errors::resource_limit());
    }
    while let Ok(line) = rx.try_recv() {
        if let Some(value) = line
            .trim()
            .strip_prefix("out_time_us=")
            .and_then(|v| v.parse::<u64>().ok())
        {
            elapsed_us = elapsed_us.max(value);
        }
        if let Some(value) = tracker.parse(&line, duration, Instant::now()) {
            progress(value);
        }
    }
    result.map(|_| elapsed_us)
}

#[cfg(not(unix))]
fn supervise(
    _: &Path,
    _: &[String],
    _: File,
    _: &File,
    _: &AtomicBool,
    _: Option<f64>,
    _: &mut impl FnMut(f64),
) -> EngineResult<u64> {
    Err(errors::tool_unavailable())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capabilities_require_exact_encoder_and_muxer_names() {
        assert!(has(b" A....D libmp3lame description\n", "libmp3lame", 'A'));
        assert!(!has(b" A....D libmp3lame_fake\n", "libmp3lame", 'A'));
        assert!(!has(b" V....D libmp3lame\n", "libmp3lame", 'A'));
        assert!(has(b" E wav\n", "wav", 'E'));
        assert!(!has(b" D wav\n", "wav", 'E'));
        assert!(!has(b" E mp3\n", "wav", 'E'));
        let tool = TrustedFfmpeg {
            executable: TrustedProbe {
                path: PathBuf::from("/missing-synthetic"),
                digest: "0".repeat(64),
            },
            capabilities: Capabilities {
                mp3: true,
                wav: false,
                demuxers: vec![],
            },
        };
        assert!(tool.supports(&FormatId::Mp3));
        assert!(!tool.supports(&FormatId::WavPcm16));
        assert!(!tool.supports(&FormatId::Mp4H264Aac));
    }
    #[test]
    fn fixed_arguments_strip_auxiliary_streams_and_metadata() {
        for format in [FormatId::Mp3, FormatId::WavPcm16] {
            let args = encode_args("wav", 2, &format);
            for pair in [
                ["-map", "0:2"],
                ["-map_metadata", "-1"],
                ["-map_chapters", "-1"],
                ["-progress", "pipe:1"],
                ["-protocol_whitelist", "pipe"],
                ["-f", "wav"],
            ] {
                assert!(args.windows(2).any(|v| v == pair));
            }
            assert_eq!(args.last().unwrap(), "fd:");
            assert!(args.contains(&"-vn".to_owned()));
            assert!(!args.contains(&"-y".to_owned()));
        }
    }
    #[test]
    fn progress_is_real_monotonic_bounded_and_rate_limited() {
        let now = Instant::now();
        let mut tracker = Progress::default();
        for line in [
            "",
            "out_time_us=N/A",
            "out_time_us=-1",
            "out_time_us=NaN",
            "out_time_us=18446744073709551616",
            "progress=end",
        ] {
            assert!(tracker.parse(line, Some(1000.0), now).is_none());
        }
        assert!(tracker.parse("out_time_us=100000", None, now).is_none());
        assert!(tracker
            .parse("out_time_us=100000", Some(0.0), now)
            .is_none());
        assert_eq!(
            tracker.parse("out_time_us=100000", Some(1000.0), now),
            Some(0.1)
        );
        assert!(tracker
            .parse(
                "out_time_us=200000",
                Some(1000.0),
                now + Duration::from_millis(249)
            )
            .is_none());
        assert_eq!(
            tracker.parse(
                "out_time_us=200000",
                Some(1000.0),
                now + Duration::from_millis(250)
            ),
            Some(0.2)
        );
        assert!(tracker
            .parse(
                "out_time_us=100000",
                Some(1000.0),
                now + Duration::from_secs(1)
            )
            .is_none());
        assert_eq!(
            tracker.parse(
                "out_time_us=9999999",
                Some(1000.0),
                now + Duration::from_secs(2)
            ),
            Some(0.999_999)
        );
    }

    #[cfg(unix)]
    #[test]
    fn fake_encoder_cancellation_overflow_and_partial_cleanup() {
        let dir = std::env::temp_dir().join(format!("loconv-supervise-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let source = dir.join("synthetic");
        std::fs::write(&source, b"synthetic").unwrap();
        for fixture in ["fixture_hang", "fixture_stdout", "fixture_stderr"] {
            let temp = dir.join("owned.tmp");
            let output = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)
                .unwrap();
            let cancel = Arc::new(AtomicBool::new(false));
            let flag = cancel.clone();
            let thread = std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(100));
                flag.store(true, Ordering::SeqCst);
            });
            let args = vec![
                "--ignored".into(),
                "--exact".into(),
                format!("backends::ffmpeg::tests::{fixture}"),
                "--nocapture".into(),
            ];
            let start = Instant::now();
            assert!(supervise(
                &std::env::current_exe().unwrap(),
                &args,
                File::open(&source).unwrap(),
                &output,
                &cancel,
                None,
                &mut |_| {}
            )
            .is_err());
            thread.join().unwrap();
            assert!(start.elapsed() < Duration::from_secs(3));
            crate::paths::cleanup_temp(&temp).unwrap();
            assert!(!temp.exists());
            assert_eq!(std::fs::read(&source).unwrap(), b"synthetic");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "subprocess fixture only"]
    fn fixture_descendant() {
        use std::os::fd::FromRawFd;
        // SAFETY: this fixture is launched only with the supervisor-owned fd 3.
        let mut output = unsafe { File::from_raw_fd(3) };
        let child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "backends::ffmpeg::tests::fixture_hang",
                "--nocapture",
            ])
            .spawn()
            .unwrap();
        writeln!(output, "{}", child.id()).unwrap();
        // SAFETY: test process deliberately ignores TERM to exercise escalation.
        unsafe {
            libc::signal(libc::SIGTERM, libc::SIG_IGN);
        }
        let _ = child; // Parent and descendant must both be killed by the group supervisor.
        std::thread::sleep(Duration::from_secs(60));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn cancellation_forces_process_group_and_reaps_descendant() {
        let dir = std::env::temp_dir().join(format!("loconv-tree-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("owned");
        let output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        let input = File::open(&path).unwrap();
        let flag = Arc::new(AtomicBool::new(false));
        let cancel = flag.clone();
        let ready = path.clone();
        let trigger = std::thread::spawn(move || {
            let start = Instant::now();
            while std::fs::metadata(&ready).unwrap().len() == 0
                && start.elapsed() < Duration::from_secs(2)
            {
                std::thread::sleep(Duration::from_millis(5));
            }
            std::thread::sleep(Duration::from_millis(30));
            cancel.store(true, Ordering::SeqCst);
        });
        let args = vec![
            "--ignored".into(),
            "--exact".into(),
            "backends::ffmpeg::audio::tests::fixture_descendant".into(),
            "--nocapture".into(),
        ];
        let start = Instant::now();
        assert!(supervise(
            &std::env::current_exe().unwrap(),
            &args,
            input,
            &output,
            &flag,
            None,
            &mut |_| {}
        )
        .is_err());
        trigger.join().unwrap();
        assert!(start.elapsed() < Duration::from_secs(4));
        let pid: i32 = std::fs::read_to_string(&path)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let start = Instant::now();
        // SAFETY: signal 0 only checks existence; it sends no signal.
        while unsafe { libc::kill(pid, 0) } == 0 && start.elapsed() < Duration::from_secs(3) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
        crate::paths::cleanup_temp(&path).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }

    #[cfg(all(debug_assertions, feature = "dev-tools", target_os = "macos"))]
    #[test]
    #[ignore = "requires explicitly opted-in hash-pinned development FFmpeg and FFprobe"]
    fn real_audio_matrix() {
        let tool = TrustedFfmpeg::resolve(Path::new("/unused-resource-root")).unwrap();
        let probe = TrustedProbe::resolve(Path::new("/unused-resource-root")).unwrap();
        assert!(tool.supports(&FormatId::Mp3));
        assert!(tool.supports(&FormatId::WavPcm16));
        let dir = std::env::temp_dir().join(format!("loconv-real-audio-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        // Exactly one second, synthetic mono 48 kHz sine PCM. No user media.
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&96036u32.to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&48000u32.to_le_bytes());
        wav.extend_from_slice(&96000u32.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&96000u32.to_le_bytes());
        for i in 0..48000 {
            let sample =
                ((i as f64 * 440.0 * std::f64::consts::TAU / 48000.0).sin() * 8000.0) as i16;
            wav.extend_from_slice(&sample.to_le_bytes());
        }
        let original = dir.join("original.wav");
        std::fs::write(&original, &wav).unwrap();
        let cancel = AtomicBool::new(false);
        for (source_name, format, output_name) in [
            ("original.wav", FormatId::Mp3, "first.mp3"),
            ("first.mp3", FormatId::WavPcm16, "second.wav"),
            ("first.mp3", FormatId::Mp3, "third.mp3"),
            ("original.wav", FormatId::WavPcm16, "fourth.wav"),
        ] {
            let input = dir.join(source_name);
            let before = std::fs::read(&input).unwrap();
            let mut inspection =
                crate::inspection::inspect_local_file_with_probe(&input, Some(&probe)).unwrap();
            // Known generated duration exercises genuine progress even when pipe probing omits it.
            inspection.duration_ms = Some(1000.0);
            let output = dir.join(output_name);
            let mut values = Vec::new();
            tool.convert(
                AudioRequest {
                    input: File::open(&input).unwrap(),
                    output: &output,
                    inspection: &inspection,
                    format: &format,
                    probe: &probe,
                    cancel: &cancel,
                },
                |v| values.push(v),
            )
            .unwrap();
            assert!(values.windows(2).all(|v| v[0] < v[1]));
            assert!(values.iter().all(|v| *v < 1.0));
            assert_eq!(std::fs::read(&input).unwrap(), before);
            let details =
                crate::inspection::inspect_local_file_with_probe(&output, Some(&probe)).unwrap();
            assert_eq!(details.streams.len(), 1);
            assert_eq!(
                details.streams[0].codec,
                if format == FormatId::Mp3 {
                    "mp3"
                } else {
                    "pcm_s16le"
                }
            );
            // Decode to PCM with bounded output and check actual sample duration.
            let bytes = run(
                &tool.executable.path,
                &[
                    "-v",
                    "error",
                    "-xerror",
                    "-protocol_whitelist",
                    "pipe",
                    "-f",
                    if format == FormatId::Mp3 {
                        "mp3"
                    } else {
                        "wav"
                    },
                    "-i",
                    "pipe:0",
                    "-f",
                    "s16le",
                    "-c:a",
                    "pcm_s16le",
                    "pipe:1",
                ],
                Some(File::open(&output).unwrap()),
                PROBE_DEADLINE,
                200000,
                65536,
            )
            .unwrap();
            assert!((90000..110000).contains(&bytes.len()));
        }
        for (name, encoder, muxer) in [
            ("flac", "flac", "flac"),
            ("vorbis", "vorbis", "ogg"),
            ("opus", "libopus", "ogg"),
        ] {
            let bytes = run(
                &tool.executable.path,
                &[
                    "-v",
                    "error",
                    "-f",
                    "wav",
                    "-protocol_whitelist",
                    "pipe",
                    "-i",
                    "pipe:0",
                    "-c:a",
                    encoder,
                    "-strict",
                    "experimental",
                    "-ac",
                    "2",
                    "-f",
                    muxer,
                    "pipe:1",
                ],
                Some(File::open(&original).unwrap()),
                PROBE_DEADLINE,
                200000,
                65536,
            )
            .unwrap();
            let input = dir.join(format!("generated-{name}"));
            std::fs::write(&input, bytes).unwrap();
            let inspected =
                crate::inspection::inspect_local_file_with_probe(&input, Some(&probe)).unwrap();
            for format in [FormatId::Mp3, FormatId::WavPcm16] {
                let output = dir.join(format!("from-{name}-{:?}", format));
                tool.convert(
                    AudioRequest {
                        input: File::open(&input).unwrap(),
                        output: &output,
                        inspection: &inspected,
                        format: &format,
                        probe: &probe,
                        cancel: &cancel,
                    },
                    |_| {},
                )
                .unwrap();
                let inspected =
                    crate::inspection::inspect_local_file_with_probe(&output, Some(&probe))
                        .unwrap();
                assert_eq!(inspected.streams.len(), 1);
            }
        }
        // Synthetic source tags and generated artwork, never imported user media.
        let artwork = dir.join("art.jpg");
        image::RgbImage::new(8, 8).save(&artwork).unwrap();
        let tagged = dir.join("tagged.mp3");
        let fixture = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&tagged)
            .unwrap();
        let args: Vec<String> = [
            "-v",
            "error",
            "-protocol_whitelist",
            "pipe",
            "-f",
            "wav",
            "-i",
            "pipe:0",
            "-protocol_whitelist",
            "file",
            "-i",
            artwork.to_str().unwrap(),
            "-map",
            "0:a:0",
            "-map",
            "1:v:0",
            "-c:a",
            "libmp3lame",
            "-c:v",
            "copy",
            "-disposition:v",
            "attached_pic",
            "-metadata",
            "title=SYNTHETIC_PRIVATE_MARKER",
            "-metadata:s:a",
            "comment=SYNTHETIC_PRIVATE_MARKER",
            "-f",
            "mp3",
            "-protocol_whitelist",
            "fd",
            "-fd",
            "3",
            "fd:",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        supervise(
            &tool.executable.path,
            &args,
            File::open(&original).unwrap(),
            &fixture,
            &cancel,
            None,
            &mut |_| {},
        )
        .unwrap();
        let tagged_bytes = std::fs::read(&tagged).unwrap();
        let raw_probe = |path: &Path| {
            run(
                &probe.path,
                &[
                    "-v",
                    "error",
                    "-protocol_whitelist",
                    "pipe",
                    "-f",
                    "mp3",
                    "-show_streams",
                    "-show_format",
                    "-of",
                    "json",
                    "-i",
                    "pipe:0",
                ],
                Some(File::open(path).unwrap()),
                PROBE_DEADLINE,
                MAX_PROBE_STDOUT_BYTES,
                65536,
            )
            .unwrap()
        };
        let raw = raw_probe(&tagged);
        assert!(raw
            .windows(b"SYNTHETIC_PRIVATE_MARKER".len())
            .any(|v| v == b"SYNTHETIC_PRIVATE_MARKER"));
        let value: serde_json::Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(value["streams"].as_array().unwrap().len(), 2);
        let inspected =
            crate::inspection::inspect_local_file_with_probe(&tagged, Some(&probe)).unwrap();
        let stripped = dir.join("stripped.mp3");
        tool.convert(
            AudioRequest {
                input: File::open(&tagged).unwrap(),
                output: &stripped,
                inspection: &inspected,
                format: &FormatId::Mp3,
                probe: &probe,
                cancel: &cancel,
            },
            |_| {},
        )
        .unwrap();
        let raw = raw_probe(&stripped);
        assert!(!raw
            .windows(b"SYNTHETIC_PRIVATE_MARKER".len())
            .any(|v| v == b"SYNTHETIC_PRIVATE_MARKER"));
        let value: serde_json::Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(value["streams"].as_array().unwrap().len(), 1);
        assert!(value["format"]["tags"].is_null());
        assert_eq!(std::fs::read(&tagged).unwrap(), tagged_bytes);
        // A throttled real encoder exercises prompt cancellation with partial output.
        let partial = dir.join("partial.tmp");
        let owned = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&partial)
            .unwrap();
        let flag = Arc::new(AtomicBool::new(false));
        let stop = flag.clone();
        let trigger = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            stop.store(true, Ordering::SeqCst);
        });
        let mut args = encode_args("wav", 0, &FormatId::Mp3);
        let index = args.iter().position(|v| v == "-i").unwrap();
        args.insert(index, "-re".into());
        let start = Instant::now();
        assert!(supervise(
            &tool.executable.path,
            &args,
            File::open(&original).unwrap(),
            &owned,
            &flag,
            None,
            &mut |_| {}
        )
        .is_err());
        trigger.join().unwrap();
        assert!(start.elapsed() < Duration::from_secs(2));
        crate::paths::cleanup_temp(&partial).unwrap();
        assert!(!partial.exists());
        assert_eq!(std::fs::read(&original).unwrap(), wav);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
