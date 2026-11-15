import React, { useCallback, useState } from 'react';

interface DropZoneProps {
  onSelectFiles: () => void;
  onDropPaths: (paths: string[]) => void;
  disabled?: boolean;
}

export const DropZone: React.FC<DropZoneProps> = ({
  onSelectFiles,
  onDropPaths,
  disabled = false,
}) => {
  const [isDragActive, setIsDragActive] = useState(false);

  const handleDragOver = useCallback(
    (e: React.DragEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (disabled) return;
      setIsDragActive(true);
    },
    [disabled],
  );

  const handleDragLeave = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragActive(false);
  }, []);

  const handleDrop = useCallback(
    (e: React.DragEvent) => {
      e.preventDefault();
      e.stopPropagation();
      setIsDragActive(false);
      if (disabled) return;

      // Extract paths only. NEVER read File bytes in JS (satisfies ARCHITECTURE.md & PRIVACY.md)
      const paths: string[] = [];
      if (e.dataTransfer.files && e.dataTransfer.files.length > 0) {
        for (let i = 0; i < e.dataTransfer.files.length; i++) {
          const file = e.dataTransfer.files[i];
          if (!file) continue;
          // In desktop/electron/tauri environments, file object may have a path property
          // In browser mock mode, use file.name
          const fileObj = file as unknown as { path?: string; name: string };
          const safePath = fileObj.path && fileObj.path.length > 0 ? fileObj.path : file.name;
          paths.push(safePath);
        }
      }

      if (paths.length > 0) {
        onDropPaths(paths);
      }
    },
    [disabled, onDropPaths],
  );

  return (
    <div
      className={`drop-zone ${isDragActive ? 'drag-active' : ''}`}
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      onDrop={handleDrop}
      onClick={onSelectFiles}
      role="button"
      tabIndex={0}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          onSelectFiles();
        }
      }}
      aria-label="Drop files here or click to select media"
    >
      <svg className="drop-zone-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.75">
        <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
        <polyline points="17 8 12 3 7 8" />
        <line x1="12" y1="3" x2="12" y2="15" />
      </svg>
      <div>
        <div className="drop-zone-title">Drop media files here</div>
        <div className="drop-zone-subtitle">or click to browse local files</div>
      </div>
    </div>
  );
};
