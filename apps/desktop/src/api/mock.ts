import {
  API_VERSION,
  AppError,
  ConversionRequest,
  DesktopApi,
  DestinationSelection,
  ErrorCode,
  FormatDescriptor,
  FormatId,
  InputInspection,
  JobEvent,
  JobSnapshot,
  MediaKind,
} from './contracts';
import { validateConversionRequest, validateOutputBasename } from './validation';

export const INITIAL_MOCK_FORMATS: FormatDescriptor[] = [
  {
    id: 'mp4_h264_aac',
    label: 'MP4 (H.264 / AAC)',
    extension: 'mp4',
    inputKinds: ['video'],
    backend: 'ffmpeg',
    available: true,
    unavailableReason: null,
  },
  {
    id: 'webm_vp9_opus',
    label: 'WebM (VP9 / Opus)',
    extension: 'webm',
    inputKinds: ['video'],
    backend: 'ffmpeg',
    available: true,
    unavailableReason: null,
  },
  {
    id: 'mp3',
    label: 'MP3 (192 kbps)',
    extension: 'mp3',
    inputKinds: ['audio', 'video'],
    backend: 'ffmpeg',
    available: false,
    unavailableReason: 'tool_unavailable',
  },
  {
    id: 'wav_pcm16',
    label: 'WAV (PCM 16-bit)',
    extension: 'wav',
    inputKinds: ['audio', 'video'],
    backend: 'ffmpeg',
    available: true,
    unavailableReason: null,
  },
  {
    id: 'png',
    label: 'PNG (lossless)',
    extension: 'png',
    inputKinds: ['image'],
    backend: 'image',
    available: true,
    unavailableReason: null,
  },
  {
    id: 'jpeg',
    label: 'JPEG (quality 85)',
    extension: 'jpg',
    inputKinds: ['image'],
    backend: 'image',
    available: true,
    unavailableReason: null,
  },
  {
    id: 'webp',
    label: 'WebP (lossless)',
    extension: 'webp',
    inputKinds: ['image'],
    backend: 'image',
    available: true,
    unavailableReason: null,
  },
];

export const INITIAL_MOCK_INPUTS: InputInspection[] = [
  {
    mediaId: 'mock-media-video-1',
    displayName: 'synthetic-landscape.mp4',
    kind: 'video',
    sizeBytes: '52428800',
    durationMs: 45200,
    container: 'mov,mp4,m4a,3gp,3g2,mj2',
    streams: [
      {
        index: 0,
        kind: 'video',
        codec: 'h264',
        width: 1920,
        height: 1080,
        sampleRateHz: null,
        channels: null,
      },
      {
        index: 1,
        kind: 'audio',
        codec: 'aac',
        width: null,
        height: null,
        sampleRateHz: 48000,
        channels: 2,
      },
    ],
  },
  {
    mediaId: 'mock-media-audio-1',
    displayName: 'synthetic-tone-stereo.wav',
    kind: 'audio',
    sizeBytes: '23040000',
    durationMs: 120000,
    container: 'wav',
    streams: [
      {
        index: 0,
        kind: 'audio',
        codec: 'pcm_s16le',
        width: null,
        height: null,
        sampleRateHz: 48000,
        channels: 2,
      },
    ],
  },
  {
    mediaId: 'mock-media-image-1',
    displayName: 'synthetic-diagram.png',
    kind: 'image',
    sizeBytes: '8388608',
    durationMs: null,
    container: 'png_pipe',
    streams: [
      {
        index: 0,
        kind: 'image',
        codec: 'png',
        width: 3840,
        height: 2160,
        sampleRateHz: null,
        channels: null,
      },
    ],
  },
];

export interface MockAdapterOptions {
  autoProgress?: boolean;
  stepDelayMs?: number;
  initialFormats?: FormatDescriptor[];
  initialInputs?: InputInspection[];
}

export class MockDesktopApi implements DesktopApi {
  private formats: FormatDescriptor[];
  private inputs: InputInspection[];
  private destination: DestinationSelection | null = {
    destinationId: 'mock-destination-default',
    displayName: 'Synthetic Output Directory',
  };
  private jobs: Map<string, JobSnapshot> = new Map();
  private listeners: Set<(event: JobEvent) => void> = new Set();
  private jobTimeouts: Map<string, NodeJS.Timeout[]> = new Map();
  private idCounter = 1;

  public autoProgress: boolean;
  public stepDelayMs: number;
  public failNextEnqueue: AppError | null = null;

  constructor(options: MockAdapterOptions = {}) {
    this.autoProgress = options.autoProgress ?? true;
    this.stepDelayMs = options.stepDelayMs ?? 40;
    this.formats = options.initialFormats
      ? options.initialFormats.map((f) => ({ ...f }))
      : INITIAL_MOCK_FORMATS.map((f) => ({ ...f }));
    this.inputs = options.initialInputs
      ? options.initialInputs.map((i) => ({ ...i }))
      : INITIAL_MOCK_INPUTS.map((i) => ({ ...i }));
  }

