//! Pure output-name and path-string validation.
//!
//! Validates basenames and source identity and publishes without replacing targets.

use std::path::Path;

use crate::errors::{self, EngineResult};

/// Maximum UTF-8 bytes for an output basename, matching the architecture limit.
pub const MAX_OUTPUT_NAME_BYTES: usize = 255;

const RESERVED_DEVICE_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

fn has_forbidden_characters(name: &str) -> bool {
    name.chars().any(|ch| {
        ch.is_control() || matches!(ch, '/' | '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*')
    })
}

fn is_reserved_device_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name);
    RESERVED_DEVICE_NAMES
        .iter()
        .any(|reserved| stem.eq_ignore_ascii_case(reserved))
}

fn has_valid_extension(name: &str, expected_extension: &str) -> bool {
    let Some((stem, extension)) = name.rsplit_once('.') else {
        return false;
    };
    !stem.is_empty()
        && !expected_extension.is_empty()
        && extension.eq_ignore_ascii_case(expected_extension)
}

/// Validate one output basename against its registry extension.
///
/// Rejects path separators, controls/NUL, `.`/`..`, Windows reserved device
/// names, trailing spaces/dots, over-long names and mismatched extensions.
/// The returned error never echoes the rejected value.
pub fn validate_output_name(name: &str, expected_extension: &str) -> EngineResult<()> {
    let valid = !name.is_empty()
        && name.len() <= MAX_OUTPUT_NAME_BYTES
        && name != "."
        && name != ".."
        && !name.ends_with(' ')
        && !name.ends_with('.')
        && !has_forbidden_characters(name)
        && !is_reserved_device_name(name)
        && has_valid_extension(name, expected_extension);

    if valid {
        Ok(())
    } else {
        Err(errors::invalid_request())
    }
}

/// Atomically publish a temporary file to its final destination without overwriting.
///
/// macOS uses exclusive rename, atomically consuming the temporary name. The
/// portability fallback uses a no-replace hard link. Every cleanup error is surfaced.
pub fn atomic_publish_no_overwrite(temp_path: &Path, final_path: &Path) -> EngineResult<()> {
    #[cfg(target_os = "macos")]
    {
        use std::{ffi::CString, os::unix::ffi::OsStrExt};
        let from =
            CString::new(temp_path.as_os_str().as_bytes()).map_err(|_| errors::internal())?;
        let to = CString::new(final_path.as_os_str().as_bytes()).map_err(|_| errors::internal())?;
        // SAFETY: valid path strings; RENAME_EXCL atomically refuses every existing target.
        if unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) } == 0 {
            return Ok(());
        }
        let err = std::io::Error::last_os_error();
        cleanup_temp(temp_path)?;
        if err.kind() == std::io::ErrorKind::AlreadyExists {
            Err(errors::output_conflict())
        } else {
            Err(errors::internal())
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let result = std::fs::hard_link(temp_path, final_path);
        cleanup_temp(temp_path)?;
        match result {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                Err(errors::output_conflict())
            }
            Err(_) => Err(errors::internal()),
        }
    }
}

/// Remove only the current job's engine-owned temporary output.
pub fn cleanup_temp(path: &Path) -> EngineResult<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(errors::cleanup_failed()),
    }
}

/// Captured at import and compared against the opened source before work starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceIdentity {
    size: u64,
    modified: std::time::SystemTime,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}
impl SourceIdentity {
    pub fn capture(meta: &std::fs::Metadata) -> EngineResult<Self> {
        if !meta.is_file() {
            return Err(errors::input_unavailable());
        }
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Ok(Self {
            size: meta.len(),
            modified: meta.modified().map_err(|_| errors::input_unavailable())?,
            #[cfg(unix)]
            device: meta.dev(),
            #[cfg(unix)]
            inode: meta.ino(),
        })
    }
    pub fn open(&self, path: &Path) -> EngineResult<std::fs::File> {
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let file = options
            .open(path)
            .map_err(|_| errors::input_unavailable())?;
        if Self::capture(&file.metadata().map_err(|_| errors::input_unavailable())?)? != *self {
            return Err(errors::input_unavailable());
        }
        Ok(file)
    }
}

/// Check if a path is on a local mount rather than a network/remote mount.
#[cfg(target_os = "macos")]
pub fn is_local_path(path: &Path) -> bool {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let Ok(canonical) = path.canonicalize() else {
        return false;
    };
    let Ok(cpath) = CString::new(canonical.as_os_str().as_bytes()) else {
        return false;
    };
    let mut stat = std::mem::MaybeUninit::<libc::statfs>::uninit();
    unsafe {
        libc::statfs(cpath.as_ptr(), stat.as_mut_ptr()) == 0
            && stat.assume_init().f_flags & libc::MNT_LOCAL as u32 != 0
    }
}

