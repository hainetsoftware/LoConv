/**
 * Safely format file sizes from decimal u64 string without losing precision.
 */
export function formatBytes(sizeBytes: string): string {
  try {
    const bytes = BigInt(sizeBytes);
    if (bytes < 0n) return '0 B';

    const units = ['B', 'KB', 'MB', 'GB', 'TB', 'PB'];
    let unitIndex = 0;
    let value = Number(bytes);

    // If bytes is large, scale down BigInt first to avoid Number overflow
    if (bytes >= 1024n * 1024n * 1024n * 1024n) {
      // TB range or higher
      const tb = Number(bytes / (1024n * 1024n * 1024n * 1024n));
      return `${tb.toFixed(2)} TB`;
    }

    while (value >= 1024 && unitIndex < units.length - 1) {
      value /= 1024;
      unitIndex++;
    }

    return `${value < 10 && unitIndex > 0 ? value.toFixed(1) : Math.round(value)} ${units[unitIndex]}`;
  } catch {
    return 'Unknown size';
  }
}

/**
 * Safely format duration in milliseconds to MM:SS or HH:MM:SS.
 */
export function formatDuration(durationMs: number | null): string | null {
  if (durationMs === null || !Number.isFinite(durationMs) || durationMs < 0) {
    return null;
  }

  const totalSeconds = Math.floor(durationMs / 1000);
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;

  if (hours > 0) {
    return `${hours}:${minutes.toString().padStart(2, '0')}:${seconds.toString().padStart(2, '0')}`;
  }
  return `${minutes}:${seconds.toString().padStart(2, '0')}`;
}
