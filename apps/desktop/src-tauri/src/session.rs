//! Session state stays in Rust. No persistence across restarts; all selections expire on exit.
use loconv_contracts::*;
use loconv_engine::{
    errors::{self, EngineResult},
    formats, inspection, paths,
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use uuid::Uuid;

/// In-memory record of an inspected and authorized local input file.
#[derive(Debug, Clone)]
pub struct InputRecord {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub mtime: std::time::SystemTime,
    pub inspection: InputInspection,
}

#[derive(Default)]
pub struct Session {
    pub probe: Option<loconv_engine::backends::ffmpeg::TrustedProbe>,
    pub inputs: HashMap<String, InputRecord>,
    pub destinations: HashMap<String, PathBuf>,
    pub jobs: loconv_engine::jobs::JobRegistry,
    pub reserved_targets: HashMap<PathBuf, String>,
    pub job_destinations: HashMap<String, PathBuf>,
}

impl Session {
    pub fn destination(
        &mut self,
        selected: Option<ValidatedDestination>,
    ) -> EngineResult<Option<DestinationSelection>> {
        let Some(selected) = selected else {
            return Ok(None);
        };
        if self.destinations.len() >= 100 {
            return Err(AppError {
                code: ErrorCode::ResourceLimit,
                message: "The session selection limit was reached.".into(),
                retryable: false,
            });
        }
        let ValidatedDestination { path, display_name } = selected;
        let destination_id = Uuid::new_v4().to_string();
        self.destinations.insert(destination_id.clone(), path);
        Ok(Some(DestinationSelection {
            destination_id,
            display_name,
        }))
    }

    pub fn import(&mut self, paths: Vec<String>) -> EngineResult<Vec<InputInspection>> {
        if paths.len() > inspection::MAX_INPUTS_PER_SESSION {
            return Err(AppError {
                code: ErrorCode::ResourceLimit,
                message: "The session selection limit was reached.".into(),
                retryable: false,
            });
        }
        if self.inputs.len() + paths.len() > inspection::MAX_INPUTS_PER_SESSION {
            return Err(AppError {
                code: ErrorCode::ResourceLimit,
                message: "The session selection limit was reached.".into(),
                retryable: false,
            });
        }
        if paths.is_empty() {
            return Ok(Vec::new());
        }

        // 1. Inspect all files first (all-or-nothing: if any fails, none are registered)
        let mut staged: Vec<InputRecord> = Vec::with_capacity(paths.len());
        for path_str in &paths {
            let path = Path::new(path_str);
            let inspection = inspection::inspect_local_file_with_probe(path, self.probe.as_ref())?;
            let meta = std::fs::symlink_metadata(path).map_err(|_| errors::inspection_failed())?;
            let mtime = meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            staged.push(InputRecord {
                path: path.to_path_buf(),
                size_bytes: meta.len(),
                mtime,
                inspection,
            });
        }

        // 2. Commit all staged inputs into session memory
        let mut results = Vec::with_capacity(staged.len());
        for record in staged {
            results.push(record.inspection.clone());
            self.inputs
                .insert(record.inspection.media_id.clone(), record);
        }

        Ok(results)
    }

    pub fn enqueue(&mut self, request: ConversionRequest) -> EngineResult<JobSnapshot> {
        let spec = formats::find(&request.format_id).ok_or_else(errors::unsupported_format)?;
        paths::validate_output_name(&request.output_name, spec.extension)?;
        if request.media_id.is_empty() || request.destination_id.is_empty() {
            return Err(errors::invalid_request());
        }

        // Never create a queued/successful job without a real runnable backend.
        match spec.descriptor().unavailable_reason {
            Some(ErrorCode::ToolUnavailable) => return Err(errors::tool_unavailable()),
            Some(_) => return Err(errors::unsupported_format()),
            None => (),
        }

        // 1. Resolve authorized IDs
        let input_entry = self
            .inputs
            .get(&request.media_id)
            .ok_or_else(errors::invalid_request)?;
        let destination_path = self
            .destinations
            .get(&request.destination_id)
            .ok_or_else(errors::invalid_request)?
            .clone();

        // 2. Validate format compatibility with media kind
        if !spec.input_kinds.contains(&input_entry.inspection.kind) {
            return Err(errors::unsupported_format());
        }

        // 3. Revalidate input file identity/size/mtime
        let meta = std::fs::symlink_metadata(&input_entry.path)
            .map_err(|_| errors::input_unavailable())?;
        if !meta.file_type().is_file() || meta.len() != input_entry.size_bytes {
            return Err(errors::input_unavailable());
        }
        if let Ok(current_mtime) = meta.modified() {
            if current_mtime != input_entry.mtime {
                return Err(errors::input_unavailable());
            }
        }

        // 4. Target path checks and reservations
        let final_target_path = destination_path.join(&request.output_name);

        // Source / output aliasing check
        if input_entry.path == final_target_path {
            return Err(errors::output_conflict());
        }
        if let (Ok(c_in), Ok(c_out)) = (
            input_entry.path.canonicalize(),
            final_target_path.canonicalize(),
        ) {
            if c_in == c_out {
                return Err(errors::output_conflict());
            }
        }

        // Existing destination file check (never overwrite!)
        if final_target_path.exists() {
            return Err(errors::output_conflict());
        }

        // Reservation check against other pending/active jobs
        if self.reserved_targets.contains_key(&final_target_path) {
            return Err(errors::output_conflict());
        }

        // 5. Register queued job
        let snapshot = self.jobs.register_queued(request)?;
        self.reserved_targets
            .insert(final_target_path, snapshot.job_id.clone());
        self.job_destinations
            .insert(snapshot.job_id.clone(), destination_path);

        Ok(snapshot)
    }
}

fn local_directory(path: &Path) -> EngineResult<PathBuf> {
    let text = path.to_str().ok_or_else(errors::invalid_request)?;
    if !path.is_absolute()
        || text.len() > inspection::MAX_SELECTION_PATH_BYTES
        || text.contains('\0')
        || text.starts_with("//")
        || text.contains("://")
    {
        return Err(errors::invalid_request());
    }
    #[cfg(target_os = "macos")]
    {
        use std::{ffi::CString, os::unix::ffi::OsStrExt};
        let canonical = path.canonicalize().map_err(|_| errors::invalid_request())?;
        let cpath = CString::new(canonical.as_os_str().as_bytes())
            .map_err(|_| errors::invalid_request())?;
        let mut stat = std::mem::MaybeUninit::<libc::statfs>::uninit();
        // SAFETY: valid NUL-terminated path and writable statfs storage. Only
        // inspect fields after statfs reports successful initialization.
        let local = unsafe {
            libc::statfs(cpath.as_ptr(), stat.as_mut_ptr()) == 0
                && stat.assume_init().f_flags & libc::MNT_LOCAL as u32 != 0
        };
        if !local || !canonical.is_dir() {
            return Err(errors::invalid_request());
        }
        // SAFETY: cpath is valid for the duration of the call. No file created.
        if unsafe { libc::access(cpath.as_ptr(), libc::W_OK | libc::X_OK) } != 0 {
            return Err(AppError {
                code: ErrorCode::PermissionDenied,
                message: "The selected folder is not writable.".into(),
                retryable: false,
            });
        }
        Ok(canonical)
    }
    #[cfg(not(target_os = "macos"))]
    Err(errors::tool_unavailable())
}

/// Constructed only after local directory checks, before taking the session lock.
pub struct ValidatedDestination {
    path: PathBuf,
    display_name: String,
}

pub fn prepare_destination(path: PathBuf) -> EngineResult<ValidatedDestination> {
    let path = local_directory(&path)?;
    let display_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("Selected folder")
        .to_owned();
    Ok(ValidatedDestination { path, display_name })
}
