//! Pure output-name and path-string validation.
//!
//! No filesystem access occurs here. Destinations, file identity, symlinks and
//! mount locality are validated by later host increments. These rules reject
//! basename injection before any path is joined or opened.

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
}
