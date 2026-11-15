import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import App from '../App';
import { API_VERSION, JobEvent } from '../api/contracts';
import { MockDesktopApi } from '../api/mock';

describe('App Desktop Frontend Component', () => {
  let mockApi: MockDesktopApi;

  beforeEach(() => {
    mockApi = new MockDesktopApi({ autoProgress: false });
  });

  it('renders initial UI with offline badge and theme toggle', async () => {
    render(<App api={mockApi} />);

    expect(screen.getByText('LoConv')).toBeInTheDocument();
    expect(screen.getByText('Offline')).toBeInTheDocument();
    expect(screen.getByText('No conversions in queue')).toBeInTheDocument();
  });

  it('toggles light/dark appearance and updates data-theme attribute', async () => {
    render(<App api={mockApi} />);

    const themeButton = screen.getByRole('button', { name: /Dark/i });
    expect(themeButton).toBeInTheDocument();

    const initialTheme = document.documentElement.getAttribute('data-theme');
    fireEvent.click(themeButton);
    const newTheme = document.documentElement.getAttribute('data-theme');

    expect(newTheme).not.toBe(initialTheme);
  });

  it('displays fixed incompatibility banner when backend API version differs', async () => {
    vi.spyOn(mockApi, 'getApiVersion').mockResolvedValue(2); // Incompatible version

    render(<App api={mockApi} />);

    await waitFor(() => {
      expect(screen.getByText(/Incompatible Backend API Version/i)).toBeInTheDocument();
    });

    // Main conversion UI should be hidden to protect state
    expect(screen.queryByText('Start Conversion')).not.toBeInTheDocument();
  });

  it('subscribes to job events BEFORE listJobs and reconciles revisions monotonically', async () => {
    const callOrder: string[] = [];
    let registeredListener: ((event: JobEvent) => void) | null = null;

    vi.spyOn(mockApi, 'getApiVersion').mockImplementation(async () => {
      callOrder.push('getApiVersion');
      return 1;
    });

    vi.spyOn(mockApi, 'onJobChanged').mockImplementation(async (listener) => {
      callOrder.push('onJobChanged');
      registeredListener = listener;
      return () => {};
    });

    vi.spyOn(mockApi, 'listJobs').mockImplementation(async () => {
      callOrder.push('listJobs');
      return [
        {
          jobId: 'job-1',
          mediaId: 'm1',
          formatId: 'mp4_h264_aac',
          outputName: 'video.mp4',
          state: 'running',
          revision: 5,
          progress: 0.5,
          error: null,
        },
      ];
    });

    render(<App api={mockApi} />);

    await waitFor(() => {
      expect(screen.getByText('video.mp4')).toBeInTheDocument();
    });

    // Ensure onJobChanged was called BEFORE listJobs
    const subIdx = callOrder.indexOf('onJobChanged');
    const listIdx = callOrder.indexOf('listJobs');
    expect(subIdx).toBeGreaterThanOrEqual(0);
    expect(listIdx).toBeGreaterThanOrEqual(0);
    expect(subIdx).toBeLessThan(listIdx);

    // Stale event (revision 4 <= existing revision 5) must be ignored
    expect(registeredListener).not.toBeNull();
    act(() => {
      registeredListener!({
        apiVersion: API_VERSION,
        job: {
          jobId: 'job-1',
          mediaId: 'm1',
          formatId: 'mp4_h264_aac',
          outputName: 'video.mp4',
          state: 'queued', // older state
          revision: 4,
          progress: null,
          error: null,
        },
      });
    });

    // State should NOT regress to queued
    expect(screen.getByText('rev 5')).toBeInTheDocument();
    expect(screen.queryByText('rev 4')).not.toBeInTheDocument();

    // Newer event (revision 6 > 5) should update state
    act(() => {
      registeredListener!({
        apiVersion: API_VERSION,
        job: {
          jobId: 'job-1',
          mediaId: 'm1',
          formatId: 'mp4_h264_aac',
          outputName: 'video.mp4',
          state: 'succeeded',
          revision: 6,
          progress: 1.0,
          error: null,
        },
      });
    });

    await waitFor(() => {
      expect(screen.getByText('rev 6')).toBeInTheDocument();
      expect(screen.getByText('Completed')).toBeInTheDocument();
    });
  });

  it('selects media, populates format choices, and enforces format availability', async () => {
    render(<App api={mockApi} />);

    // Click Add Files
    const addFilesButton = screen.getByRole('button', { name: /\+ Add Files/i });
    fireEvent.click(addFilesButton);

    await waitFor(() => {
      expect(screen.getByText('synthetic-landscape.mp4')).toBeInTheDocument();
    });

    // Click on synthetic-landscape.mp4
    const videoOption = screen.getByText('synthetic-landscape.mp4');
    fireEvent.click(videoOption);

    // Check available and unavailable formats for video
    // MP4 should be available
    const mp4Format = screen.getByText('.mp4');
    expect(mp4Format).toBeInTheDocument();

    // MP3 is marked unavailable in INITIAL_MOCK_FORMATS
    const mp3Format = screen.getByText('.mp3');
    expect(mp3Format.closest('.format-card')).toHaveClass('disabled');
  });

  it('validates output basename and disables conversion button when invalid', async () => {
    render(<App api={mockApi} />);

    // Add files
    fireEvent.click(screen.getByRole('button', { name: /\+ Add Files/i }));
    await waitFor(() => {
      expect(screen.getByText('synthetic-landscape.mp4')).toBeInTheDocument();
    });

    // Select destination
    fireEvent.click(screen.getByRole('button', { name: /Select destination directory/i }));

    const outputInput = screen.getByLabelText(/Output Filename/i);

    // Enter invalid path with slash
    fireEvent.change(outputInput, { target: { value: 'bad/filename.mp4' } });
    expect(screen.getByText(/cannot contain path separators/i)).toBeInTheDocument();

    const startButton = screen.getByRole('button', { name: /Start Conversion/i });
    expect(startButton).toBeDisabled();

    // Enter valid filename
    fireEvent.change(outputInput, { target: { value: 'valid-output.mp4' } });
    expect(screen.queryByText(/cannot contain path separators/i)).not.toBeInTheDocument();
    await waitFor(() => {
      expect(startButton).toBeEnabled();
    });
  });

  it('enqueues conversion job and allows cancellation', async () => {
    render(<App api={mockApi} />);

    // Add files
    fireEvent.click(screen.getByRole('button', { name: /\+ Add Files/i }));
    await waitFor(() => {
      expect(screen.getByText('synthetic-landscape.mp4')).toBeInTheDocument();
    });

    // Select destination
    fireEvent.click(screen.getByRole('button', { name: /Select destination directory/i }));

    const startButton = screen.getByRole('button', { name: /Start Conversion/i });
    await waitFor(() => {
      expect(startButton).toBeEnabled();
    });
    fireEvent.click(startButton);

    // Job card appears in queue
    let cancelButton: HTMLElement;
    await waitFor(() => {
      cancelButton = screen.getByRole('button', { name: /Cancel conversion/i });
      expect(cancelButton).toBeInTheDocument();
    });

    // Click cancel
    fireEvent.click(cancelButton!);

    await waitFor(() => {
      expect(screen.getByText('Cancelled')).toBeInTheDocument();
    });
  });

  it('renders probe strings safely as text without HTML execution', async () => {
    const maliciousInput = {
      mediaId: 'malicious-media-1',
      displayName: '<script>alert("xss")</script>clean.mp4',
      kind: 'video' as const,
      sizeBytes: '1024',
      durationMs: 1000,
      container: '<img src=x onerror=alert(1)>',
      streams: [
        {
          index: 0,
          kind: 'video' as const,
          codec: '<b onclick="evil()">h264</b>',
          width: 1920,
          height: 1080,
          sampleRateHz: null,
          channels: null,
        },
      ],
    };

    const xssApi = new MockDesktopApi({
      autoProgress: false,
      initialInputs: [maliciousInput],
    });

    render(<App api={xssApi} />);

    // Add files
    fireEvent.click(screen.getByRole('button', { name: /\+ Add Files/i }));

    await waitFor(() => {
      // The text should be rendered as literal text content, not parsed HTML
      const element = screen.getByText('<script>alert("xss")</script>clean.mp4');
      expect(element).toBeInTheDocument();
      expect(element.tagName.toLowerCase()).not.toBe('script');
    });
  });
});
