//! Job lifecycle helpers.
//!
//! No queue, worker, cancellation, or persistence is implemented in this
//! increment. These pure predicates keep lifecycle rules in one place for the
//! later manager and can be tested without I/O.

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
