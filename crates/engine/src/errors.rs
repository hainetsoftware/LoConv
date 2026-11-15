//! Fixed, safe errors for the engine and native host.
//!
//! Messages are constants. Do not append paths, filenames, metadata, tool
//! arguments, or raw subprocess output. Frontends render these as text only.

use loconv_contracts::{AppError, ErrorCode};

/// Result type used by engine operations that cross the IPC boundary.
pub type EngineResult<T> = Result<T, AppError>;

pub const INVALID_REQUEST_MESSAGE: &str = "The request is not valid.";
pub const UNSUPPORTED_INPUT_MESSAGE: &str = "The selected file format is not supported.";
pub const UNSUPPORTED_FORMAT_MESSAGE: &str = "This output format is not available.";
pub const INPUT_UNAVAILABLE_MESSAGE: &str = "The input file is no longer available.";
pub const OUTPUT_CONFLICT_MESSAGE: &str = "An output file with this name already exists.";
pub const TOOL_UNAVAILABLE_MESSAGE: &str = "A required local tool is unavailable.";
pub const INSPECTION_FAILED_MESSAGE: &str = "The selected file could not be inspected.";
pub const CONVERSION_FAILED_MESSAGE: &str = "The conversion operation failed.";
pub const RESOURCE_LIMIT_MESSAGE: &str = "The requested resource limit was exceeded.";
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

pub fn unsupported_input() -> AppError {
    app_error(
        ErrorCode::UnsupportedInput,
        UNSUPPORTED_INPUT_MESSAGE,
        false,
    )
}

pub fn unsupported_format() -> AppError {
    app_error(
        ErrorCode::UnsupportedFormat,
        UNSUPPORTED_FORMAT_MESSAGE,
        false,
    )
}

pub fn input_unavailable() -> AppError {
    app_error(
        ErrorCode::InputUnavailable,
        INPUT_UNAVAILABLE_MESSAGE,
        false,
    )
}

pub fn output_conflict() -> AppError {
    app_error(ErrorCode::OutputConflict, OUTPUT_CONFLICT_MESSAGE, false)
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

pub fn conversion_failed() -> AppError {
    app_error(
        ErrorCode::ConversionFailed,
        CONVERSION_FAILED_MESSAGE,
        false,
    )
}

pub fn resource_limit() -> AppError {
    app_error(ErrorCode::ResourceLimit, RESOURCE_LIMIT_MESSAGE, false)
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
            unsupported_input(),
            ErrorCode::UnsupportedInput,
            UNSUPPORTED_INPUT_MESSAGE,
        );
        assert_safe(
            unsupported_format(),
            ErrorCode::UnsupportedFormat,
            UNSUPPORTED_FORMAT_MESSAGE,
        );
        assert_safe(
            input_unavailable(),
            ErrorCode::InputUnavailable,
            INPUT_UNAVAILABLE_MESSAGE,
        );
        assert_safe(
            output_conflict(),
            ErrorCode::OutputConflict,
            OUTPUT_CONFLICT_MESSAGE,
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
        assert_safe(
            conversion_failed(),
            ErrorCode::ConversionFailed,
            CONVERSION_FAILED_MESSAGE,
        );
        assert_safe(
            resource_limit(),
            ErrorCode::ResourceLimit,
            RESOURCE_LIMIT_MESSAGE,
        );
        assert_safe(internal(), ErrorCode::Internal, INTERNAL_MESSAGE);
    }
}
