//! How far a set advances when one record hands over to the next.
//!
//! # Why this is its own module
//!
//! It was not, and that was a defect. The planner laid tracks end to end — each
//! one starting where the last one ended — while the renderer started each track
//! at the *outgoing track's exit point*, which is where the analysis says the
//! record wants to be left. Two models of the same clock, in two files, neither
//! aware of the other.
//!
//! For material with no exit points the two nearly agree, which is why it went
//! unnoticed: the tests used tracks with no analysis attached. For material with
//! exit points — the normal case, and the case the whole product is built on —
//! they disagree badly. Eight five-minute records whose exit points sit a quarter
//! of the way in produce a *forty*-minute plan and a *thirteen*-minute mix, and
//! [`crate::MixPlan::duration_error`] reports the set as a perfect match for what
//! the user asked for.
//!
//! That last part is what makes it serious rather than merely wrong. A planner
//! that comes up short can say so, and a user can accept it or ask for more.
//! A planner that comes up short and reports success has no way to be caught,
//! and `duration_error` is also the number the search *sorts by* — so the wrong
//! clock was choosing between plans as well as describing them.
//!
//! So there is now one function that answers "how far does the set move on",
//! and both the planner and the renderer call it. A rule that lives in one place
//! can be wrong; a rule that lives in two places will eventually be wrong in
//! only one of them, which is worse, because then it looks right from wherever
//! you happen to be reading.

use prv_time::{Frames, SampleRate};

use crate::candidate::Candidate;
use crate::num::{micros_to_seconds, round_to_i64};
use crate::render::choose_technique;
use crate::transition::TransitionScore;

/// How long the overlap into a track should be.
///
/// The incoming track's tempo sets the length. The planner has already
/// constrained the two records to be within a few per cent of each other, so
/// which of them sets the pulse changes the answer by less than a beat — and
/// using the incoming one keeps the overlap a whole number of *its* bars, which
/// is the record whose arrangement the listener is arriving at.
pub(crate) fn overlap(
    score: &TransitionScore,
    incoming: &Candidate,
    sample_rate: SampleRate,
) -> Frames {
    let choice = choose_technique(score.components());
    let beats = choice.technique().overlap_beats(score.total());
    let seconds_per_beat = micros_to_seconds(incoming.tempo().micros_per_beat());
    let frames = beats * seconds_per_beat * f64::from(sample_rate.hz());

    // Never longer than the record it belongs to. An overlap longer than the
    // incoming track would place it before the set began.
    Frames::new(round_to_i64(frames).clamp(0, incoming.duration().get()))
}

/// How far the set advances when `outgoing` hands over to the next record.
///
/// # Transitions land where the music offers them
///
/// The naive placement — overlap the last few bars of whatever is playing —
/// mixes into the outgoing track's *ending*, which on a produced record is often
/// a fade, a drum outro, or nothing at all. The analysis has already found where
/// the record actually wants to be left: a quiet outro, or a breakdown.
///
/// So the set advances to the outgoing track's best exit point when it has one,
/// and to "one overlap before the end" when it does not. The fallback is not a
/// compromise — a track with no identified exit genuinely offers no better
/// answer than "near the end" — but it is the difference between a transition
/// that lands and one that merely happens on time.
///
/// # The bound is a safety rail, not a preference
///
/// An exit point later than the record can support — from a stale analysis, or
/// from a track that has since been trimmed — would push the incoming record
/// into silence. It is clamped rather than trusted.
pub(crate) fn advance(outgoing: &Candidate, overlap: Frames) -> Frames {
    let latest = outgoing
        .duration()
        .get()
        .saturating_sub(overlap.get())
        .max(0);
    let chosen = outgoing
        .best_exit()
        .map_or(latest, |exit| exit.position().get());
    Frames::new(chosen.clamp(0, latest))
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "a test that cannot build its own fixture should fail loudly"
    )]

    use super::*;
    use crate::candidate::{MixPoint, MixPointRole, TrackId};
    use prv_time::Tempo;

    const MINUTE: i64 = 44_100 * 60;

    fn record(seconds: i64) -> Candidate {
        Candidate::new(
            TrackId::new(1),
            Frames::new(44_100 * seconds),
            Tempo::from_bpm(128.0).expect("valid"),
            0.5,
        )
    }

    #[test]
    fn a_record_hands_over_at_its_exit_point() {
        let outgoing = record(300).with_point(MixPoint::new(
            Frames::new(MINUTE * 4),
            0.1,
            MixPointRole::Exit,
        ));
        assert_eq!(
            advance(&outgoing, Frames::new(44_100 * 8)),
            Frames::new(MINUTE * 4)
        );
    }

    #[test]
    fn a_record_with_no_exit_point_hands_over_one_overlap_before_the_end() {
        let outgoing = record(300);
        let overlap = Frames::new(44_100 * 8);
        assert_eq!(
            outgoing.duration().get() - overlap.get(),
            MINUTE * 5 - 44_100 * 8
        );
        assert_eq!(
            advance(&outgoing, overlap),
            Frames::new(MINUTE * 5 - 44_100 * 8)
        );
    }

    #[test]
    fn an_exit_point_too_late_to_use_is_clamped_rather_than_trusted() {
        // A stale analysis, or a track that has since been trimmed. Trusting it
        // would place the incoming record after the outgoing one had finished,
        // which is a gap of silence in the middle of a set.
        let outgoing = record(300).with_point(MixPoint::new(
            Frames::new(MINUTE * 40),
            0.1,
            MixPointRole::Exit,
        ));
        let overlap = Frames::new(44_100 * 8);
        assert_eq!(
            advance(&outgoing, overlap),
            Frames::new(MINUTE * 5 - 44_100 * 8),
            "an impossible exit point was used rather than clamped"
        );
    }

    #[test]
    fn an_overlap_longer_than_the_record_does_not_move_the_set_backwards() {
        // Frames are signed, so an unclamped subtraction here would produce a
        // negative advance and a track placed before the set began.
        let outgoing = record(60);
        let advanced = advance(&outgoing, Frames::new(44_100 * 600));
        assert_eq!(advanced, Frames::ZERO);
        assert!(advanced.get() >= 0);
    }

    #[test]
    fn an_exit_point_at_the_very_start_is_honoured_rather_than_treated_as_absent() {
        // Zero is a position, not a missing value. A record whose analysis says
        // "leave immediately" is unusual, but it is not the same as one with no
        // opinion, and conflating them is how `Option` gets flattened by
        // accident.
        let outgoing = record(300).with_point(MixPoint::new(Frames::ZERO, 0.1, MixPointRole::Exit));
        assert_eq!(advance(&outgoing, Frames::new(44_100 * 8)), Frames::ZERO);
    }
}
