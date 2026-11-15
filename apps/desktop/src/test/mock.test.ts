import { describe, expect, it } from 'vitest';
import { JobEvent } from '../api/contracts';
import { MockDesktopApi } from '../api/mock';

describe('MockDesktopApi', () => {
  it('returns API version 1', async () => {
    const api = new MockDesktopApi({ autoProgress: false });
    expect(await api.getApiVersion()).toBe(1);
  });

  it('provides initial synthetic inputs and destination', async () => {
    const api = new MockDesktopApi({ autoProgress: false });
    const inputs = await api.selectInputs();
    expect(inputs.length).toBeGreaterThan(0);
    expect(inputs[0]?.kind).toBe('video');

    const dest = await api.selectDestination();
    expect(dest).not.toBeNull();
    expect(dest?.destinationId).toBe('mock-destination-default');
  });

  it('rejects remote URLs and invalid drop paths', async () => {
    const api = new MockDesktopApi({ autoProgress: false });

    await expect(api.importDroppedInputs(['https://evil.com/video.mp4'])).rejects.toMatchObject({
      code: 'unsupported_input',
    });

    await expect(api.importDroppedInputs(['file:///remote/server/share/file.mp4'])).rejects.toMatchObject({
      code: 'unsupported_input',
    });

    await expect(api.importDroppedInputs([''])).rejects.toMatchObject({
      code: 'invalid_request',
    });
  });

  it('imports valid local dropped files without reading bytes', async () => {
    const api = new MockDesktopApi({ autoProgress: false });
    const imported = await api.importDroppedInputs(['/Users/test/Documents/recording.wav']);
    expect(imported.length).toBe(1);
    expect(imported[0]?.kind).toBe('audio');
    expect(imported[0]?.displayName).toBe('recording.wav');
    expect(imported[0]?.streams[0]?.codec).toBe('pcm_s16le');
  });

  it('enforces format availability on enqueue', async () => {
    const api = new MockDesktopApi({ autoProgress: false });
    api.setFormatAvailability('mp4_h264_aac', false, 'tool_unavailable');

    await expect(
      api.enqueueJob({
        mediaId: 'mock-media-video-1',
        destinationId: 'mock-dest',
        outputName: 'test.mp4',
        formatId: 'mp4_h264_aac',
      }),
    ).rejects.toMatchObject({
      code: 'tool_unavailable',
    });
  });

  it('rejects invalid output names on enqueue', async () => {
    const api = new MockDesktopApi({ autoProgress: false });

    await expect(
      api.enqueueJob({
        mediaId: 'mock-media-video-1',
        destinationId: 'mock-dest',
        outputName: '../escape.mp4',
        formatId: 'mp4_h264_aac',
      }),
    ).rejects.toMatchObject({
      code: 'invalid_request',
    });
  });

  it('executes deterministic progress updates with increasing revisions', async () => {
    const api = new MockDesktopApi({ autoProgress: true, stepDelayMs: 15 });
    const events: JobEvent[] = [];

    const unlisten = await api.onJobChanged((ev) => {
      events.push(ev);
    });

    const initial = await api.enqueueJob({
      mediaId: 'mock-media-image-1',
      destinationId: 'mock-dest',
      outputName: 'converted.png',
      formatId: 'png',
    });

    expect(initial.state).toBe('queued');
    expect(initial.revision).toBe(1);

    // Wait for auto-progress steps to complete (4 steps * 15ms + buffer)
    await new Promise((resolve) => setTimeout(resolve, 120));

    expect(events.length).toBeGreaterThanOrEqual(4);

    // Verify monotonic revisions
    for (let i = 1; i < events.length; i++) {
      const prevRev = events[i - 1]!.job.revision;
      const currRev = events[i]!.job.revision;
      expect(currRev).toBeGreaterThan(prevRev);
    }

    const lastEvent = events[events.length - 1]!;
    expect(lastEvent.job.state).toBe('succeeded');
    expect(lastEvent.job.progress).toBe(1.0);
    expect(lastEvent.job.error).toBeNull();

    unlisten();
    api.clearAll();
  });

  it('cancels queued and running jobs with monotonic revision increment', async () => {
    const api = new MockDesktopApi({ autoProgress: false });

    const job = await api.enqueueJob({
      mediaId: 'mock-media-video-1',
      destinationId: 'mock-dest',
      outputName: 'cancel-me.mp4',
      formatId: 'mp4_h264_aac',
    });

    expect(job.state).toBe('queued');
    expect(job.revision).toBe(1);

    const cancelled = await api.cancelJob(job.jobId);
    expect(cancelled.state).toBe('cancelled');
    expect(cancelled.revision).toBe(2);
    expect(cancelled.error).toBeNull();

    // Second cancel on terminal state is idempotent
    const secondCancel = await api.cancelJob(job.jobId);
    expect(secondCancel.state).toBe('cancelled');
    expect(secondCancel.revision).toBe(2);
  });

  it('rejects cancellation for non-existent job ID', async () => {
    const api = new MockDesktopApi({ autoProgress: false });
    await expect(api.cancelJob('non-existent-id')).rejects.toMatchObject({
      code: 'job_not_found',
    });
  });
});
