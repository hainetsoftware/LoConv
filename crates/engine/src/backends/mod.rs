//! Local media backends.
//!
//! Backends are placeholders in this increment: neither FFmpeg execution nor
//! image decoding/encoding is implemented. They deliberately report
//! unavailable so no caller can advertise a missing capability.

pub mod ffmpeg;
pub mod image;
