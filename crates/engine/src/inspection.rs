//! Inspection limits and fail-closed placeholder.
//!
//! No file is opened and no subprocess is spawned in this increment. The
//! `Inspector` trait is the seam for a future bounded FFprobe/image probe so
//! the host can be tested without a real media backend.

use std::path::Path;
use std::time::Duration;

use loconv_contracts::InputInspection;

use crate::errors::{self, EngineResult};

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

/// Seam for future bounded media inspection.
pub trait Inspector {
    fn inspect(&self, path: &Path) -> EngineResult<InputInspection>;
}

/// Inspector that refuses every request because no trusted probe exists yet.
#[derive(Debug, Default, Clone, Copy)]
pub struct UnavailableInspector;

impl Inspector for UnavailableInspector {
    fn inspect(&self, _path: &Path) -> EngineResult<InputInspection> {
        Err(errors::tool_unavailable())
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
}
