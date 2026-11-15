import { describe, expect, it } from 'vitest';
import { normalizeError, validateOutputBasename } from '../api/validation';
import { formatBytes, formatDuration } from '../utils/formatters';

describe('Validation & Formatters', () => {
  describe('validateOutputBasename', () => {
    it('accepts valid simple basenames', () => {
      expect(validateOutputBasename('output.mp4', 'mp4').valid).toBe(true);
      expect(validateOutputBasename('my-great-file_123.jpg', 'jpg').valid).toBe(true);
      expect(validateOutputBasename('audio.wav', 'wav').valid).toBe(true);
    });

    it('rejects path separators', () => {
      expect(validateOutputBasename('sub/dir.mp4', 'mp4').valid).toBe(false);
      expect(validateOutputBasename('sub\\dir.mp4', 'mp4').valid).toBe(false);
      expect(validateOutputBasename('../dir.mp4', 'mp4').valid).toBe(false);
    });

    it('rejects dot and double-dot', () => {
      expect(validateOutputBasename('.', 'mp4').valid).toBe(false);
      expect(validateOutputBasename('..', 'mp4').valid).toBe(false);
    });

    it('rejects control characters and NUL', () => {
      expect(validateOutputBasename('test\x00file.mp4', 'mp4').valid).toBe(false);
      expect(validateOutputBasename('test\nfile.mp4', 'mp4').valid).toBe(false);
      expect(validateOutputBasename('test\rfile.mp4', 'mp4').valid).toBe(false);
      expect(validateOutputBasename('test\x1Ffile.mp4', 'mp4').valid).toBe(false);
    });

    it('rejects trailing spaces and periods', () => {
      expect(validateOutputBasename('test.mp4 ', 'mp4').valid).toBe(false);
      expect(validateOutputBasename('test.mp4.', 'mp4').valid).toBe(false);
    });

    it('rejects Windows reserved device names', () => {
      expect(validateOutputBasename('CON.mp4', 'mp4').valid).toBe(false);
      expect(validateOutputBasename('con.mp4', 'mp4').valid).toBe(false);
      expect(validateOutputBasename('prn.png', 'png').valid).toBe(false);
      expect(validateOutputBasename('aux.wav', 'wav').valid).toBe(false);
      expect(validateOutputBasename('NUL.mp3', 'mp3').valid).toBe(false);
      expect(validateOutputBasename('com1.webm', 'webm').valid).toBe(false);
      expect(validateOutputBasename('lpt9.jpg', 'jpg').valid).toBe(false);
    });

    it('rejects strings exceeding 255 UTF-8 bytes', () => {
      const longName = 'a'.repeat(252) + '.mp4'; // 256 bytes
      expect(validateOutputBasename(longName, 'mp4').valid).toBe(false);

      const valid255 = 'a'.repeat(251) + '.mp4'; // 255 bytes
      expect(validateOutputBasename(valid255, 'mp4').valid).toBe(true);

      // Multi-byte UTF-8 test (3 bytes each)
      const multiByte = '世'.repeat(85) + '.mp4'; // 85*3 + 4 = 259 bytes
      expect(validateOutputBasename(multiByte, 'mp4').valid).toBe(false);
    });

    it('enforces expected file extension', () => {
      expect(validateOutputBasename('output.mp4', 'mp4').valid).toBe(true);
      expect(validateOutputBasename('output.wav', 'mp4').valid).toBe(false);
      expect(validateOutputBasename('output.mp4', 'webm').valid).toBe(false);
    });
  });

  describe('normalizeError', () => {
    it('preserves conforming AppError', () => {
      const validErr = {
        code: 'tool_unavailable',
        message: 'Required local tool is unavailable.',
        retryable: false,
      };
      expect(normalizeError(validErr)).toEqual(validErr);
    });

    it('maps unexpected errors to safe internal message without leaking raw content', () => {
      const exception = new Error('/Users/secret/file.mp4 does not exist at 0xdeadbeef');
      const normalized = normalizeError(exception);
      expect(normalized.code).toBe('internal');
      expect(normalized.message).toBe('An unexpected internal error occurred.');
      expect(normalized.retryable).toBe(false);
      expect(normalized.message).not.toContain('/Users/secret');
    });

    it('maps random strings or objects to safe internal error', () => {
      expect(normalizeError('raw error text').code).toBe('internal');
      expect(normalizeError({ code: 'unknown_code_xyz' }).code).toBe('internal');
      expect(normalizeError(null).code).toBe('internal');
    });
  });

  describe('formatBytes', () => {
    it('formats small and typical byte counts', () => {
      expect(formatBytes('0')).toBe('0 B');
      expect(formatBytes('512')).toBe('512 B');
      expect(formatBytes('1024')).toBe('1.0 KB');
      expect(formatBytes('10485760')).toBe('10 MB');
      expect(formatBytes('52428800')).toBe('50 MB');
    });

    it('handles large u64 strings without numeric overflow', () => {
      // 9007199254740993 is larger than Number.MAX_SAFE_INTEGER
      const result = formatBytes('9007199254740993');
      expect(result).toContain('TB');
    });
  });

  describe('formatDuration', () => {
    it('formats milliseconds to MM:SS or HH:MM:SS', () => {
      expect(formatDuration(null)).toBeNull();
      expect(formatDuration(-10)).toBeNull();
      expect(formatDuration(0)).toBe('0:00');
      expect(formatDuration(45000)).toBe('0:45');
      expect(formatDuration(125000)).toBe('2:05');
      expect(formatDuration(3665000)).toBe('1:01:05');
    });
  });
});
