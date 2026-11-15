//! FFmpeg/FFprobe backend placeholder.
//!
//! Future increments resolve bundled, network-disabled binaries from trusted
//! resource paths and execute them with argument arrays only. This module must
//! never expose a generic process runner to the frontend.

/// Whether a trusted FFmpeg/FFprobe pair is configured.
///
/// Always `false` until the tool resolver and capability discovery land.
pub const fn available() -> bool {
    false
}