  async getApiVersion(): Promise<number> {
    return API_VERSION;
  }

  async selectInputs(): Promise<InputInspection[]> {
    return [...this.inputs];
  }

  async importDroppedInputs(paths: string[]): Promise<InputInspection[]> {
    if (!Array.isArray(paths) || paths.length === 0) {
      throw {
        code: 'invalid_request',
        message: 'No paths provided for drop import.',
        retryable: false,
      } as AppError;
    }

    const imported: InputInspection[] = [];

    for (const rawPath of paths) {
      if (typeof rawPath !== 'string' || rawPath.trim().length === 0) {
        throw {
          code: 'invalid_request',
          message: 'Invalid empty path provided.',
          retryable: false,
        } as AppError;
      }

      if (rawPath.startsWith('http://') || rawPath.startsWith('https://') || rawPath.startsWith('file://')) {
        throw {
          code: 'unsupported_input',
          message: 'URL schemes are not allowed. Only local files are accepted.',
          retryable: false,
        } as AppError;
      }

      const basename = rawPath.split('/').pop()?.split('\\').pop() ?? 'file';
      const ext = basename.split('.').pop()?.toLowerCase() ?? '';

      let kind: MediaKind = 'image';
      let codec = 'png';
      let width: number | null = 1920;
      let height: number | null = 1080;
      let sampleRateHz: number | null = null;
      let channels: number | null = null;
      let durationMs: number | null = null;

      if (['mp4', 'mov', 'mkv', 'webm', 'avi'].includes(ext)) {
        kind = 'video';
        codec = ext === 'webm' ? 'vp9' : 'h264';
        durationMs = 30000;
        sampleRateHz = 48000;
        channels = 2;
      } else if (['mp3', 'wav', 'flac', 'aac', 'ogg', 'm4a'].includes(ext)) {
        kind = 'audio';
        codec = ext === 'wav' ? 'pcm_s16le' : ext;
        width = null;
        height = null;
        sampleRateHz = 44100;
        channels = 2;
        durationMs = 60000;
      } else if (['png', 'jpg', 'jpeg', 'webp', 'bmp'].includes(ext)) {
        kind = 'image';
        codec = ext === 'jpg' ? 'jpeg' : ext;
        durationMs = null;
      } else {
        throw {
          code: 'unsupported_input',
          message: `Unsupported file format: .${ext}`,
          retryable: false,
        } as AppError;
      }

      const streams = [
        {
          index: 0,
          kind,
          codec,
          width,
          height,
          sampleRateHz: kind === 'video' ? null : sampleRateHz,
          channels: kind === 'video' ? null : channels,
        },
      ];

      if (kind === 'video') {
        streams.push({
          index: 1,
          kind: 'audio',
          codec: 'aac',
          width: null,
          height: null,
          sampleRateHz: 48000,
          channels: 2,
        });
      }

      const inspection: InputInspection = {
        mediaId: `mock-media-${Date.now()}-${this.idCounter++}`,
        displayName: basename,
        kind,
        sizeBytes: '10485760',
        durationMs,
        container: ext,
        streams,
      };

      imported.push(inspection);
    }

    // Add to known inputs
    this.inputs.push(...imported);
    return imported;
  }

  async selectDestination(): Promise<DestinationSelection | null> {
    if (!this.destination) {
      this.destination = {
        destinationId: `mock-dest-${this.idCounter++}`,
        displayName: 'Selected Output Folder (synthetic)',
      };
    }
    return this.destination;
  }

  setDestination(dest: DestinationSelection | null): void {
    this.destination = dest;
  }

  async listFormats(): Promise<FormatDescriptor[]> {
    return [...this.formats];
  }

  setFormatAvailability(formatId: FormatId, available: boolean, reason: ErrorCode | null = null): void {
    const fmt = this.formats.find((f) => f.id === formatId);
    if (fmt) {
      fmt.available = available;
      fmt.unavailableReason = available ? null : (reason ?? 'tool_unavailable');
    }
  }

  async enqueueJob(request: ConversionRequest): Promise<JobSnapshot> {
    if (this.failNextEnqueue) {
      const err = this.failNextEnqueue;
      this.failNextEnqueue = null;
      throw err;
    }

    const validReq = validateConversionRequest(request);

    // Format check
    const format = this.formats.find((f) => f.id === validReq.formatId);
    if (!format) {
      throw {
        code: 'unsupported_format',
        message: 'Unknown format requested.',
        retryable: false,
      } as AppError;
    }

    if (!format.available) {
      throw {
        code: format.unavailableReason ?? 'tool_unavailable',
        message: 'The requested format is currently unavailable.',
        retryable: false,
      } as AppError;
    }

    // Basename check
    const baseValidation = validateOutputBasename(validReq.outputName, format.extension);
    if (!baseValidation.valid) {
      throw {
        code: 'invalid_request',
        message: baseValidation.error ?? 'Invalid output filename.',
        retryable: false,
      } as AppError;
    }

    const jobId = `mock-job-${this.idCounter++}`;
    const initialSnapshot: JobSnapshot = {
      jobId,
      mediaId: validReq.mediaId,
      formatId: validReq.formatId,
      outputName: validReq.outputName,
      state: 'queued',
      revision: 1,
      progress: null,
      error: null,
    };

    this.jobs.set(jobId, initialSnapshot);
    this.emitEvent(initialSnapshot);

    if (this.autoProgress) {
      this.scheduleMockProgress(jobId);
    }

    return initialSnapshot;
  }

