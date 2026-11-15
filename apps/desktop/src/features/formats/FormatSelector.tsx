import React from 'react';
import { FormatDescriptor, FormatId, InputInspection } from '../../api/contracts';
import { validateOutputBasename } from '../../api/validation';

interface FormatSelectorProps {
  formats: FormatDescriptor[];
  selectedInput: InputInspection | null;
  selectedFormatId: FormatId | null;
  onSelectFormatId: (id: FormatId) => void;
  outputName: string;
  onChangeOutputName: (name: string) => void;
}

export const FormatSelector: React.FC<FormatSelectorProps> = ({
  formats,
  selectedInput,
  selectedFormatId,
  onSelectFormatId,
  outputName,
  onChangeOutputName,
}) => {
  // Filter formats based on selected media kind
  const compatibleFormats = selectedInput
    ? formats.filter((f) => f.inputKinds.includes(selectedInput.kind))
    : formats;

  const currentFormat = formats.find((f) => f.id === selectedFormatId);
  const validation = currentFormat
    ? validateOutputBasename(outputName, currentFormat.extension)
    : { valid: false, error: 'No format selected' };

  return (
    <div className="panel-card">
      <div className="panel-card-title">
        <span>Output Configuration</span>
        {selectedInput && (
          <span style={{ fontSize: '11px', color: 'var(--text-tertiary)', fontWeight: 'normal' }}>
            Converting {selectedInput.kind}
          </span>
        )}
      </div>

      {!selectedInput ? (
        <div style={{ padding: '16px 8px', textAlign: 'center', color: 'var(--text-tertiary)', fontSize: '12px' }}>
          Select a source file from above to configure output format.
        </div>
      ) : (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
          <div className="form-group">
            <label className="form-label">Target Format</label>
            <div className="format-grid" role="radiogroup" aria-label="Available output formats">
              {compatibleFormats.map((fmt) => {
                const isSelected = fmt.id === selectedFormatId;
                const isDisabled = !fmt.available;

                return (
                  <div
                    key={fmt.id}
                    className={`format-card ${isSelected ? 'selected' : ''} ${isDisabled ? 'disabled' : ''}`}
                    onClick={() => {
                      if (!isDisabled) onSelectFormatId(fmt.id);
                    }}
                    role="radio"
                    aria-checked={isSelected}
                    aria-disabled={isDisabled}
                    tabIndex={isDisabled ? -1 : 0}
                    onKeyDown={(e) => {
                      if (!isDisabled && (e.key === 'Enter' || e.key === ' ')) {
                        e.preventDefault();
                        onSelectFormatId(fmt.id);
                      }
                    }}
                  >
                    <div className="format-card-header">
                      <span className="format-card-label">.{fmt.extension}</span>
                      <span className="format-card-backend">{fmt.backend}</span>
                    </div>
                    <span className="format-card-desc">{fmt.label}</span>
                    {isDisabled && (
                      <span className="format-unavailable-note" title={fmt.unavailableReason ?? 'Unavailable'}>
                        Unavailable: {fmt.unavailableReason ?? 'tool missing'}
                      </span>
                    )}
                  </div>
                );
              })}
            </div>
          </div>

          <div className="form-group">
            <label className="form-label" htmlFor="output-name-input">
              Output Filename
            </label>
            <input
              id="output-name-input"
              type="text"
              className="form-input"
              value={outputName}
              onChange={(e) => onChangeOutputName(e.target.value)}
              placeholder="e.g. converted-file.mp4"
            />
            {!validation.valid && outputName.length > 0 && (
              <span className="form-error">{validation.error}</span>
            )}
            <span className="form-hint">
              Single local basename without directory separators. Maximum 255 UTF-8 bytes.
            </span>
          </div>
        </div>
      )}
    </div>
  );
};
