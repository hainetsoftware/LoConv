//! IPC v1 data only. No filesystem, process, Tauri, or network dependencies.
//! Semantic validation belongs to the engine; see ARCHITECTURE.md.
use serde::{Deserialize, Serialize};

pub const API_VERSION: u32 = 1;

macro_rules! wire_enum {
    ($name:ident { $($variant:ident),+ $(,)? }) => {
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($variant),+ }
    };
}

wire_enum!(MediaKind {
    Video,
    Audio,
    Image
});
wire_enum!(FormatId {
    Mp4H264Aac,
    MkvH264Aac,
    AviMpeg4Mp3,
    WebmVp9Opus,
    Mp3,
    WavPcm16,
    Png,
    Jpeg,
    Webp
});
wire_enum!(Backend { Ffmpeg, Image });
wire_enum!(JobState {
    Queued,
    Running,
    Cancelling,
    Succeeded,
    Failed,
    Cancelled
});
wire_enum!(ErrorCode {
    InvalidRequest,
    UnsupportedInput,
    UnsupportedFormat,
    InputUnavailable,
    OutputConflict,
    PermissionDenied,
    ToolUnavailable,
    InspectionFailed,
    ConversionFailed,
    ResourceLimit,
    JobNotFound,
    InvalidJobState,
    Internal
});

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppError {
    pub code: ErrorCode,
    /// Fixed, safe UI message. Never include paths, metadata, or stderr.
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StreamInfo {
    pub index: u32,
    pub kind: MediaKind,
    pub codec: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub sample_rate_hz: Option<u32>,
    pub channels: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InputInspection {
    pub media_id: String,
    /// Local UI display only, never log this value.
    pub display_name: String,
    pub kind: MediaKind,
    /// Decimal u64 string to preserve values above JS MAX_SAFE_INTEGER.
    pub size_bytes: String,
    pub duration_ms: Option<f64>,
    pub container: String,
    pub streams: Vec<StreamInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FormatDescriptor {
    pub id: FormatId,
    pub label: String,
    pub extension: String,
    pub input_kinds: Vec<MediaKind>,
    pub backend: Backend,
    pub available: bool,
    pub unavailable_reason: Option<ErrorCode>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConversionRequest {
    pub media_id: String,
    pub destination_id: String,
    /// Single basename, including the registry's output extension.
    pub output_name: String,
    pub format_id: FormatId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DestinationSelection {
    pub destination_id: String,
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobSnapshot {
    pub job_id: String,
    pub media_id: String,
    pub format_id: FormatId,
    pub output_name: String,
    pub state: JobState,
    pub revision: u32,
    pub progress: Option<f64>,
    pub error: Option<AppError>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobEvent {
    pub api_version: u32,
    pub job: JobSnapshot,
}
