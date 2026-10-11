//! Normal termination: save the latest state, then fade out, then quit.
//!
//! The coordinator is a pure state machine. The app feeds it facts (a save receipt,
//! a finished fade) and performs the side effects it names. Every fact carries the
//! revision or attempt it belongs to, so a stale result can never advance a newer
//! attempt.

use std::time::Duration;

use crate::{
    audio::timing::MachNanos,
    controls::Revision,
    persistence::{SaveError, SaveStage},
};

/// Identifies one termination attempt. Results of an older attempt never apply to a
/// newer one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AttemptId(u64);

impl AttemptId {
    pub(crate) fn get(self) -> u64 {
        self.0
    }

    /// `0` is reserved for "no attempt" in the lock-free audio mailbox.
    pub(crate) fn from_raw(value: u64) -> Option<Self> {
        (value != 0).then_some(Self(value))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// Edits are stopped; wait until the audio side acknowledged the latest control.
    AwaitLatestState,
    /// This exact save revision must complete before anything audible changes.
    Saving(Revision),
    /// Shutdown gain runs until its scheduled final playback time.
    Fading,
    /// Quit has been allowed. Nothing can cancel it any more.
    Committing,
}

#[derive(Debug)]
struct Attempt {
    id: AttemptId,
    phase: Phase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Requested {
    Started(AttemptId),
    /// A Quit menu, ⌘Q and an OS request all merge into the active attempt.
    Joined(AttemptId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SaveDecision {
    /// Not the active attempt's exact save: an older revision or a stale attempt.
    Ignore,
    /// Saved. Only now may the output fade begin.
    Fade(AttemptId),
    /// Not saved. The attempt is gone; editing, sound and the app continue.
    Cancel(AttemptId),
}

#[derive(Debug, Default)]
pub(crate) struct TerminationCoordinator {
    last: u64,
    active: Option<Attempt>,
}

impl TerminationCoordinator {
    pub(crate) fn request(&mut self) -> Requested {
        if let Some(active) = &self.active {
            return Requested::Joined(active.id);
        }
        self.last += 1;
        let id = AttemptId(self.last);
        self.active = Some(Attempt {
            id,
            phase: Phase::AwaitLatestState,
        });
        Requested::Started(id)
    }

    /// Settings edits and autosaves stop for as long as an attempt exists.
    pub(crate) fn is_active(&self) -> bool {
        self.active.is_some()
    }

    pub(crate) fn is_committing(&self) -> bool {
        matches!(
            self.active,
            Some(Attempt {
                phase: Phase::Committing,
                ..
            })
        )
    }

    /// The attempt is still the active one (not cancelled, not replaced).
    pub(crate) fn is_current(&self, id: AttemptId) -> bool {
        self.phase_of(id).is_some()
    }
    pub(crate) fn is_awaiting_latest_state(&self, id: AttemptId) -> bool {
        self.phase_of(id) == Some(Phase::AwaitLatestState)
    }

    pub(crate) fn is_fading(&self, id: AttemptId) -> bool {
        self.phase_of(id) == Some(Phase::Fading)
    }

    fn phase_of(&self, id: AttemptId) -> Option<Phase> {
        self.active
            .as_ref()
            .filter(|attempt| attempt.id == id)
            .map(|attempt| attempt.phase)
    }

    /// The latest state is settled and `revision` was submitted as the final snapshot.
    pub(crate) fn begin_saving(&mut self, id: AttemptId, revision: Revision) -> bool {
        match &mut self.active {
            Some(attempt) if attempt.id == id && attempt.phase == Phase::AwaitLatestState => {
                attempt.phase = Phase::Saving(revision);
                true
            }
            _ => false,
        }
    }

    /// A save finished. Only the exact revision of the active attempt counts: the
    /// success of an earlier snapshot says nothing about the final one.
    pub(crate) fn save_completed(&mut self, revision: Revision, saved: bool) -> SaveDecision {
        let Some(attempt) = &mut self.active else {
            return SaveDecision::Ignore;
        };
        if attempt.phase != Phase::Saving(revision) {
            return SaveDecision::Ignore;
        }
        let id = attempt.id;
        if saved {
            attempt.phase = Phase::Fading;
            SaveDecision::Fade(id)
        } else {
            self.active = None;
            SaveDecision::Cancel(id)
        }
    }

    /// The final fade sample has played (or there was nothing to fade). Returns true
    /// exactly once per attempt.
    pub(crate) fn begin_commit(&mut self, id: AttemptId) -> bool {
        match &mut self.active {
            Some(attempt) if attempt.id == id && attempt.phase == Phase::Fading => {
                attempt.phase = Phase::Committing;
                true
            }
            _ => false,
        }
    }

    /// Gives up before the output was touched. A committing attempt cannot be cancelled.
    pub(crate) fn cancel(&mut self, id: AttemptId) -> bool {
        match &self.active {
            Some(attempt)
                if attempt.id == id
                    && matches!(attempt.phase, Phase::AwaitLatestState | Phase::Saving(_)) =>
            {
                self.active = None;
                true
            }
            _ => false,
        }
    }
}

/// Whether the final save lets the quit proceed. Some states can never be written: a
/// newer-version or unbackupable original is deliberately protected, and a missing or
/// dead save worker cannot write at all. Refusing to quit would trap the user (and block
/// logout) without protecting anything. A genuine write failure is different: the user
/// can fix it and quit again, so the app keeps running.
pub(crate) fn final_save_allows_quit(result: &Result<(), SaveError>) -> bool {
    match result {
        Ok(()) => true,
        Err(error) => matches!(
            error.stage,
            SaveStage::ReadOnly
                | SaveStage::Spawn
                | SaveStage::StartupChannel
                | SaveStage::WorkerStopped
                | SaveStage::WorkerPanicked
        ),
    }
}

// Implementation constants, not product timing controls.
pub(crate) const POLL_INTERVAL: Duration = Duration::from_millis(10);
const ACK_TIMEOUT: Duration = Duration::from_millis(500);
const DEADLINE_TIMEOUT: Duration = Duration::from_millis(500);

/// Whether the audio side has acknowledged the latest control, so the saved resume
/// target reflects the final edit. Nothing to wait for without a source, and a
/// bounded wait otherwise: a stream that never delivers a callback must not hold the
/// quit forever.
pub(crate) fn latest_state_settled(
    source_present: bool,
    acknowledged: bool,
    waited: Duration,
) -> bool {
    !source_present || acknowledged || waited >= ACK_TIMEOUT
}

/// Whether the output fade is over. A stopped output (sleep, device loss, failure) is
/// already silent and is never waited on. Otherwise the stream is kept until the final
/// sample's scheduled playback time; if the callback never reports it, wait a bounded
/// time instead of forever.
pub(crate) fn fade_finished(
    output_running: bool,
    deadline: Option<MachNanos>,
    now: MachNanos,
    waited: Duration,
) -> bool {
    if !output_running {
        return true;
    }
    match deadline {
        Some(deadline) => now.get() >= deadline.get(),
        None => waited >= DEADLINE_TIMEOUT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn started(coordinator: &mut TerminationCoordinator) -> AttemptId {
        match coordinator.request() {
            Requested::Started(id) => id,
            Requested::Joined(_) => panic!("expected a new attempt"),
        }
    }

    fn rev(value: u64) -> Revision {
        Revision::from_raw(value)
    }

    #[test]
    fn quit_menu_and_os_requests_merge_into_one_attempt() {
        let mut coordinator = TerminationCoordinator::default();
        let first = started(&mut coordinator);
        assert_eq!(coordinator.request(), Requested::Joined(first));
        assert_eq!(coordinator.request(), Requested::Joined(first));
        assert!(coordinator.is_active());
    }

    #[test]
    fn an_older_snapshot_succeeding_never_starts_the_fade() {
        let mut coordinator = TerminationCoordinator::default();
        let id = started(&mut coordinator);
        assert!(coordinator.begin_saving(id, rev(7)));
        assert_eq!(
            coordinator.save_completed(rev(6), true),
            SaveDecision::Ignore
        );
        assert!(!coordinator.is_fading(id));
        assert_eq!(
            coordinator.save_completed(rev(7), true),
            SaveDecision::Fade(id)
        );
        assert!(coordinator.is_fading(id));
    }

    #[test]
    fn a_failed_final_save_cancels_the_attempt_and_frees_edits() {
        let mut coordinator = TerminationCoordinator::default();
        let id = started(&mut coordinator);
        assert!(coordinator.begin_saving(id, rev(3)));
        assert_eq!(
            coordinator.save_completed(rev(3), false),
            SaveDecision::Cancel(id)
        );
        assert!(!coordinator.is_active(), "edits and autosaves resume");
        assert!(
            !coordinator.begin_commit(id),
            "a cancelled attempt never quits"
        );
        // A later request is a new attempt.
        assert_ne!(started(&mut coordinator), id);
    }

    #[test]
    fn a_stale_attempt_cannot_touch_a_newer_one() {
        let mut coordinator = TerminationCoordinator::default();
        let old = started(&mut coordinator);
        assert!(coordinator.begin_saving(old, rev(1)));
        assert_eq!(
            coordinator.save_completed(rev(1), false),
            SaveDecision::Cancel(old)
        );
        let new = started(&mut coordinator);
        assert!(coordinator.begin_saving(new, rev(2)));
        // Old attempt results arrive late.
        assert_eq!(
            coordinator.save_completed(rev(1), true),
            SaveDecision::Ignore
        );
        assert!(!coordinator.begin_saving(old, rev(9)));
        assert!(!coordinator.begin_commit(old));
        assert!(!coordinator.cancel(old));
        assert!(coordinator.is_active());
        assert!(!coordinator.is_fading(new));
        assert_eq!(
            coordinator.save_completed(rev(2), true),
            SaveDecision::Fade(new)
        );
    }

    #[test]
    fn quit_commits_once_after_the_fade_and_cannot_be_cancelled_afterwards() {
        let mut coordinator = TerminationCoordinator::default();
        let id = started(&mut coordinator);
        assert!(!coordinator.begin_commit(id), "no commit before the save");
        assert!(coordinator.begin_saving(id, rev(4)));
        assert!(!coordinator.begin_commit(id), "no commit while saving");
        assert_eq!(
            coordinator.save_completed(rev(4), true),
            SaveDecision::Fade(id)
        );
        assert!(
            !coordinator.cancel(id),
            "a fading attempt is past cancelling"
        );
        assert!(coordinator.begin_commit(id));
        assert!(!coordinator.begin_commit(id), "exactly one quit");
        assert!(coordinator.is_committing());
        assert_eq!(coordinator.request(), Requested::Joined(id));
    }

    #[test]
    fn the_latest_state_wait_is_bounded_and_skips_without_a_source() {
        let zero = Duration::ZERO;
        assert!(
            latest_state_settled(false, false, zero),
            "no source: nothing to ack"
        );
        assert!(latest_state_settled(true, true, zero));
        assert!(!latest_state_settled(
            true,
            false,
            Duration::from_millis(499)
        ));
        assert!(latest_state_settled(
            true,
            false,
            Duration::from_millis(500)
        ));
    }

    #[test]
    fn the_fade_holds_the_stream_until_the_final_sample_plays() {
        let deadline = Some(MachNanos::new(1_000));
        let zero = Duration::ZERO;
        assert!(!fade_finished(true, deadline, MachNanos::new(999), zero));
        assert!(fade_finished(true, deadline, MachNanos::new(1_000), zero));
        assert!(fade_finished(true, deadline, MachNanos::new(5_000), zero));
    }

    #[test]
    fn unwritable_states_do_not_trap_the_quit_but_a_fixable_write_failure_does() {
        let error = |stage| {
            Err(SaveError {
                stage,
                message: String::from("fixture"),
            })
        };
        assert!(final_save_allows_quit(&Ok(())));
        // Never writable: a protected original, or no working save worker at all.
        for stage in [
            SaveStage::ReadOnly,
            SaveStage::Spawn,
            SaveStage::StartupChannel,
            SaveStage::WorkerStopped,
            SaveStage::WorkerPanicked,
        ] {
            assert!(final_save_allows_quit(&error(stage)), "{stage:?}");
        }
        // A write that can be fixed and retried keeps the app running.
        for stage in [
            SaveStage::CreateTemp,
            SaveStage::Write,
            SaveStage::SyncFile,
            SaveStage::Replace,
            SaveStage::SyncDirectory,
        ] {
            assert!(!final_save_allows_quit(&error(stage)), "{stage:?}");
        }
    }

    #[test]
    fn stopped_output_never_waits_and_a_silent_callback_is_bounded() {
        let zero = Duration::ZERO;
        // Sleep, device loss or failure: already silent, even without a deadline.
        assert!(fade_finished(false, None, MachNanos::new(0), zero));
        assert!(fade_finished(
            false,
            Some(MachNanos::new(u64::MAX)),
            MachNanos::new(0),
            zero
        ));
        // Running but the callback never reports a deadline.
        assert!(!fade_finished(
            true,
            None,
            MachNanos::new(0),
            Duration::from_millis(499)
        ));
        assert!(fade_finished(
            true,
            None,
            MachNanos::new(0),
            Duration::from_millis(500)
        ));
    }
}
