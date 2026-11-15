//! Offline media engine: image conversion and bounded trusted audio inspection.
//! Audio/video encoding remains unavailable. See ARCHITECTURE.md and PRIVACY.md.

pub mod backends;
pub mod errors;
pub mod formats;
pub mod inspection;
pub mod jobs;
pub mod paths;

pub use image;
pub use loconv_contracts as contracts;

/// IPC version implemented by this engine.
pub const API_VERSION: u32 = loconv_contracts::API_VERSION;

/// Return the IPC version understood by the native host.
///
/// Kept as a function so the Tauri command and tests do not duplicate the
/// constant owned by `loconv-contracts`.
pub fn api_version() -> u32 {
    API_VERSION
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_version_is_ipc_version_one() {
        assert_eq!(api_version(), 1);
        assert_eq!(api_version(), loconv_contracts::API_VERSION);
    }
}
