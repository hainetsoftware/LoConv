import React from 'react';
import { JobSnapshot } from '../../api/contracts';
import { Badge } from '../common/Badge';
import { ProgressBar } from '../common/ProgressBar';

interface JobItemProps {
  job: JobSnapshot;
  onCancelJob: (jobId: string) => void;
  isCancelling?: boolean;
}

export const JobItem: React.FC<JobItemProps> = ({
  job,
  onCancelJob,
  isCancelling = false,
}) => {
  const canCancel = job.state === 'queued' || job.state === 'running';

  return (
    <div className="job-card" data-testid={`job-${job.jobId}`}>
      <div className="job-card-header">
        <div style={{ display: 'flex', alignItems: 'center', gap: 8, overflow: 'hidden' }}>
          <Badge type="state" value={job.state} />
          <span className="job-output-name" title={job.outputName}>
            {job.outputName}
          </span>
        </div>

        <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
          <span style={{ fontSize: '11px', color: 'var(--text-tertiary)' }} title="Monotonic state revision">
            rev {job.revision}
          </span>
          {canCancel && (
            <button
              type="button"
              className="btn btn-danger"
              style={{ padding: '3px 10px', fontSize: '11px' }}
              onClick={() => onCancelJob(job.jobId)}
              disabled={isCancelling || job.state === 'cancelling'}
              aria-label={`Cancel conversion of ${job.outputName}`}
            >
              {job.state === 'cancelling' || isCancelling ? 'Cancelling...' : 'Cancel'}
            </button>
          )}
        </div>
      </div>

      <ProgressBar progress={job.progress} state={job.state} />

      {job.state === 'failed' && job.error && (
        <div className="job-error-box" role="alert">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <circle cx="12" cy="12" r="10" />
            <line x1="12" y1="8" x2="12" y2="12" />
            <line x1="12" y1="16" x2="12.01" y2="16" />
          </svg>
          <span>
            [{job.error.code}]: {job.error.message}
          </span>
        </div>
      )}
    </div>
  );
};
