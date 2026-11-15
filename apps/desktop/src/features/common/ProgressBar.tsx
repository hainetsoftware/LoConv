import React from 'react';
import { JobState } from '../../api/contracts';

interface ProgressBarProps {
  progress: number | null;
  state: JobState;
  showText?: boolean;
}

export const ProgressBar: React.FC<ProgressBarProps> = ({ progress, state, showText = true }) => {
  const isIndeterminate = progress === null && state === 'running';
  const percentage = progress !== null ? Math.min(100, Math.max(0, Math.round(progress * 100))) : 0;

  let fillClass = 'progress-fill';
  if (isIndeterminate) {
    fillClass += ' indeterminate';
  } else if (state === 'succeeded') {
    fillClass += ' succeeded';
  } else if (state === 'failed') {
    fillClass += ' failed';
  }

  const fillStyle: React.CSSProperties = isIndeterminate
    ? {}
    : {
        width: state === 'succeeded' ? '100%' : `${percentage}%`,
      };

  return (
    <div className="progress-container" role="progressbar" aria-valuenow={progress !== null ? percentage : undefined} aria-valuemin={0} aria-valuemax={100} aria-label="Conversion progress">
      <div className="progress-track">
        <div className={fillClass} style={fillStyle} />
      </div>
      {showText && (
        <div className="progress-text">
          <span>
            {state === 'queued' && 'Queued'}
            {state === 'running' && (isIndeterminate ? 'Processing...' : `Converting: ${percentage}%`)}
            {state === 'cancelling' && 'Cancelling...'}
            {state === 'succeeded' && 'Completed'}
            {state === 'failed' && 'Failed'}
            {state === 'cancelled' && 'Cancelled'}
          </span>
          {!isIndeterminate && progress !== null && <span>{percentage}%</span>}
        </div>
      )}
    </div>
  );
};
