import React from 'react';

interface IncompatibleBannerProps {
  version: number | null;
  expectedVersion: number;
}

export const IncompatibleBanner: React.FC<IncompatibleBannerProps> = ({
  version,
  expectedVersion,
}) => {
  return (
    <div
      className="alert-banner alert-danger"
      role="alert"
      style={{
        margin: 20,
        padding: 24,
        flexDirection: 'column',
        alignItems: 'flex-start',
        gap: 8,
      }}
    >
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
          <circle cx="12" cy="12" r="10" />
          <line x1="12" y1="8" x2="12" y2="12" />
          <line x1="12" y1="16" x2="12.01" y2="16" />
        </svg>
        <strong style={{ fontSize: 16 }}>Incompatible Backend API Version</strong>
      </div>
      <p style={{ fontSize: 13, color: 'var(--text-primary)', marginTop: 4 }}>
        The backend IPC interface version ({version === null ? 'unknown' : version}) does not match the required version ({expectedVersion}).
        Conversion operations are disabled to prevent corruption. Please update LoConv.
      </p>
    </div>
  );
};
