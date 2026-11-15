//! Fixed, safe errors for the engine and native host.
//!
//! Messages are constants. Do not append paths, filenames, metadata, tool
//! arguments, or raw subprocess output. Frontends render these as text only.

use loconv_contracts::{AppError, ErrorCode};

/// Result type used by engine operations that cross the IPC boundary.
pub type EngineResult<T> = Result<T, AppError>;

pub const INVALID_REQUEST_MESSAGE: &str = "The request is not valid.";
pub const UNSUPPORTED_FORMAT_MESSAGE: &str = "This output format is not available.";
pub const TOOL_UNAVAILABLE_MESSAGE: &str = "A required local tool is unavailable.";
pub const INSPECTION_FAILED_MESSAGE: &str = "The selected file could not be inspected.";
pub const INTERNAL_MESSAGE: &str = "An internal error occurred.";

fn app_error(code: ErrorCode, message: &'static str, retryable: bool) -> AppError {
    AppError {
        code,
        message: message.to_owned(),
        retryable,
    }
}

pub fn invalid_request() -> AppError {
    app_error(ErrorCode::InvalidRequest, INVALID_REQUEST_MESSAGE, false)
}

pub fn unsupported_format() -> AppError {
    app_error(
        ErrorCode::UnsupportedFormat,
        UNSUPPORTED_FORMAT_MESSAGE,
        false,
    )
}

pub fn tool_unavailable() -> AppError {
    app_error(ErrorCode::ToolUnavailable, TOOL_UNAVAILABLE_MESSAGE, false)
}

pub fn inspection_failed() -> AppError {
    app_error(
        ErrorCode::InspectionFailed,
        INSPECTION_FAILED_MESSAGE,
        false,
    )
}

pub fn internal() -> AppError {
    app_error(ErrorCode::Internal, INTERNAL_MESSAGE, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_safe(error: AppError, code: ErrorCode, message: &str) {
        assert_eq!(error.code, code);
        assert_eq!(error.message, message);
        assert!(!error.message.contains('/'));
        assert!(!error.message.contains('\\'));
        assert!(!error.retryable);
    }

    #[test]
    fn errors_use_fixed_safe_messages() {
        assert_safe(
            invalid_request(),
            ErrorCode::InvalidRequest,
            INVALID_REQUEST_MESSAGE,
        );
        assert_safe(
            unsupported_format(),
            ErrorCode::UnsupportedFormat,
            UNSUPPORTED_FORMAT_MESSAGE,
        );
        assert_safe(
            tool_unavailable(),
            ErrorCode::ToolUnavailable,
            TOOL_UNAVAILABLE_MESSAGE,
        );
        assert_safe(
            inspection_failed(),
            ErrorCode::InspectionFailed,
            INSPECTION_FAILED_MESSAGE,
        );
        assert_safe(internal(), ErrorCode::Internal, INTERNAL_MESSAGE);
    }
}
