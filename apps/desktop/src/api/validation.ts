import {
  API_VERSION,
  AppError,
  Backend,
  ConversionRequest,
  DestinationSelection,
  ErrorCode,
  FormatDescriptor,
  FormatId,
  InputInspection,
  JobEvent,
  JobSnapshot,
  JobState,
  MediaKind,
  StreamInfo,
} from './contracts';

export const KNOWN_ERROR_CODES: readonly ErrorCode[] = [
  'invalid_request',
  'unsupported_input',
  'unsupported_format',
  'input_unavailable',
  'output_conflict',
  'permission_denied',
  'tool_unavailable',
  'inspection_failed',
  'conversion_failed',
  'resource_limit',
  'job_not_found',
  'invalid_job_state',
  'internal',
] as const;

export const KNOWN_MEDIA_KINDS: readonly MediaKind[] = ['video', 'audio', 'image'] as const;

export const KNOWN_FORMAT_IDS: readonly FormatId[] = [
  'mp4_h264_aac',
  'webm_vp9_opus',
  'mp3',
  'wav_pcm16',
  'png',
  'jpeg',
  'webp',
] as const;

export const KNOWN_BACKENDS: readonly Backend[] = ['ffmpeg', 'image'] as const;

export const KNOWN_JOB_STATES: readonly JobState[] = [
  'queued',
  'running',
  'cancelling',
  'succeeded',
  'failed',
  'cancelled',
] as const;

const SAFE_INTERNAL_ERROR: AppError = {
  code: 'internal',
  message: 'An unexpected internal error occurred.',
  retryable: false,
};

export function isRecord(val: unknown): val is Record<string, unknown> {
  return typeof val === 'object' && val !== null && !Array.isArray(val);
}

export function isErrorCode(val: unknown): val is ErrorCode {
  return typeof val === 'string' && (KNOWN_ERROR_CODES as readonly string[]).includes(val);
}

export function isMediaKind(val: unknown): val is MediaKind {
  return typeof val === 'string' && (KNOWN_MEDIA_KINDS as readonly string[]).includes(val);
}

export function isFormatId(val: unknown): val is FormatId {
  return typeof val === 'string' && (KNOWN_FORMAT_IDS as readonly string[]).includes(val);
}

export function isBackend(val: unknown): val is Backend {
  return typeof val === 'string' && (KNOWN_BACKENDS as readonly string[]).includes(val);
}

export function isJobState(val: unknown): val is JobState {
  return typeof val === 'string' && (KNOWN_JOB_STATES as readonly string[]).includes(val);
}

export function normalizeError(err: unknown): AppError {
  if (isRecord(err) && isErrorCode(err.code) && typeof err.message === 'string' && typeof err.retryable === 'boolean') {
    return {
      code: err.code,
      message: err.message,
      retryable: err.retryable,
    };
  }
  return SAFE_INTERNAL_ERROR;
}

export function validateStreamInfo(val: unknown): StreamInfo {
  if (!isRecord(val)) throw new Error('Invalid StreamInfo: not an object');
  const { index, kind, codec, width, height, sampleRateHz, channels } = val;

  if (typeof index !== 'number' || !Number.isInteger(index) || index < 0) {
    throw new Error('Invalid StreamInfo: index must be a non-negative integer');
  }
  if (!isMediaKind(kind)) {
    throw new Error(`Invalid StreamInfo: unknown kind ${String(kind)}`);
  }
  if (typeof codec !== 'string') {
    throw new Error('Invalid StreamInfo: codec must be a string');
  }
  if (width !== null && (typeof width !== 'number' || !Number.isInteger(width) || width <= 0)) {
    throw new Error('Invalid StreamInfo: width must be null or positive integer');
  }
  if (height !== null && (typeof height !== 'number' || !Number.isInteger(height) || height <= 0)) {
    throw new Error('Invalid StreamInfo: height must be null or positive integer');
  }
  if (sampleRateHz !== null && (typeof sampleRateHz !== 'number' || !Number.isInteger(sampleRateHz) || sampleRateHz <= 0)) {
    throw new Error('Invalid StreamInfo: sampleRateHz must be null or positive integer');
  }
  if (channels !== null && (typeof channels !== 'number' || !Number.isInteger(channels) || channels <= 0)) {
    throw new Error('Invalid StreamInfo: channels must be null or positive integer');
  }

  return {
    index,
    kind,
    codec,
    width: width as number | null,
    height: height as number | null,
    sampleRateHz: sampleRateHz as number | null,
    channels: channels as number | null,
  };
}

