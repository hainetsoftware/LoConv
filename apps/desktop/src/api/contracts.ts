/** IPC v1 mirror of crates/contracts. JSON uses camelCase fields and snake_case enums.
 * Local media data must never enter logs, telemetry, storage, or network calls.
 */
export const API_VERSION = 1 as const;
export type MediaKind = 'video' | 'audio' | 'image';
export type FormatId = 'mp4_h264_aac' | 'mkv_h264_aac' | 'avi_mpeg4_mp3' | 'webm_vp9_opus' | 'mp3' | 'wav_pcm16' | 'png' | 'jpeg' | 'webp';
export type Backend = 'ffmpeg' | 'image';
export type JobState = 'queued' | 'running' | 'cancelling' | 'succeeded' | 'failed' | 'cancelled';
export type ErrorCode = 'invalid_request' | 'unsupported_input' | 'unsupported_format'
  | 'input_unavailable' | 'output_conflict' | 'permission_denied' | 'tool_unavailable'
  | 'inspection_failed' | 'conversion_failed' | 'resource_limit' | 'job_not_found'
  | 'invalid_job_state' | 'internal';
export interface AppError { code: ErrorCode; message: string; retryable: boolean }
export interface StreamInfo {
  index: number; kind: MediaKind; codec: string; width: number | null;
  height: number | null; sampleRateHz: number | null; channels: number | null;
}
export interface InputInspection {
  mediaId: string; displayName: string; kind: MediaKind; sizeBytes: string;
  durationMs: number | null; container: string; streams: StreamInfo[];
}
export interface FormatDescriptor {
  id: FormatId; label: string; extension: string; inputKinds: MediaKind[];
  backend: Backend; available: boolean; unavailableReason: ErrorCode | null;
}
export interface ConversionRequest {
  mediaId: string; destinationId: string; outputName: string; formatId: FormatId;
}
export interface DestinationSelection { destinationId: string; displayName: string }
export interface JobSnapshot {
  jobId: string; mediaId: string; formatId: FormatId; outputName: string;
  state: JobState; revision: number; progress: number | null; error: AppError | null;
}
export interface JobEvent { apiVersion: typeof API_VERSION; job: JobSnapshot }

/** Tauri adapter and in-memory UI mock must implement the same port.
 * Failures reject with AppError; dialog cancellation resolves to [] / null.
 */
export interface DesktopApi {
  getApiVersion(): Promise<number>;
  selectInputs(): Promise<InputInspection[]>;
  importDroppedInputs(paths: string[]): Promise<InputInspection[]>;
  selectDestination(): Promise<DestinationSelection | null>;
  listFormats(): Promise<FormatDescriptor[]>;
  enqueueJob(request: ConversionRequest): Promise<JobSnapshot>;
  listJobs(): Promise<JobSnapshot[]>;
  cancelJob(jobId: string): Promise<JobSnapshot>;
  onJobChanged(listener: (event: JobEvent) => void): Promise<() => void>;
}
