import React from 'react';
import { JobSnapshot } from '../../api/contracts';
import { JobItem } from './JobItem';

interface ConversionQueueProps {
  jobs: JobSnapshot[];
  onCancelJob: (jobId: string) => void;
  cancellingJobIds: Set<string>;
}

export const ConversionQueue: React.FC<ConversionQueueProps> = ({
  jobs,
  onCancelJob,
  cancellingJobIds,
}) => {
  const activeCount = jobs.filter((j) => j.state === 'running' || j.state === 'queued' || j.state === 'cancelling').length;
  const completedCount = jobs.filter((j) => j.state === 'succeeded').length;
  const failedCount = jobs.filter((j) => j.state === 'failed').length;

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
      <div className="queue-header">
        <div className="queue-title">
          <span>Conversion Queue</span>
          <span className="badge" style={{ backgroundColor: 'var(--bg-inset)', color: 'var(--text-secondary)' }}>
            {jobs.length} total
          </span>
        </div>

        {jobs.length > 0 && (
          <div style={{ display: 'flex', gap: 8, fontSize: '11px', color: 'var(--text-secondary)' }}>
            {activeCount > 0 && <span style={{ color: 'var(--accent-color)' }}>● {activeCount} active</span>}
            {completedCount > 0 && <span style={{ color: 'var(--success-color)' }}>✓ {completedCount} done</span>}
            {failedCount > 0 && <span style={{ color: 'var(--danger-color)' }}>✕ {failedCount} failed</span>}
          </div>
        )}
      </div>

      {jobs.length === 0 ? (
        <div className="panel-card">
          <div className="empty-state">
            <svg className="empty-state-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
              <path d="M4 4h16c1.1 0 2 .9 2 2v12c0 1.1-.9 2-2 2H4c-1.1 0-2-.9-2-2V6c0-1.1.9-2 2-2z" />
              <polyline points="22,6 12,13 2,6" />
            </svg>
            <div className="empty-state-title">No conversions in queue</div>
            <div className="empty-state-desc">
              Select an input file, choose an output format, and click "Start Conversion" to begin processing offline.
            </div>
          </div>
        </div>
      ) : (
        <div className="queue-list" role="feed" aria-label="Conversion jobs">
          {jobs.map((job) => (
            <JobItem
              key={job.jobId}
              job={job}
              onCancelJob={onCancelJob}
              isCancelling={cancellingJobIds.has(job.jobId)}
            />
          ))}
        </div>
      )}
    </div>
  );
};