export function validateInputInspection(val: unknown): InputInspection {
  if (!isRecord(val)) throw new Error('Invalid InputInspection: not an object');
  const { mediaId, displayName, kind, sizeBytes, durationMs, container, streams } = val;

  if (typeof mediaId !== 'string' || mediaId.length === 0) {
    throw new Error('Invalid InputInspection: mediaId must be non-empty string');
  }
  if (typeof displayName !== 'string') {
    throw new Error('Invalid InputInspection: displayName must be string');
  }
  if (!isMediaKind(kind)) {
    throw new Error(`Invalid InputInspection: unknown kind ${String(kind)}`);
  }
  if (typeof sizeBytes !== 'string' || !/^\d+$/.test(sizeBytes)) {
    throw new Error('Invalid InputInspection: sizeBytes must be decimal u64 string');
  }
  if (durationMs !== null && (typeof durationMs !== 'number' || !Number.isFinite(durationMs) || durationMs < 0)) {
    throw new Error('Invalid InputInspection: durationMs must be null or non-negative finite number');
  }
  if (typeof container !== 'string') {
    throw new Error('Invalid InputInspection: container must be string');
  }
  if (!Array.isArray(streams)) {
    throw new Error('Invalid InputInspection: streams must be array');
  }

  return {
    mediaId,
    displayName,
    kind,
    sizeBytes,
    durationMs: durationMs as number | null,
    container,
    streams: streams.map(validateStreamInfo),
  };
}

export function validateFormatDescriptor(val: unknown): FormatDescriptor {
  if (!isRecord(val)) throw new Error('Invalid FormatDescriptor: not an object');
  const { id, label, extension, inputKinds, backend, available, unavailableReason } = val;

  if (!isFormatId(id)) throw new Error(`Invalid FormatDescriptor: unknown format id ${String(id)}`);
  if (typeof label !== 'string') throw new Error('Invalid FormatDescriptor: label must be string');
  if (typeof extension !== 'string') throw new Error('Invalid FormatDescriptor: extension must be string');
  if (!Array.isArray(inputKinds) || !inputKinds.every(isMediaKind)) {
    throw new Error('Invalid FormatDescriptor: inputKinds must be array of MediaKind');
  }
  if (!isBackend(backend)) throw new Error(`Invalid FormatDescriptor: unknown backend ${String(backend)}`);
  if (typeof available !== 'boolean') throw new Error('Invalid FormatDescriptor: available must be boolean');
  if (unavailableReason !== null && !isErrorCode(unavailableReason)) {
    throw new Error(`Invalid FormatDescriptor: unavailableReason must be null or ErrorCode`);
  }

  return {
    id,
    label,
    extension,
    inputKinds,
    backend,
    available,
    unavailableReason: unavailableReason as ErrorCode | null,
  };
}

export function validateDestinationSelection(val: unknown): DestinationSelection | null {
  if (val === null) return null;
  if (!isRecord(val)) throw new Error('Invalid DestinationSelection: not an object or null');
  const { destinationId, displayName } = val;
  if (typeof destinationId !== 'string' || destinationId.length === 0) {
    throw new Error('Invalid DestinationSelection: destinationId must be non-empty string');
  }
  if (typeof displayName !== 'string') {
    throw new Error('Invalid DestinationSelection: displayName must be string');
  }
  return { destinationId, displayName };
}

export function validateJobSnapshot(val: unknown): JobSnapshot {
  if (!isRecord(val)) throw new Error('Invalid JobSnapshot: not an object');
  const { jobId, mediaId, formatId, outputName, state, revision, progress, error } = val;

  if (typeof jobId !== 'string' || jobId.length === 0) {
    throw new Error('Invalid JobSnapshot: jobId must be non-empty string');
  }
  if (typeof mediaId !== 'string' || mediaId.length === 0) {
    throw new Error('Invalid JobSnapshot: mediaId must be non-empty string');
  }
  if (!isFormatId(formatId)) {
    throw new Error(`Invalid JobSnapshot: unknown formatId ${String(formatId)}`);
  }
  if (typeof outputName !== 'string' || outputName.length === 0) {
    throw new Error('Invalid JobSnapshot: outputName must be non-empty string');
  }
  if (!isJobState(state)) {
    throw new Error(`Invalid JobSnapshot: unknown state ${String(state)}`);
  }
  if (typeof revision !== 'number' || !Number.isInteger(revision) || revision < 1) {
    throw new Error('Invalid JobSnapshot: revision must be an integer >= 1');
  }
  if (progress !== null && (typeof progress !== 'number' || !Number.isFinite(progress) || progress < 0 || progress > 1)) {
    throw new Error('Invalid JobSnapshot: progress must be null or finite number in [0, 1]');
  }

  let validatedError: AppError | null = null;
  if (error !== null) {
    if (!isRecord(error) || !isErrorCode(error.code) || typeof error.message !== 'string' || typeof error.retryable !== 'boolean') {
      throw new Error('Invalid JobSnapshot: error must be null or valid AppError');
    }
    validatedError = {
      code: error.code,
      message: error.message,
      retryable: error.retryable,
    };
  }

  return {
    jobId,
    mediaId,
    formatId,
    outputName,
    state,
    revision,
    progress: progress as number | null,
    error: validatedError,
  };
}

