import React from 'react';

interface HeaderProps {
  isMock: boolean;
  theme: 'light' | 'dark';
  onToggleTheme: () => void;
}

export const Header: React.FC<HeaderProps> = ({ isMock, theme, onToggleTheme }) => {
  return (
    <header className="app-header">
      <div className="app-title-group">
        <svg className="app-logo" viewBox="0 0 24 24" fill="currentColor">
          <path d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm-1 14.5v-9l6 4.5-6 4.5z" />
        </svg>
        <span className="app-title">LoConv</span>
        <span className="app-subtitle" title="Processes media entirely on your device without network calls">
          Offline
        </span>
        {isMock && (
          <span className="badge" style={{ backgroundColor: 'rgba(255, 159, 10, 0.15)', color: 'var(--warning-color)' }} title="Running with synthetic in-memory DesktopApi mock">
            Mock Mode
          </span>
        )}
      </div>

      <div className="header-actions">
        <button
          type="button"
          onClick={onToggleTheme}
          className="btn btn-secondary"
          style={{ padding: '4px 10px', fontSize: '11px' }}
          title={`Switch to ${theme === 'light' ? 'Dark' : 'Light'} appearance`}
        >
          {theme === 'light' ? '🌙 Dark' : '☀️ Light'}
        </button>
      </div>
    </header>
  );
};
