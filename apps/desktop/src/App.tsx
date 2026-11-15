import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  API_VERSION,
  AppError,
  ConversionRequest,
  DesktopApi,
  DestinationSelection,
  FormatDescriptor,
  FormatId,
  InputInspection,
  JobEvent,
  JobSnapshot,
} from './api/contracts';
import { getDesktopApi, MockDesktopApi } from './api';
import { validateOutputBasename } from './api/validation';
import { Header } from './components/Header';
import { IncompatibleBanner } from './components/IncompatibleBanner';
import { Alert } from './features/common/Alert';
import { DestinationSelector } from './features/destination/DestinationSelector';
import { FormatSelector } from './features/formats/FormatSelector';
import { DropZone } from './features/inputs/DropZone';
import { InputList } from './features/inputs/InputList';
import { ConversionQueue } from './features/queue/ConversionQueue';
import './App.css';

interface AppProps {
  api?: DesktopApi;
}

export const App: React.FC<AppProps> = ({ api: injectedApi }) => {
  const [api, setApi] = useState<DesktopApi | null>(() => injectedApi ?? null);
  const [apiVersion, setApiVersion] = useState<number | null>(null);
  const [isIncompatible, setIsIncompatible] = useState(false);
  const [initError, setInitError] = useState<AppError | string | null>(null);

  // Theme state
  const [theme, setTheme] = useState<'light' | 'dark'>(() => {
    if (typeof window !== 'undefined' && typeof window.matchMedia === 'function' && window.matchMedia('(prefers-color-scheme: dark)').matches) {
      return 'dark';
    }
    return 'light';
  });

  // Data states
  const [inputs, setInputs] = useState<InputInspection[]>([]);
  const [selectedMediaId, setSelectedMediaId] = useState<string | null>(null);
  const [destination, setDestination] = useState<DestinationSelection | null>(null);
  const [formats, setFormats] = useState<FormatDescriptor[]>([]);
  const [selectedFormatId, setSelectedFormatId] = useState<FormatId | null>(null);
  const [outputName, setOutputName] = useState<string>('');
  const [isOutputCustomized, setIsOutputCustomized] = useState(false);

  // Queue state: jobs map keyed by jobId
  const [jobs, setJobs] = useState<JobSnapshot[]>([]);
  const jobsRef = useRef<JobSnapshot[]>([]);
  jobsRef.current = jobs;

  const [activeError, setActiveError] = useState<AppError | string | null>(null);
  const [isEnqueueing, setIsEnqueueing] = useState(false);
  const [cancellingJobIds, setCancellingJobIds] = useState<Set<string>>(new Set());

  // Initialize API
  useEffect(() => {
    if (!api) {
      try {
        const resolved = getDesktopApi();
        setApi(resolved);
      } catch (err) {
        setInitError(err instanceof Error ? err.message : 'Failed to initialize desktop API.');
      }
    }
  }, [api]);

  // Apply theme
  useEffect(() => {
    document.documentElement.setAttribute('data-theme', theme);
  }, [theme]);

  const toggleTheme = useCallback(() => {
    setTheme((prev) => (prev === 'light' ? 'dark' : 'light'));
  }, []);

  // Job reconciliation helper
  const reconcileJobSnapshot = useCallback((newSnapshot: JobSnapshot) => {
    setJobs((prevJobs) => {
      const existingIndex = prevJobs.findIndex((j) => j.jobId === newSnapshot.jobId);
      if (existingIndex === -1) {
        return [newSnapshot, ...prevJobs];
      }
      const existing = prevJobs[existingIndex];
      if (!existing) return prevJobs;

      // Ignore equal or older revisions to prevent regressions (ARCHITECTURE.md)
      if (newSnapshot.revision <= existing.revision) {
        return prevJobs;
      }

      const updated = [...prevJobs];
      updated[existingIndex] = newSnapshot;
      return updated;
    });
  }, []);

  // Startup lifecycle: Check API version, Subscribe before listJobs, listFormats
  useEffect(() => {
    if (!api) return;

    let unlisten: (() => void) | null = null;
    let isCancelled = false;

    async function initialize() {
      if (!api) return;
      try {
        // 1. Check API Version
        const ver = await api.getApiVersion();
        if (isCancelled) return;
        setApiVersion(ver);

        if (ver !== API_VERSION) {
          setIsIncompatible(true);
          return;
        }

        // 2. Load Formats
        const loadedFormats = await api.listFormats();
        if (isCancelled) return;
        setFormats(loadedFormats);

        // 3. Subscribe to Job Events BEFORE listJobs (ARCHITECTURE.md)
        unlisten = await api.onJobChanged((event: JobEvent) => {
          if (isCancelled) return;
          if (event.apiVersion !== API_VERSION) return;
          reconcileJobSnapshot(event.job);
        });

        // 4. Fetch authoritative initial jobs list
        const initialJobs = await api.listJobs();
        if (isCancelled) return;
        for (const job of initialJobs) {
          reconcileJobSnapshot(job);
        }
      } catch (err) {
        if (isCancelled) return;
        const appErr = err as AppError;
        setInitError(appErr.message ?? 'Failed to initialize backend services.');
      }
    }

    void initialize();

    return () => {
      isCancelled = true;
      if (unlisten) {
        unlisten();
      }
    };
  }, [api, reconcileJobSnapshot]);

  const isMock = useMemo(() => api instanceof MockDesktopApi, [api]);

  const selectedInput = useMemo(() => {
    return inputs.find((i) => i.mediaId === selectedMediaId) ?? null;
  }, [inputs, selectedMediaId]);

  const currentFormat = useMemo(() => {
    return formats.find((f) => f.id === selectedFormatId) ?? null;
  }, [formats, selectedFormatId]);

  // When selected input changes, auto-select a compatible format and suggest output filename
  useEffect(() => {
    if (!selectedInput) return;

    // Pick first available format compatible with this input kind
    const compatible = formats.filter(
      (f) => f.inputKinds.includes(selectedInput.kind) && f.available,
    );
    const preferredFormat = compatible[0] ?? null;

    if (preferredFormat && (!selectedFormatId || !formats.find((f) => f.id === selectedFormatId)?.inputKinds.includes(selectedInput.kind))) {
      setSelectedFormatId(preferredFormat.id);
    }

    // Auto-suggest basename
    if (!isOutputCustomized) {
      const stem = selectedInput.displayName.replace(/\.[^/.]+$/, '');
      const ext = preferredFormat?.extension ?? 'mp4';
      setOutputName(`${stem}.${ext}`);
    }
  }, [selectedInput, formats, selectedFormatId, isOutputCustomized]);

  // When format changes and outputName wasn't custom-typed, update extension
  const handleSelectFormat = useCallback(
    (formatId: FormatId) => {
      setSelectedFormatId(formatId);
      const fmt = formats.find((f) => f.id === formatId);
      if (fmt && !isOutputCustomized && selectedInput) {
        const stem = selectedInput.displayName.replace(/\.[^/.]+$/, '');
        setOutputName(`${stem}.${fmt.extension}`);
      }
    },
    [formats, isOutputCustomized, selectedInput],
  );

  const handleOutputNameChange = useCallback((name: string) => {
    setOutputName(name);
    setIsOutputCustomized(true);
  }, []);

  // File selection handlers
  const handleSelectFiles = useCallback(async () => {
    if (!api || isIncompatible) return;
    try {
      setActiveError(null);
      const selected = await api.selectInputs();
      if (selected.length > 0) {
        setInputs((prev) => {
          const newMap = new Map(prev.map((i) => [i.mediaId, i]));
          for (const item of selected) newMap.set(item.mediaId, item);
          return Array.from(newMap.values());
        });
        if (!selectedMediaId) {
          setSelectedMediaId(selected[0]?.mediaId ?? null);
        }
      }
    } catch (err) {
      setActiveError(err as AppError);
    }
  }, [api, isIncompatible, selectedMediaId]);

  const handleDropPaths = useCallback(
    async (paths: string[]) => {
      if (!api || isIncompatible) return;
      try {
        setActiveError(null);
        const imported = await api.importDroppedInputs(paths);
        if (imported.length > 0) {
          setInputs((prev) => {
            const newMap = new Map(prev.map((i) => [i.mediaId, i]));
            for (const item of imported) newMap.set(item.mediaId, item);
            return Array.from(newMap.values());
          });
          if (!selectedMediaId) {
            setSelectedMediaId(imported[0]?.mediaId ?? null);
          }
        }
      } catch (err) {
        setActiveError(err as AppError);
      }
    },
    [api, isIncompatible, selectedMediaId],
  );

  const handleRemoveMedia = useCallback((mediaId: string) => {
    setInputs((prev) => prev.filter((i) => i.mediaId !== mediaId));
    setSelectedMediaId((prev) => (prev === mediaId ? null : prev));
  }, []);

  // Destination selection handler
  const handleSelectDestination = useCallback(async () => {
    if (!api || isIncompatible) return;
    try {
      setActiveError(null);
      const dest = await api.selectDestination();
      if (dest) {
        setDestination(dest);
      }
    } catch (err) {
      setActiveError(err as AppError);
    }
  }, [api, isIncompatible]);

  // Conversion enqueuing
  const canEnqueue = useMemo(() => {
    if (!selectedInput || !destination || !currentFormat || !currentFormat.available) {
      return false;
    }
    const val = validateOutputBasename(outputName, currentFormat.extension);
    return val.valid && !isEnqueueing;
  }, [selectedInput, destination, currentFormat, outputName, isEnqueueing]);

  const handleEnqueue = useCallback(async () => {
    if (!api || !canEnqueue || !selectedInput || !destination || !currentFormat) return;

    const request: ConversionRequest = {
      mediaId: selectedInput.mediaId,
      destinationId: destination.destinationId,
      outputName: outputName.trim(),
      formatId: currentFormat.id,
    };

    try {
      setIsEnqueueing(true);
      setActiveError(null);
      const snapshot = await api.enqueueJob(request);
      reconcileJobSnapshot(snapshot);
    } catch (err) {
      setActiveError(err as AppError);
    } finally {
      setIsEnqueueing(false);
    }
  }, [api, canEnqueue, selectedInput, destination, currentFormat, outputName, reconcileJobSnapshot]);

  // Cancellation handler
  const handleCancelJob = useCallback(
    async (jobId: string) => {
      if (!api) return;
      try {
        setCancellingJobIds((prev) => new Set(prev).add(jobId));
        const updatedSnapshot = await api.cancelJob(jobId);
        reconcileJobSnapshot(updatedSnapshot);
      } catch (err) {
        setActiveError(err as AppError);
      } finally {
        setCancellingJobIds((prev) => {
          const next = new Set(prev);
          next.delete(jobId);
          return next;
        });
      }
    },
    [api, reconcileJobSnapshot],
  );

  return (
    <div className="app-window">
      <Header isMock={isMock} theme={theme} onToggleTheme={toggleTheme} />

      {initError && (
        <div style={{ padding: '12px 18px' }}>
          <Alert error={initError} onDismiss={() => setInitError(null)} />
        </div>
      )}

      {isIncompatible ? (
        <IncompatibleBanner version={apiVersion} expectedVersion={API_VERSION} />
      ) : (
        <div className="app-body">
          {/* Left Sidebar: Inputs & Configuration */}
          <aside className="sidebar-panel">
            <DropZone onSelectFiles={handleSelectFiles} onDropPaths={handleDropPaths} />

            <InputList
              inputs={inputs}
              selectedMediaId={selectedMediaId}
              onSelectMedia={setSelectedMediaId}
              onRemoveMedia={handleRemoveMedia}
              onAddFiles={handleSelectFiles}
            />

            <DestinationSelector
              destination={destination}
              onSelectDestination={handleSelectDestination}
            />

            <FormatSelector
              formats={formats}
              selectedInput={selectedInput}
              selectedFormatId={selectedFormatId}
              onSelectFormatId={handleSelectFormat}
              outputName={outputName}
              onChangeOutputName={handleOutputNameChange}
            />

            <button
              type="button"
              className="btn btn-primary"
              disabled={!canEnqueue}
              onClick={handleEnqueue}
              style={{ padding: '10px 16px', fontSize: '13px' }}
            >
              {isEnqueueing ? 'Queueing...' : 'Start Conversion'}
            </button>
          </aside>

          {/* Right Main Panel: Queue & Progress */}
          <main className="content-panel">
            {activeError && (
              <Alert error={activeError} onDismiss={() => setActiveError(null)} />
            )}

            <ConversionQueue
              jobs={jobs}
              onCancelJob={handleCancelJob}
              cancellingJobIds={cancellingJobIds}
            />
          </main>
        </div>
      )}
    </div>
  );
};

export default App;
