import React from 'react';
import { AppError } from '../../api/contracts';

interface AlertProps {
  error: AppError | string;
  onDismiss?: () => void;
}

export const Alert: React.FC<AlertProps> = ({ error, onDismiss }) => {
  const message = typeof error === 'string' ? error : error.message;
  const code = typeof error === 'string' ? null : error.code;

  return (
    <div className="alert-banner alert-danger" role="alert">
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
        <circle cx="12" cy="12" r="10" />
        <line x1="12" y1="8" x2="12" y2="12" />
        <line x1="12" y1="16" x2="12.01" y2="16" />
      </svg>
      <div style={{ flex: 1 }}>
        {code && <span style={{ opacity: 0.8, marginRight: 6 }}>[{code}]</span>}
        <span>{message}</span>
      </div>
      {onDismiss && (
        <button type="button" onClick={onDismiss} className="btn-icon" aria-label="Dismiss error" style={{ color: 'inherit' }}>
          ✕
        </button>
      )}
    </div>
  );
};