/// Check if a path is on a local mount rather than a network/remote mount.
#[cfg(not(target_os = "macos"))]
pub fn is_local_path(_path: &Path) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use loconv_contracts::ErrorCode;

    fn assert_rejected(name: &str, extension: &str) {
        let error = validate_output_name(name, extension).expect_err("name must be rejected");
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert_eq!(error.message, errors::INVALID_REQUEST_MESSAGE);
    }

    #[test]
    fn accepts_safe_basenames_with_matching_extension() {
        assert!(validate_output_name("holiday-clip.mp4", "mp4").is_ok());
        assert!(validate_output_name("synthetic image.PNG", "png").is_ok());
        assert!(validate_output_name("track.final.mp3", "mp3").is_ok());
        assert!(validate_output_name("document name.JPG", "jpg").is_ok());
    }

    #[test]
    fn rejects_separators_traversal_and_control_characters() {
        for name in [
            "../escape.mp4",
            "nested/clip.mp4",
            "nested\\clip.mp4",
            ".",
            "..",
            "bad\0name.mp4",
            "line\nbreak.mp4",
            "tab\tname.mp4",
            "drive:name.mp4",
        ] {
            assert_rejected(name, "mp4");
        }
    }

    #[test]
    fn rejects_windows_device_names_and_trailing_whitespace() {
        for name in [
            "CON.mp4",
            "con.mp4",
            "NUL.txt.mp4",
            "COM1.mp4",
            "LPT9.mp4",
            "clip.mp4 ",
            "clip.mp4.",
        ] {
            assert_rejected(name, "mp4");
        }
    }

    #[test]
    fn rejects_length_extension_and_empty_stem_problems() {
        assert_rejected("", "mp4");
        assert_rejected(".mp4", "mp4");
        assert_rejected("clip", "mp4");
        assert_rejected("clip.mp4", "webm");
        assert_rejected("clip.", "mp4");

        let too_long = format!("{}.mp4", "a".repeat(MAX_OUTPUT_NAME_BYTES));
        assert_rejected(&too_long, "mp4");
    }

    #[test]
    fn atomic_publish_no_overwrite_publishes_and_prevents_overwrite() {
        let temp_dir = std::env::temp_dir().join(format!("loconv-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        let temp_file = temp_dir.join("tmp-1.tmp");
        let target_file = temp_dir.join("output.jpg");

        std::fs::write(&temp_file, b"content-1").unwrap();

        // 1. First publish succeeds
        assert!(atomic_publish_no_overwrite(&temp_file, &target_file).is_ok());
        assert!(!temp_file.exists());
        assert_eq!(std::fs::read(&target_file).unwrap(), b"content-1");

        // 2. Second publish to existing target fails with OutputConflict
        let temp_file_2 = temp_dir.join("tmp-2.tmp");
        std::fs::write(&temp_file_2, b"content-2").unwrap();

        let err = atomic_publish_no_overwrite(&temp_file_2, &target_file).unwrap_err();
        assert_eq!(err.code, ErrorCode::OutputConflict);
        // temp file cleaned up
        assert!(!temp_file_2.exists());
        // original target is preserved byte-for-byte!
        assert_eq!(std::fs::read(&target_file).unwrap(), b"content-1");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}

#[cfg(test)]
mod identity_tests {
    use super::*;
    #[test]
    fn source_replacement_and_symlink_changes_are_rejected() {
        let dir = std::env::temp_dir().join(format!("loconv-identity-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let source = dir.join("synthetic");
        std::fs::write(&source, b"synthetic").unwrap();
        let identity = SourceIdentity::capture(&std::fs::metadata(&source).unwrap()).unwrap();
        assert!(identity.open(&source).is_ok());
        std::fs::rename(&source, dir.join("preserved")).unwrap();
        std::fs::write(&source, b"synthetic").unwrap();
        assert!(identity.open(&source).is_err());
        std::fs::remove_file(&source).unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.join("preserved"), &source).unwrap();
            assert!(identity.open(&source).is_err());
        }
        assert_eq!(std::fs::read(dir.join("preserved")).unwrap(), b"synthetic");
        assert!(cleanup_temp(&dir).is_err()); // Cannot silently claim cleanup of a directory.
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn publication_rejects_dangling_symlinks_and_hardlink_conflicts() {
        let dir = std::env::temp_dir().join(format!("loconv-publish-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let target = dir.join("target");
        let source = dir.join("original");
        let temp = dir.join("owned");
        std::fs::write(&source, b"original").unwrap();
        std::fs::hard_link(&source, &target).unwrap();
        std::fs::write(&temp, b"converted").unwrap();
        assert!(atomic_publish_no_overwrite(&temp, &target).is_err());
        assert_eq!(std::fs::read(&source).unwrap(), b"original");
        std::fs::remove_file(&target).unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.join("missing"), &target).unwrap();
            std::fs::write(&temp, b"converted").unwrap();
            assert!(atomic_publish_no_overwrite(&temp, &target).is_err());
            assert!(std::fs::symlink_metadata(&target)
                .unwrap()
                .file_type()
                .is_symlink());
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
