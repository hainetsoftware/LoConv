import { describe, expect, it } from 'vitest';
import formatsFixture from '../../../../contracts/fixtures/formats.json';
import inspectionFixture from '../../../../contracts/fixtures/inspection.json';
import jobEventFixture from '../../../../contracts/fixtures/job-event.json';
import requestFixture from '../../../../contracts/fixtures/request.json';
import {
  validateConversionRequest,
  validateFormatDescriptor,
  validateInputInspection,
  validateJobEvent,
  validateJobSnapshot,
} from '../api/validation';

describe('Synthetic Wire Fixture Conformance', () => {
  it('validates formats.json fixture against FormatDescriptor specification', () => {
    expect(Array.isArray(formatsFixture)).toBe(true);
    expect(formatsFixture.length).toBe(9);

    const validated = formatsFixture.map(validateFormatDescriptor);
    expect(validated.length).toBe(9);

    // Verify expected formats from contract
    const ids = validated.map((f) => f.id);
    expect(ids).toContain('mp4_h264_aac');
    expect(ids).toContain('mkv_h264_aac');
    expect(ids).toContain('avi_mpeg4_mp3');
    expect(ids).toContain('webm_vp9_opus');
    expect(ids).toContain('mp3');
    expect(ids).toContain('wav_pcm16');
    expect(ids).toContain('png');
    expect(ids).toContain('jpeg');
    expect(ids).toContain('webp');

    // Verify unavailable format has unavailableReason
    const mp3 = validated.find((f) => f.id === 'mp3');
    expect(mp3?.available).toBe(false);
    expect(mp3?.unavailableReason).toBe('tool_unavailable');

    // Verify available format has null unavailableReason
    const png = validated.find((f) => f.id === 'png');
    expect(png?.available).toBe(true);
    expect(png?.unavailableReason).toBeNull();
  });

  it('validates request.json fixture against ConversionRequest specification', () => {
    const validated = validateConversionRequest(requestFixture);
    expect(validated.mediaId).toBe('synthetic-media-1');
    expect(validated.destinationId).toBe('synthetic-destination-1');
    expect(validated.outputName).toBe('synthetic-tone.mp3');
    expect(validated.formatId).toBe('mp3');
  });

  it('validates inspection.json fixture against InputInspection specification', () => {
    const validated = validateInputInspection(inspectionFixture);
    expect(validated.mediaId).toBe('synthetic-media-1');
    expect(validated.displayName).toBe('synthetic-tone.wav');
    expect(validated.kind).toBe('audio');
    // Ensure sizeBytes preserves 64-bit integer as string (exceeds JS MAX_SAFE_INTEGER)
    expect(validated.sizeBytes).toBe('9007199254740993');
    expect(BigInt(validated.sizeBytes)).toBe(9007199254740993n);
    expect(validated.durationMs).toBe(1000.0);
    expect(validated.container).toBe('wav');
    expect(validated.streams.length).toBe(1);

    const stream = validated.streams[0]!;
    expect(stream.index).toBe(0);
    expect(stream.kind).toBe('audio');
    expect(stream.codec).toBe('pcm_s16le');
    expect(stream.width).toBeNull();
    expect(stream.height).toBeNull();
    expect(stream.sampleRateHz).toBe(48000);
    expect(stream.channels).toBe(1);
  });

  it('validates job-event.json fixture against JobEvent specification', () => {
    const validated = validateJobEvent(jobEventFixture);
    expect(validated.apiVersion).toBe(1);
    expect(validated.job.jobId).toBe('synthetic-job-1');
    expect(validated.job.mediaId).toBe('synthetic-media-1');
    expect(validated.job.formatId).toBe('mp3');
    expect(validated.job.outputName).toBe('synthetic-tone.mp3');
    expect(validated.job.state).toBe('failed');
    expect(validated.job.revision).toBe(3);
    expect(validated.job.progress).toBeNull();
    expect(validated.job.error).toEqual({
      code: 'tool_unavailable',
      message: 'Required local tool is unavailable.',
      retryable: false,
    });
  });

  it('rejects malformed and out-of-range fixtures', () => {
    // Unknown field / invalid structure
    expect(() => validateFormatDescriptor({ ...formatsFixture[0], id: 'invalid_format' })).toThrow();
    expect(() => validateInputInspection({ ...inspectionFixture, sizeBytes: 'not-a-number' })).toThrow();
    expect(() => validateInputInspection({ ...inspectionFixture, durationMs: -50 })).toThrow();
    expect(() => validateJobEvent({ ...jobEventFixture, apiVersion: 99 })).toThrow();
    expect(() =>
      validateJobSnapshot({
        jobId: 'j1',
        mediaId: 'm1',
        formatId: 'mp3',
        outputName: 'test.mp3',
        state: 'running',
        revision: 0, // revision must be >= 1
        progress: null,
        error: null,
      }),
    ).toThrow();
    expect(() =>
      validateJobSnapshot({
        jobId: 'j1',
        mediaId: 'm1',
        formatId: 'mp3',
        outputName: 'test.mp3',
        state: 'running',
        revision: 1,
        progress: 1.5, // progress must be <= 1.0
        error: null,
      }),
    ).toThrow();
  });
});
