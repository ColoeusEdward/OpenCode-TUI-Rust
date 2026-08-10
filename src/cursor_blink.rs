//! Prompt cursor blink timing.
//!
//! The Composer draws its own cursor as a styled span, so blinking is decided at
//! render time from elapsed wall-clock time rather than by the terminal.
//!
//! While the session is thinking the cursor blinks fast, and the period drifts
//! inside a bounded range instead of holding one exact rate, which reads as
//! activity rather than as a fixed indicator. Idle blinking uses a single calm
//! period. The drift is deterministic for a given elapsed time so rendering the
//! same frame twice cannot produce two different results.

use std::time::Duration;

/// Blink period when the session is idle.
const IDLE_PERIOD: Duration = Duration::from_millis(600);

/// Bounds for the thinking-state period. The midpoint is the nominal fast rate
/// and the drift moves the period between these two values.
const THINKING_MIN_PERIOD: Duration = Duration::from_millis(90);
const THINKING_MAX_PERIOD: Duration = Duration::from_millis(210);

/// The shortest half-period this module can produce.
///
/// Exposed so the render loop can prove its frame interval samples the blink
/// fast enough; a frame rate slower than this aliases the fast blink away.
pub const MIN_HALF_PERIOD: Duration = THINKING_MIN_PERIOD;

/// How long one full sweep from the minimum period to the maximum and back takes.
/// Slow relative to the blink itself, so the rate change is perceptible as drift
/// rather than as jitter.
const DRIFT_CYCLE: Duration = Duration::from_millis(2300);

/// Tracks the blink phase and reports whether the cursor is currently visible.
///
/// The phase is *integrated* rather than derived from total elapsed time. Because
/// the thinking period drifts, `elapsed % period(elapsed)` is not a continuous
/// phase — as the period changes, that expression jumps and can even run
/// backwards, which shows up as an irregular twitch instead of a blink. Each
/// step therefore advances the phase by `delta / current_period`, so the phase
/// only ever moves forward at the current rate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CursorBlink {
    /// Time accumulated since the blink was last reset. Drives the drift sweep
    /// only, never the on/off decision.
    elapsed: Duration,
    /// Position within the current on/off cycle, in half-periods. The integer
    /// part counts completed half-periods; the cursor is visible while that
    /// count is even.
    half_periods: f64,
}

impl Default for CursorBlink {
    fn default() -> Self {
        Self::new()
    }
}

impl CursorBlink {
    pub const fn new() -> Self {
        Self {
            elapsed: Duration::ZERO,
            half_periods: 0.0,
        }
    }

    /// Advances the blink phase by `delta`, blinking at `thinking`'s rate.
    ///
    /// The caller passes the same state it will render with, so the phase always
    /// advances at the rate the user is about to see.
    pub fn advance(&mut self, delta: Duration, thinking: bool) {
        // The drift sweep is periodic, so the accumulator can wrap on a whole
        // number of drift cycles without moving the sweep position.
        let wrap = DRIFT_CYCLE.saturating_mul(64);
        self.elapsed = self.elapsed.saturating_add(delta);
        if self.elapsed >= wrap {
            self.elapsed -= wrap;
        }

        let period = self.period(thinking).as_secs_f64().max(f64::MIN_POSITIVE);
        self.half_periods += delta.as_secs_f64() / period;
        // Keep the phase counter small and on an even boundary, so wrapping never
        // flips the cursor's visibility.
        if self.half_periods >= 1_048_576.0 {
            self.half_periods %= 2.0;
        }
    }

    /// Restarts the blink at a visible cursor. Used on input so typing never
    /// leaves the cursor hidden at the moment the user looks for it.
    pub fn restart(&mut self) {
        self.elapsed = Duration::ZERO;
        self.half_periods = 0.0;
    }

    /// Whether the cursor should be drawn this frame.
    pub fn is_visible(&self) -> bool {
        // Visible during even half-periods, hidden during odd ones.
        (self.half_periods as u64).is_multiple_of(2)
    }