  async listJobs(): Promise<JobSnapshot[]> {
    return Array.from(this.jobs.values());
  }

  async cancelJob(jobId: string): Promise<JobSnapshot> {
    const job = this.jobs.get(jobId);
    if (!job) {
      throw {
        code: 'job_not_found',
        message: 'Job not found.',
        retryable: false,
      } as AppError;
    }

    // Clear any pending timeouts
    this.clearTimeoutsForJob(jobId);

    // Terminal states are immutable
    if (job.state === 'succeeded' || job.state === 'failed' || job.state === 'cancelled') {
      return job;
    }

    if (job.state === 'queued') {
      const cancelledSnapshot: JobSnapshot = {
        ...job,
        state: 'cancelled',
        revision: job.revision + 1,
        progress: null,
        error: null,
      };
      this.jobs.set(jobId, cancelledSnapshot);
      this.emitEvent(cancelledSnapshot);
      return cancelledSnapshot;
    }

    // If running -> cancelling -> cancelled
    const cancellingSnapshot: JobSnapshot = {
      ...job,
      state: 'cancelling',
      revision: job.revision + 1,
    };
    this.jobs.set(jobId, cancellingSnapshot);
    this.emitEvent(cancellingSnapshot);

    const cancelledSnapshot: JobSnapshot = {
      ...cancellingSnapshot,
      state: 'cancelled',
      revision: cancellingSnapshot.revision + 1,
      progress: null,
      error: null,
    };
    this.jobs.set(jobId, cancelledSnapshot);
    this.emitEvent(cancelledSnapshot);

    return cancelledSnapshot;
  }

  async onJobChanged(listener: (event: JobEvent) => void): Promise<() => void> {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  private emitEvent(snapshot: JobSnapshot): void {
    const event: JobEvent = {
      apiVersion: API_VERSION,
      job: snapshot,
    };
    for (const listener of this.listeners) {
      try {
        listener(event);
      } catch {
        // Listener error ignored
      }
    }
  }

  private scheduleMockProgress(jobId: string): void {
    const timeouts: NodeJS.Timeout[] = [];

    // Step 1: running (progress indeterminate)
    timeouts.push(
      setTimeout(() => {
        const job = this.jobs.get(jobId);
        if (!job || job.state !== 'queued') return;
        const runningSnapshot: JobSnapshot = {
          ...job,
          state: 'running',
          revision: job.revision + 1,
          progress: null,
        };
        this.jobs.set(jobId, runningSnapshot);
        this.emitEvent(runningSnapshot);
      }, this.stepDelayMs),
    );

    // Step 2: progress 0.25
    timeouts.push(
      setTimeout(() => {
        const job = this.jobs.get(jobId);
        if (!job || job.state !== 'running') return;
        const updated: JobSnapshot = {
          ...job,
          revision: job.revision + 1,
          progress: 0.25,
        };
        this.jobs.set(jobId, updated);
        this.emitEvent(updated);
      }, this.stepDelayMs * 2),
    );

    // Step 3: progress 0.65
    timeouts.push(
      setTimeout(() => {
        const job = this.jobs.get(jobId);
        if (!job || job.state !== 'running') return;
        const updated: JobSnapshot = {
          ...job,
          revision: job.revision + 1,
          progress: 0.65,
        };
        this.jobs.set(jobId, updated);
        this.emitEvent(updated);
      }, this.stepDelayMs * 3),
    );

    // Step 4: succeeded progress 1.0
    timeouts.push(
      setTimeout(() => {
        const job = this.jobs.get(jobId);
        if (!job || job.state !== 'running') return;
        const succeededSnapshot: JobSnapshot = {
          ...job,
          state: 'succeeded',
          revision: job.revision + 1,
          progress: 1.0,
          error: null,
        };
        this.jobs.set(jobId, succeededSnapshot);
        this.emitEvent(succeededSnapshot);
        this.clearTimeoutsForJob(jobId);
      }, this.stepDelayMs * 4),
    );

    this.jobTimeouts.set(jobId, timeouts);
  }

  private clearTimeoutsForJob(jobId: string): void {
    const timeouts = this.jobTimeouts.get(jobId);
    if (timeouts) {
      for (const t of timeouts) clearTimeout(t);
      this.jobTimeouts.delete(jobId);
    }
  }

  clearAll(): void {
    for (const timeouts of this.jobTimeouts.values()) {
      for (const t of timeouts) clearTimeout(t);
    }
    this.jobTimeouts.clear();
    this.jobs.clear();
    this.listeners.clear();
  }
}
