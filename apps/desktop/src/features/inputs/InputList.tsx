import React from 'react';
import { InputInspection } from '../../api/contracts';
import { formatBytes, formatDuration } from '../../utils/formatters';
import { Badge } from '../common/Badge';

interface InputListProps {
  inputs: InputInspection[];
  selectedMediaId: string | null;
  onSelectMedia: (mediaId: string) => void;
  onRemoveMedia: (mediaId: string) => void;
  onAddFiles: () => void;
}

export const InputList: React.FC<InputListProps> = ({
  inputs,
  selectedMediaId,
  onSelectMedia,
  onRemoveMedia,
  onAddFiles,
}) => {
  return (
    <div className="panel-card">
      <div className="panel-card-title">
        <span>Source Media ({inputs.length})</span>
        <button
          type="button"
          onClick={onAddFiles}
          className="btn btn-secondary"
          style={{ padding: '3px 8px', fontSize: '11px' }}
        >
          + Add Files
        </button>
      </div>

      {inputs.length === 0 ? (
        <div style={{ padding: '16px 8px', textAlign: 'center', color: 'var(--text-tertiary)', fontSize: '12px' }}>
          No files added yet. Drop files above or click Add Files.
        </div>
      ) : (
        <div className="media-list" role="listbox" aria-label="Selected media files">
          {inputs.map((input) => {
            const isSelected = input.mediaId === selectedMediaId;
            const sizeLabel = formatBytes(input.sizeBytes);
            const durationLabel = formatDuration(input.durationMs);

            // Stream summary string
            const primaryStream = input.streams[0];
            let streamDesc = '';
            if (primaryStream) {
              if (primaryStream.width && primaryStream.height) {
                streamDesc = `${primaryStream.width}x${primaryStream.height} (${primaryStream.codec})`;
              } else if (primaryStream.sampleRateHz) {
                streamDesc = `${primaryStream.sampleRateHz} Hz (${primaryStream.codec})`;
              } else {
                streamDesc = primaryStream.codec;
              }
            }

            return (
              <div
                key={input.mediaId}
                className={`media-item ${isSelected ? 'selected' : ''}`}
                onClick={() => onSelectMedia(input.mediaId)}
                role="option"
                aria-selected={isSelected}
                tabIndex={0}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' || e.key === ' ') {
                    e.preventDefault();
                    onSelectMedia(input.mediaId);
                  }
                }}
              >
                <div className="media-item-info">
                  <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                    <Badge type="kind" value={input.kind} />
                    <span className="media-item-name" title={input.displayName}>
                      {input.displayName}
                    </span>
                  </div>
                  <div className="media-item-meta">
                    <span>{sizeLabel}</span>
                    {durationLabel && <span>• {durationLabel}</span>}
                    {streamDesc && <span>• {streamDesc}</span>}
                  </div>
                </div>

                <button
                  type="button"
                  className="btn-icon"
                  title="Remove from session"
                  aria-label={`Remove ${input.displayName}`}
                  onClick={(e) => {
                    e.stopPropagation();
                    onRemoveMedia(input.mediaId);
                  }}
                  style={{ fontSize: '14px', lineHeight: 1 }}
                >
                  ✕
                </button>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
};
