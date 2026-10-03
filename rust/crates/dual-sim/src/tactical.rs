use super::{Side, TACTICAL_OPENING_WINDOW_FRAMES};

/// Player-readable facts emitted by a shortbow-to-longbow tactical sequence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TacticalEventKind {
    Pressure,
    Opening,
    Disrupt,
    Finish,
    Intercept,
}

/// One immutable tactical fact with the side and active simulation frame that produced it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TacticalEvent {
    pub attacker: Option<Side>,
    pub kind: TacticalEventKind,
    pub frame: u64,
}

impl TacticalEvent {
    pub const fn for_side(attacker: Side, kind: TacticalEventKind, frame: u64) -> Self {
        Self {
            attacker: Some(attacker),
            kind,
            frame,
        }
    }

    pub const fn intercept(frame: u64) -> Self {
        Self {
            attacker: None,
            kind: TacticalEventKind::Intercept,
            frame,
        }
    }
}

/// Tracks independent pressure windows without retaining players, arrows, or renderer state.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct TacticalEventRecorder {
    pending: [Option<PendingSequence>; 2],
}

impl TacticalEventRecorder {
    /// Replaces the attacking player's pending chain after every confirmed shortbow hit.
    pub(crate) fn record_pressure(&mut self, attacker: Side, frame: u64) -> TacticalEvent {
        self.pending[attacker.index()] = Some(PendingSequence {
            pressure_frame: frame,
            opening_recorded: false,
        });
        TacticalEvent::for_side(attacker, TacticalEventKind::Pressure, frame)
    }

    /// Records the first longbow charge that starts inside the same player's pressure window.
    pub(crate) fn record_longbow_charge_started(
        &mut self,
        attacker: Side,
        frame: u64,
    ) -> Option<TacticalEvent> {
        let sequence = self.pending[attacker.index()]?;
        if sequence.opening_recorded || !sequence.is_in_window(frame) {
            return None;
        }
        self.pending[attacker.index()] = Some(PendingSequence {
            opening_recorded: true,
            ..sequence
        });
        Some(TacticalEvent::for_side(
            attacker,
            TacticalEventKind::Opening,
            frame,
        ))
    }

    /// Cancels only the current opening so a later charge can establish a new one.
    pub(crate) fn record_longbow_charge_cancelled(&mut self, attacker: Side, frame: u64) {
        let Some(sequence) = self.pending[attacker.index()] else {
            return;
        };
        if !sequence.is_in_window(frame) {
            self.pending[attacker.index()] = None;
        } else if sequence.opening_recorded {
            self.pending[attacker.index()] = Some(PendingSequence {
                opening_recorded: false,
                ..sequence
            });
        }
    }

    /// Emits a tactical finish only for a lethal hit after a valid opening.
    pub(crate) fn record_longbow_finish(
        &mut self,
        attacker: Side,
        frame: u64,
    ) -> Option<TacticalEvent> {
        let sequence = self.pending[attacker.index()]?;
        if !sequence.opening_recorded || !sequence.is_in_window(frame) {
            return None;
        }
        self.pending[attacker.index()] = None;
        Some(TacticalEvent::for_side(
            attacker,
            TacticalEventKind::Finish,
            frame,
        ))
    }

    pub(crate) fn has_active_opening(&self, attacker: Side, frame: u64) -> bool {
        self.pending[attacker.index()]
            .is_some_and(|sequence| sequence.opening_recorded && sequence.is_in_window(frame))
    }

    pub(crate) fn reset(&mut self) {
        self.pending = [None; 2];
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PendingSequence {
    pressure_frame: u64,
    opening_recorded: bool,
}

impl PendingSequence {
    fn is_in_window(self, frame: u64) -> bool {
        frame >= self.pressure_frame
            && frame - self.pressure_frame <= TACTICAL_OPENING_WINDOW_FRAMES
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_pressure_opening_and_finish_within_the_ninety_frame_window() {
        let mut recorder = TacticalEventRecorder::default();

        assert_eq!(
            recorder.record_pressure(Side::One, 10),
            TacticalEvent::for_side(Side::One, TacticalEventKind::Pressure, 10)
        );
        assert_eq!(
            recorder.record_longbow_charge_started(Side::One, 30),
            Some(TacticalEvent::for_side(
                Side::One,
                TacticalEventKind::Opening,
                30
            ))
        );
        assert_eq!(
            recorder.record_longbow_finish(Side::One, 100),
            Some(TacticalEvent::for_side(
                Side::One,
                TacticalEventKind::Finish,
                100
            ))
        );
    }

    #[test]
    fn expires_at_the_same_inclusive_boundary_as_the_java_rules() {
        let mut recorder = TacticalEventRecorder::default();
        recorder.record_pressure(Side::One, 10);

        assert!(
            recorder
                .record_longbow_charge_started(Side::One, 100)
                .is_some()
        );
        assert!(
            recorder
                .record_longbow_charge_started(Side::Two, 101)
                .is_none()
        );
        assert!(
            recorder
                .record_longbow_charge_started(Side::One, 101)
                .is_none()
        );
    }

    #[test]
    fn keeps_pressure_sequences_independent_by_player() {
        let mut recorder = TacticalEventRecorder::default();
        recorder.record_pressure(Side::One, 10);

        assert!(
            recorder
                .record_longbow_charge_started(Side::Two, 20)
                .is_none()
        );
        assert!(
            recorder
                .record_longbow_charge_started(Side::One, 20)
                .is_some()
        );
        assert!(recorder.record_longbow_finish(Side::Two, 30).is_none());
        assert!(recorder.record_longbow_finish(Side::One, 30).is_some());
    }

    #[test]
    fn prevents_duplicate_openings_and_finishes_without_a_valid_chain() {
        let mut recorder = TacticalEventRecorder::default();
        assert!(
            recorder
                .record_longbow_charge_started(Side::One, 10)
                .is_none()
        );
        assert!(recorder.record_longbow_finish(Side::One, 10).is_none());

        recorder.record_pressure(Side::One, 10);
        assert!(
            recorder
                .record_longbow_charge_started(Side::One, 20)
                .is_some()
        );
        assert!(
            recorder
                .record_longbow_charge_started(Side::One, 21)
                .is_none()
        );
        assert!(recorder.record_longbow_finish(Side::One, 30).is_some());
        assert!(recorder.record_longbow_finish(Side::One, 31).is_none());
    }

    #[test]
    fn a_cancelled_charge_requires_a_fresh_opening_before_finishing() {
        let mut recorder = TacticalEventRecorder::default();
        recorder.record_pressure(Side::One, 10);
        assert!(
            recorder
                .record_longbow_charge_started(Side::One, 20)
                .is_some()
        );

        recorder.record_longbow_charge_cancelled(Side::One, 30);

        assert!(recorder.record_longbow_finish(Side::One, 31).is_none());
        assert!(
            recorder
                .record_longbow_charge_started(Side::One, 32)
                .is_some()
        );
        assert!(recorder.record_longbow_finish(Side::One, 40).is_some());
    }

    #[test]
    fn resets_pending_sequences_between_rounds() {
        let mut recorder = TacticalEventRecorder::default();
        recorder.record_pressure(Side::One, 10);
        recorder.reset();

        assert!(
            recorder
                .record_longbow_charge_started(Side::One, 20)
                .is_none()
        );
    }
}
