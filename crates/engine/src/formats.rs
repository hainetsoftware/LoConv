//! Static format registry foundation.
//!
//! Descriptors describe real input/output combinations. Availability is derived
//! from backend capability, never from file extensions or optimistic defaults.
//! Images are available; audio needs an explicitly configured trusted encoder.

use loconv_contracts::{Backend, ErrorCode, FormatDescriptor, FormatId, MediaKind};

use crate::backends;

/// An immutable registry entry. The wire `FormatDescriptor` is produced from it
/// so availability cannot drift between the registry and advertised presets.
#[derive(Debug, Clone)]
pub struct FormatSpec {
    pub id: FormatId,
    pub label: &'static str,
    pub extension: &'static str,
    pub input_kinds: &'static [MediaKind],
    pub backend: Backend,
}

const VIDEO_ONLY: &[MediaKind] = &[MediaKind::Video];
const AUDIO_OR_VIDEO: &[MediaKind] = &[MediaKind::Audio];
const IMAGE_ONLY: &[MediaKind] = &[MediaKind::Image];

pub const REGISTRY: &[FormatSpec] = &[
    FormatSpec {
        id: FormatId::Mp4H264Aac,
        label: "MP4 (H.264/AAC)",
        extension: "mp4",
        input_kinds: VIDEO_ONLY,
        backend: Backend::Ffmpeg,
    },
    FormatSpec {
        id: FormatId::WebmVp9Opus,
        label: "WebM (VP9/Opus)",
        extension: "webm",
        input_kinds: VIDEO_ONLY,
        backend: Backend::Ffmpeg,
    },
    FormatSpec {
        id: FormatId::Mp3,
        label: "MP3",
        extension: "mp3",
        input_kinds: AUDIO_OR_VIDEO,
        backend: Backend::Ffmpeg,
    },
    FormatSpec {
        id: FormatId::WavPcm16,
        label: "WAV (PCM 16-bit)",
        extension: "wav",
        input_kinds: AUDIO_OR_VIDEO,
        backend: Backend::Ffmpeg,
    },
    FormatSpec {
        id: FormatId::Png,
        label: "PNG",
        extension: "png",
        input_kinds: IMAGE_ONLY,
        backend: Backend::Image,
    },
    FormatSpec {
        id: FormatId::Jpeg,
        label: "JPEG",
        extension: "jpg",
        input_kinds: IMAGE_ONLY,
        backend: Backend::Image,
    },
    FormatSpec {
        id: FormatId::Webp,
        label: "WebP (lossless)",
        extension: "webp",
        input_kinds: IMAGE_ONLY,
        backend: Backend::Image,
    },
];

fn backend_state(backend: Backend) -> (bool, Option<ErrorCode>) {
    match backend {
        Backend::Ffmpeg => {
            let available = backends::ffmpeg::available();
            (
                available,
                (!available).then_some(ErrorCode::ToolUnavailable),
            )
        }
        Backend::Image => {
            let available = backends::image::available();
            (
                available,
                (!available).then_some(ErrorCode::UnsupportedFormat),
            )
        }
    }
}

impl FormatSpec {
    /// Build the wire descriptor with current backend availability.
    pub fn descriptor(&self) -> FormatDescriptor {
        let (available, unavailable_reason) = backend_state(self.backend.clone());
        FormatDescriptor {
            id: self.id.clone(),
            label: self.label.to_owned(),
            extension: self.extension.to_owned(),
            input_kinds: self.input_kinds.to_vec(),
            backend: self.backend.clone(),
            available,
            unavailable_reason,
        }
    }
}

/// All registered formats with current availability.
pub fn descriptors() -> Vec<FormatDescriptor> {
    REGISTRY.iter().map(FormatSpec::descriptor).collect()
}

/// Find a registry entry without consulting the filesystem.
pub fn find(id: &FormatId) -> Option<&'static FormatSpec> {
    REGISTRY.iter().find(|spec| &spec.id == id)
}

/// Session capabilities come exclusively from the configured trusted encoder.
pub fn descriptors_with_audio(
    tool: Option<&backends::ffmpeg::TrustedFfmpeg>,
) -> Vec<FormatDescriptor> {
    descriptors()
        .into_iter()
        .map(|mut d| {
            if tool.is_some_and(|t| t.supports(&d.id)) {
                d.available = true;
                d.unavailable_reason = None;
            }
            d
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_complete_and_derives_availability_from_backends() {
        let descriptors = descriptors();
        assert_eq!(descriptors.len(), REGISTRY.len());
        assert_eq!(descriptors.len(), 7);

        for descriptor in &descriptors {
            assert!(!descriptor.label.is_empty());
            assert!(!descriptor.input_kinds.is_empty());
            assert!(!descriptor.extension.is_empty());
            match descriptor.backend {
                Backend::Ffmpeg => {
                    assert!(!descriptor.available);
                    assert_eq!(
                        descriptor.unavailable_reason,
                        Some(ErrorCode::ToolUnavailable)
                    );
                }
                Backend::Image => {
                    assert!(descriptor.available);
                    assert_eq!(descriptor.unavailable_reason, None);
                }
            }
        }
    }

    #[test]
    fn registry_entries_are_unique_and_well_formed() {
        for (index, spec) in REGISTRY.iter().enumerate() {
            assert!(!spec.label.is_empty());
            assert!(!spec.extension.is_empty());
            assert!(spec
                .extension
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit()));
            assert!(spec.input_kinds.iter().all(|kind| matches!(
                kind,
                MediaKind::Audio | MediaKind::Video | MediaKind::Image
            )));
            assert!(
                REGISTRY
                    .iter()
                    .skip(index + 1)
                    .all(|other| other.id != spec.id),
                "duplicate format id"
            );
        }
    }

    #[test]
    fn find_returns_registered_entry_without_io() {
        assert_eq!(
            find(&FormatId::Mp4H264Aac).map(|spec| spec.extension),
            Some("mp4")
        );
        assert_eq!(
            find(&FormatId::Webp).map(|spec| spec.backend.clone()),
            Some(Backend::Image)
        );
    }
}