    /// The current half-period, i.e. how long the cursor stays in one state.
    fn period(&self, thinking: bool) -> Duration {
        if !thinking {
            return IDLE_PERIOD;
        }
        let min = THINKING_MIN_PERIOD.as_nanos();
        let max = THINKING_MAX_PERIOD.as_nanos();
        let span = max.saturating_sub(min);
        if span == 0 {
            return THINKING_MIN_PERIOD;
        }

        // Triangle wave over the drift cycle: 0 -> span -> 0. A triangle is used
        // rather than a sine so the value is exact integer arithmetic and the
        // function stays deterministic across platforms.
        let cycle = DRIFT_CYCLE.as_nanos().max(1);
        let position = self.elapsed.as_nanos() % cycle;
        let half = cycle / 2;
        let ramp = if position < half {
            position
        } else {
            cycle - position
        };
        let offset = span * ramp / half.max(1);
        Duration::from_nanos((min + offset).min(u64::MAX as u128) as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CursorBlink, DRIFT_CYCLE, IDLE_PERIOD, MIN_HALF_PERIOD, THINKING_MAX_PERIOD,
        THINKING_MIN_PERIOD,
    };
    use std::time::Duration;

    /// A frame interval that samples the fastest blink several times per
    /// half-period, matching what the render loop uses.
    const FRAME: Duration = Duration::from_millis(10);

    /// Samples visibility over `duration` and returns the length in frames of each
    /// on/off run.
    fn run_lengths(thinking: bool, duration: Duration) -> Vec<usize> {
        let mut blink = CursorBlink::new();
        let mut runs = Vec::new();
        let mut current = blink.is_visible();
        let mut length = 0usize;
        let frames = duration.as_nanos() / FRAME.as_nanos();
        for _ in 0..frames {
            blink.advance(FRAME, thinking);
            let next = blink.is_visible();
            length += 1;
            if next != current {
                runs.push(length);
                length = 0;
                current = next;
            }
        }
        runs
    }

    #[test]
    fn a_fresh_blink_shows_the_cursor() {
        assert!(CursorBlink::new().is_visible());
    }

    #[test]
    fn idle_blinking_alternates_on_a_fixed_period() {
        let mut blink = CursorBlink::new();
        blink.advance(IDLE_PERIOD, false);
        assert!(!blink.is_visible());
        blink.advance(IDLE_PERIOD, false);
        assert!(blink.is_visible());
    }

    #[test]
    fn thinking_blinks_several_times_faster_than_idle() {
        let window = Duration::from_secs(4);
        let idle = run_lengths(false, window).len();
        let thinking = run_lengths(true, window).len();

        assert!(
            thinking >= idle * 3,
            "the thinking cursor should blink several times faster than idle, \
             got {thinking} runs vs {idle} over {window:?}"
        );
    }

    #[test]
    fn every_thinking_run_stays_inside_the_configured_bounds() {
        // Regression: the phase used to be derived as `elapsed % period(elapsed)`.
        // Because the period drifts, that expression jumped and sometimes ran
        // backwards, producing runs far longer than the configured maximum — an
        // irregular twitch rather than a blink. Integrating the phase keeps every
        // run within the bounds.
        let runs = run_lengths(true, DRIFT_CYCLE * 3);
        assert!(!runs.is_empty(), "the thinking cursor should blink at all");

        let frame_ms = FRAME.as_millis() as usize;
        let min_frames = THINKING_MIN_PERIOD.as_millis() as usize / frame_ms;
        let max_frames = THINKING_MAX_PERIOD.as_millis() as usize / frame_ms;
        // One frame of rounding slack at each end, since a run boundary can fall
        // between samples.
        for (index, length) in runs.iter().enumerate() {
            assert!(
                *length + 1 >= min_frames && *length <= max_frames + 1,
                "run {index} lasted {length} frames, outside the {min_frames}..={max_frames} \
                 frame range implied by the configured periods; full runs: {runs:?}"
            );
        }
    }

    #[test]
    fn the_thinking_rate_actually_drifts_across_a_cycle() {
        let runs = run_lengths(true, DRIFT_CYCLE * 3);
        let shortest = runs.iter().min().copied().expect("runs were measured");
        let longest = runs.iter().max().copied().expect("runs were measured");

        assert!(
            longest > shortest,
            "the blink rate should float rather than hold one value; every run was \
             {shortest} frames"
        );
    }

    #[test]
    fn the_blink_rate_follows_a_change_in_the_thinking_state() {
        // Advancing with `thinking = true` then `false` must change the observed
        // rate, since the rate is chosen per step rather than from total elapsed
        // time.
        let mut blink = CursorBlink::new();
        let mut fast_flips = 0;
        let mut previous = blink.is_visible();
        for _ in 0..100 {
            blink.advance(FRAME, true);
            if blink.is_visible() != previous {
                fast_flips += 1;
            }
            previous = blink.is_visible();
        }

        let mut slow_flips = 0;
        for _ in 0..100 {
            blink.advance(FRAME, false);
            if blink.is_visible() != previous {
                slow_flips += 1;
            }
            previous = blink.is_visible();
        }

        assert!(
            fast_flips > slow_flips,
            "the same elapsed time should produce more flips while thinking, \
             got {fast_flips} vs {slow_flips}"
        );
    }

    #[test]
    fn restart_makes_the_cursor_visible_again() {
        let mut blink = CursorBlink::new();
        blink.advance(IDLE_PERIOD, false);
        assert!(!blink.is_visible());

        blink.restart();
        assert!(blink.is_visible());
    }

    #[test]
    fn the_exposed_minimum_matches_the_fastest_configured_period() {
        // The render loop asserts its frame interval against this constant, so it
        // has to stay tied to the real minimum.
        assert_eq!(MIN_HALF_PERIOD, THINKING_MIN_PERIOD);
    }

    #[test]
    fn the_accumulators_stay_bounded_over_a_long_session() {
        let mut blink = CursorBlink::new();
        for index in 0..500_000 {
            blink.advance(FRAME, index % 2 == 0);
        }

        assert!(blink.elapsed < DRIFT_CYCLE * 64);
        assert!(
            blink.half_periods < 1_048_576.0,
            "the phase counter should wrap rather than grow without bound"
        );
    }
}
