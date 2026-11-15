//! Session capabilities stay in Rust. No persistence, file-content reads or workers.
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

#[derive(Default)]
pub struct Session {
    destinations: HashMap<String, PathBuf>,
    pub jobs: loconv_engine::jobs::JobRegistry,
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

    pub fn enqueue(&self, request: ConversionRequest) -> EngineResult<JobSnapshot> {
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
        // Inspection currently registers no inputs. Future backends must resolve
        // both authorized IDs and revalidate file identity before queue insertion.
        Err(errors::invalid_request())
    }
}

pub fn import(paths: Vec<String>) -> EngineResult<Vec<InputInspection>> {
    if paths.len() > inspection::MAX_INPUTS_PER_SESSION {
        return Err(AppError {
            code: ErrorCode::ResourceLimit,
            message: "The session selection limit was reached.".into(),
            retryable: false,
        });
    }
    for path in &paths {
        if path.is_empty()
            || path.len() > inspection::MAX_SELECTION_PATH_BYTES
            || path.contains('\0')
            || path.contains("://")
            || path.starts_with("file:")
            || path.starts_with("\\\\")
            || path.starts_with("//")
            || !Path::new(path).is_absolute()
        {
            return Err(errors::invalid_request());
        }
    }
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    // Fail before opening any path: locality checks and real bounded inspection
    // are prerequisites to registering inputs. All-or-nothing, with no metadata.
    Err(errors::tool_unavailable())
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
