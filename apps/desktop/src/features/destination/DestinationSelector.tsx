import React from 'react';
import { DestinationSelection } from '../../api/contracts';

interface DestinationSelectorProps {
  destination: DestinationSelection | null;
  onSelectDestination: () => void;
  disabled?: boolean;
}

export const DestinationSelector: React.FC<DestinationSelectorProps> = ({
  destination,
  onSelectDestination,
  disabled = false,
}) => {
  return (
    <div className="panel-card">
      <div className="panel-card-title">
        <span>Destination Directory</span>
      </div>

      <div className="destination-box">
        <div style={{ display: 'flex', alignItems: 'center', gap: 8, overflow: 'hidden' }}>
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" style={{ color: 'var(--accent-color)', flexShrink: 0 }}>
            <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
          </svg>
          <span className="destination-path" title={destination?.displayName ?? 'No destination directory selected'}>
            {destination ? destination.displayName : 'No destination directory selected'}
          </span>
        </div>

        <button
          type="button"
          onClick={onSelectDestination}
          className="btn btn-secondary"
          disabled={disabled}
          aria-label={destination ? 'Change destination directory' : 'Select destination directory'}
          style={{ padding: '4px 10px', fontSize: '11px', flexShrink: 0 }}
        >
          {destination ? 'Change...' : 'Select...'}
        </button>
      </div>
    </div>
  );
};
