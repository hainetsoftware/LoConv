//! Job lifecycle helpers.
//!
//! Bounded session-only snapshot storage. The host serializes access with its
//! session mutex. No worker, file I/O, successful conversions or persistence.

use loconv_contracts::JobState;

/// Maximum number of jobs in one session, bounding revisions.
pub const MAX_JOBS_PER_SESSION: usize = 100;

/// First published revision for a new snapshot.
pub const INITIAL_REVISION: u32 = 1;

/// Terminal states are immutable and cannot be cancelled.
pub fn is_terminal(state: &JobState) -> bool {
    matches!(
        state,
        JobState::Succeeded | JobState::Failed | JobState::Cancelled
    )
}

/// Cancellation may start only before the job is already cancelling/terminal.
pub fn can_request_cancel(state: &JobState) -> bool {
    matches!(state, JobState::Queued | JobState::Running)
}

/// Accept an incoming event only when it advances the snapshot revision.
pub fn accepts_revision(current: u32, incoming: u32) -> bool {
    incoming > current
}

/// Storage only: callers must authorize selections and validate backend capability
/// before registering a request. This registry never runs or completes work.
#[derive(Default)]
pub struct JobRegistry {
    jobs: Vec<loconv_contracts::JobSnapshot>,
}

impl JobRegistry {
    pub fn list(&self) -> Vec<loconv_contracts::JobSnapshot> {
        self.jobs.clone()
    }

    pub fn register_queued(
        &mut self,
        request: loconv_contracts::ConversionRequest,
    ) -> crate::errors::EngineResult<loconv_contracts::JobSnapshot> {
        use loconv_contracts::*;
        if self.jobs.len() >= MAX_JOBS_PER_SESSION {
            return Err(AppError {
                code: ErrorCode::ResourceLimit,
                message: "The session job limit was reached.".into(),
                retryable: false,
            });
        }
        let spec = crate::formats::find(&request.format_id)
            .ok_or_else(crate::errors::unsupported_format)?;
        crate::paths::validate_output_name(&request.output_name, spec.extension)?;
        if request.media_id.is_empty() || request.destination_id.is_empty() {
            return Err(crate::errors::invalid_request());
        }
        let job = JobSnapshot {
            job_id: uuid::Uuid::new_v4().to_string(),
            media_id: request.media_id,
            format_id: request.format_id,
            output_name: request.output_name,
            state: JobState::Queued,
            revision: INITIAL_REVISION,
            progress: None,
            error: None,
        };
        self.jobs.push(job.clone());
        Ok(job)
    }

    pub fn cancel(
        &mut self,
        id: &str,
    ) -> crate::errors::EngineResult<loconv_contracts::JobSnapshot> {
        use loconv_contracts::*;
        let job = self
            .jobs
            .iter_mut()
            .find(|job| job.job_id == id)
            .ok_or_else(|| AppError {
                code: ErrorCode::JobNotFound,
                message: "The job was not found in this session.".into(),
                retryable: false,
            })?;
        if !is_terminal(&job.state) {
            // This registry only creates queued jobs. Active worker cancellation
            // must be implemented with a supervisor before workers are enabled.
            if job.state != JobState::Queued {
                return Err(crate::errors::internal());
            }
            job.revision = job
                .revision
                .checked_add(1)
                .ok_or_else(crate::errors::internal)?;
            job.state = JobState::Cancelled;
            job.progress = None;
            job.error = None;
        }
        Ok(job.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_states_are_exactly_the_three_final_states() {
        for state in [JobState::Succeeded, JobState::Failed, JobState::Cancelled] {
            assert!(is_terminal(&state));
            assert!(!can_request_cancel(&state));
        }

        for state in [JobState::Queued, JobState::Running, JobState::Cancelling] {
            assert!(!is_terminal(&state));
        }
    }

    #[test]
    fn cancel_is_requestable_only_from_pre_work_states() {
        assert!(can_request_cancel(&JobState::Queued));
        assert!(can_request_cancel(&JobState::Running));
        assert!(!can_request_cancel(&JobState::Cancelling));
        assert!(!can_request_cancel(&JobState::Cancelled));
    }

    #[test]
    fn revisions_must_strictly_increase() {
        assert_eq!(INITIAL_REVISION, 1);
        assert!(!accepts_revision(1, 1));
        assert!(!accepts_revision(2, 1));
        assert!(accepts_revision(1, 2));
        assert_eq!(MAX_JOBS_PER_SESSION, 100);
    }
}

#[cfg(test)]
mod registry_tests {
    use super::*;
    use loconv_contracts::*;
    fn request() -> ConversionRequest {
        ConversionRequest {
            media_id: "synthetic-input".into(),
            destination_id: "synthetic-destination".into(),
            output_name: "synthetic.png".into(),
            format_id: FormatId::Png,
        }
    }
    #[test]
    fn registry_is_bounded_and_session_local() {
        let mut registry = JobRegistry::default();
        assert!(registry.list().is_empty());
        for _ in 0..MAX_JOBS_PER_SESSION {
            let snapshot = registry.register_queued(request()).unwrap();
            assert_eq!(snapshot.state, JobState::Queued);
            assert_eq!(snapshot.revision, 1);
            assert!(snapshot.progress.is_none());
            assert!(snapshot.error.is_none());
            assert!(uuid::Uuid::parse_str(&snapshot.job_id).is_ok());
        }
        assert_eq!(
            registry.register_queued(request()).unwrap_err().code,
            ErrorCode::ResourceLimit
        );
        assert_eq!(registry.list().len(), MAX_JOBS_PER_SESSION);
        assert!(JobRegistry::default().list().is_empty());
    }
    #[test]
    fn cancellation_is_serialized_and_terminal_snapshots_are_immutable() {
        let mut registry = JobRegistry::default();
        let job = registry.register_queued(request()).unwrap();
        let registry = std::sync::Arc::new(std::sync::Mutex::new(registry));
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let registry = registry.clone();
                let id = job.job_id.clone();
                std::thread::spawn(move || registry.lock().unwrap().cancel(&id).unwrap())
            })
            .collect();
        for handle in handles {
            let cancelled = handle.join().unwrap();
            assert_eq!(cancelled.state, JobState::Cancelled);
            assert_eq!(cancelled.revision, 2);
        }
        for state in [JobState::Succeeded, JobState::Failed, JobState::Cancelled] {
            let mut registry = registry.lock().unwrap();
            registry.jobs[0].state = state;
            let before = registry.list()[0].clone();
            assert_eq!(registry.cancel(&job.job_id).unwrap(), before);
        }
    }
}
