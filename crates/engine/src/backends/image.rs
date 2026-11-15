//! Still-image backend placeholder.
//!
//! Future increments add bounded PNG/JPEG/WebP decode and encode with explicit
//! decoder limits. No image bytes are read or written in this increment.

/// Whether a bounded image codec backend is configured.
///
/// Always `false` until explicit codec features and limits land.
pub const fn available() -> bool {
    false
}
