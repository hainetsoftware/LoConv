import { describe, expect, it, vi } from 'vitest';
import { API_VERSION, ConversionRequest, JobEvent } from '../api/contracts';
import { JOB_CHANGED_EVENT, TauriDesktopApi } from '../api/tauri';

describe('TauriDesktopApi', () => {
  it('invokes get_api_version with empty args and returns integer', async () => {
    const mockInvoke = vi.fn().mockResolvedValue(1);
    const api = new TauriDesktopApi(mockInvoke);

    const version = await api.getApiVersion();
    expect(mockInvoke).toHaveBeenCalledWith('get_api_version', {});
    expect(version).toBe(1);
  });

  it('invokes select_inputs and validates returned array', async () => {
    const rawInspection = [
      {
        mediaId: 'm1',
        displayName: 'test.mp4',
        kind: 'video',
        sizeBytes: '1000',
        durationMs: 5000,
        container: 'mp4',
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
        ],
      },
    ];

    const mockInvoke = vi.fn().mockResolvedValue(rawInspection);
    const api = new TauriDesktopApi(mockInvoke);

    const result = await api.selectInputs();
    expect(mockInvoke).toHaveBeenCalledWith('select_inputs', {});
    expect(result.length).toBe(1);
    expect(result[0]?.displayName).toBe('test.mp4');
  });

  it('invokes import_dropped_inputs with paths argument', async () => {
    const mockInvoke = vi.fn().mockResolvedValue([]);
    const api = new TauriDesktopApi(mockInvoke);

    await api.importDroppedInputs(['/path/one.mp4', '/path/two.wav']);
    expect(mockInvoke).toHaveBeenCalledWith('import_dropped_inputs', {
      paths: ['/path/one.mp4', '/path/two.wav'],
    });
  });

  it('invokes select_destination and validates result', async () => {
    const mockInvoke = vi.fn().mockResolvedValue({
      destinationId: 'dest-1',
      displayName: 'Downloads',
    });
    const api = new TauriDesktopApi(mockInvoke);

    const dest = await api.selectDestination();
    expect(mockInvoke).toHaveBeenCalledWith('select_destination', {});
    expect(dest?.destinationId).toBe('dest-1');
  });

  it('invokes list_formats and validates descriptors', async () => {
    const rawFormats = [
      {
        id: 'png',
        label: 'PNG',
        extension: 'png',
        inputKinds: ['image'],
        backend: 'image',
        available: true,
        unavailableReason: null,
      },
    ];
    const mockInvoke = vi.fn().mockResolvedValue(rawFormats);
    const api = new TauriDesktopApi(mockInvoke);

    const formats = await api.listFormats();
    expect(mockInvoke).toHaveBeenCalledWith('list_formats', {});
    expect(formats[0]?.id).toBe('png');
  });

  it('invokes enqueue_job with validated ConversionRequest', async () => {
    const rawSnapshot = {
      jobId: 'job-1',
      mediaId: 'm1',
      formatId: 'mp4_h264_aac',
      outputName: 'out.mp4',
      state: 'queued',
      revision: 1,
      progress: null,
      error: null,
    };
    const mockInvoke = vi.fn().mockResolvedValue(rawSnapshot);
    const api = new TauriDesktopApi(mockInvoke);

    const req: ConversionRequest = {
      mediaId: 'm1',
      destinationId: 'd1',
      outputName: 'out.mp4',
      formatId: 'mp4_h264_aac',
    };

    const snapshot = await api.enqueueJob(req);
    expect(mockInvoke).toHaveBeenCalledWith('enqueue_job', { request: req });
    expect(snapshot.jobId).toBe('job-1');
  });

  it('invokes cancel_job with jobId argument', async () => {
    const rawSnapshot = {
      jobId: 'job-1',
      mediaId: 'm1',
      formatId: 'mp4_h264_aac',
      outputName: 'out.mp4',
      state: 'cancelled',
      revision: 2,
      progress: null,
      error: null,
    };
    const mockInvoke = vi.fn().mockResolvedValue(rawSnapshot);
    const api = new TauriDesktopApi(mockInvoke);

    const snapshot = await api.cancelJob('job-1');
    expect(mockInvoke).toHaveBeenCalledWith('cancel_job', { jobId: 'job-1' });
    expect(snapshot.state).toBe('cancelled');
  });

  it('subscribes to loconv://job-changed event and disposes listener', async () => {
    const unlistenMock = vi.fn();
    let registeredHandler: ((e: { payload: unknown }) => void) | null = null;

    const mockListen = vi.fn().mockImplementation((event: string, handler: (e: { payload: unknown }) => void) => {
      expect(event).toBe(JOB_CHANGED_EVENT);
      registeredHandler = handler;
      return Promise.resolve(unlistenMock);
    });

    const api = new TauriDesktopApi(vi.fn(), mockListen);

    const receivedEvents: JobEvent[] = [];
    const unlisten = await api.onJobChanged((ev) => {
      receivedEvents.push(ev);
    });

    expect(registeredHandler).not.toBeNull();

    // Trigger matching event
    const validPayload: JobEvent = {
      apiVersion: API_VERSION,
      job: {
        jobId: 'j1',
        mediaId: 'm1',
        formatId: 'mp3',
        outputName: 'song.mp3',
        state: 'running',
        revision: 2,
        progress: 0.5,
        error: null,
      },
    };

    registeredHandler!({ payload: validPayload });
    expect(receivedEvents.length).toBe(1);
    expect(receivedEvents[0]?.job.jobId).toBe('j1');

    // Trigger mismatched apiVersion (must be ignored)
    registeredHandler!({ payload: { ...validPayload, apiVersion: 99 } });
    expect(receivedEvents.length).toBe(1); // not incremented

    // Call unlisten
    unlisten();
    expect(unlistenMock).toHaveBeenCalled();
  });

  it('normalizes unexpected backend rejections to safe fixed internal error', async () => {
    const mockInvoke = vi.fn().mockRejectedValue(new Error('Sensitive path /var/log/kernel failed'));
    const api = new TauriDesktopApi(mockInvoke);

    await expect(api.selectInputs()).rejects.toEqual({
      code: 'internal',
      message: 'An unexpected internal error occurred.',
      retryable: false,
    });
  });

  it('preserves valid structured AppError from backend', async () => {
    const backendError = {
      code: 'tool_unavailable',
      message: 'Required local tool is unavailable.',
      retryable: false,
    };
    const mockInvoke = vi.fn().mockRejectedValue(backendError);
    const api = new TauriDesktopApi(mockInvoke);

    await expect(api.selectInputs()).rejects.toEqual(backendError);
  });
});