export function validateJobEvent(val: unknown): JobEvent {
  if (!isRecord(val)) throw new Error('Invalid JobEvent: not an object');
  const { apiVersion, job } = val;

  if (apiVersion !== API_VERSION) {
    throw new Error(`Invalid JobEvent: unsupported apiVersion ${String(apiVersion)}, expected ${API_VERSION}`);
  }

  return {
    apiVersion: API_VERSION,
    job: validateJobSnapshot(job),
  };
}

export function validateConversionRequest(val: unknown): ConversionRequest {
  if (!isRecord(val)) throw new Error('Invalid ConversionRequest: not an object');
  const { mediaId, destinationId, outputName, formatId } = val;

  if (typeof mediaId !== 'string' || mediaId.length === 0) {
    throw new Error('Invalid ConversionRequest: mediaId must be non-empty string');
  }
  if (typeof destinationId !== 'string' || destinationId.length === 0) {
    throw new Error('Invalid ConversionRequest: destinationId must be non-empty string');
  }
  if (typeof outputName !== 'string' || outputName.length === 0) {
    throw new Error('Invalid ConversionRequest: outputName must be non-empty string');
  }
  if (!isFormatId(formatId)) {
    throw new Error(`Invalid ConversionRequest: unknown formatId ${String(formatId)}`);
  }

  return { mediaId, destinationId, outputName, formatId };
}

const WINDOWS_RESERVED_NAMES = new Set([
  'CON', 'PRN', 'AUX', 'NUL',
  'COM1', 'COM2', 'COM3', 'COM4', 'COM5', 'COM6', 'COM7', 'COM8', 'COM9',
  'LPT1', 'LPT2', 'LPT3', 'LPT4', 'LPT5', 'LPT6', 'LPT7', 'LPT8', 'LPT9',
]);

/**
 * Validates output basename according to ARCHITECTURE.md:
 * - Reject separators (/ and \)
 * - Reject . and ..
 * - Reject control characters and NUL
 * - Reject Windows reserved device names
 * - Reject trailing spaces and dots
 * - Length must be between 1 and 255 UTF-8 bytes
 * - Matches expected extension if provided
 */
export function validateOutputBasename(
  name: string,
  expectedExtension?: string,
): { valid: boolean; error?: string } {
  if (!name || name.trim().length === 0) {
    return { valid: false, error: 'Output filename cannot be empty' };
  }

  if (name.includes('/') || name.includes('\\')) {
    return { valid: false, error: 'Output filename cannot contain path separators' };
  }

  if (name === '.' || name === '..') {
    return { valid: false, error: 'Output filename cannot be "." or ".."' };
  }

  // Check control chars and NUL
  // eslint-disable-next-line no-control-regex
  if (/[\x00-\x1F\x7F]/.test(name)) {
    return { valid: false, error: 'Output filename contains invalid control characters' };
  }

  // Trailing space or dot
  if (name.endsWith(' ') || name.endsWith('.')) {
    return { valid: false, error: 'Output filename cannot end with a space or period' };
  }

  // UTF-8 byte length max 255
  const encoder = new TextEncoder();
  const byteLength = encoder.encode(name).length;
  if (byteLength > 255) {
    return { valid: false, error: 'Output filename exceeds 255 UTF-8 bytes limit' };
  }

  // Base stem check for reserved device names
  const stem = name.split('.')[0]?.toUpperCase() ?? '';
  if (WINDOWS_RESERVED_NAMES.has(stem)) {
    return { valid: false, error: `Output filename uses reserved name "${stem}"` };
  }

  // Check expected extension
  if (expectedExtension) {
    const dotExt = `.${expectedExtension.toLowerCase()}`;
    if (!name.toLowerCase().endsWith(dotExt)) {
      return { valid: false, error: `Output filename must end with "${dotExt}"` };
    }
  }

  return { valid: true };
}
