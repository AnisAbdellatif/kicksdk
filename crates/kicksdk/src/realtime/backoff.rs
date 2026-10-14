use serde::{Deserialize, Serialize};

/// When to try again after a failed or dropped connection: 1 s, doubling to
/// 30 s, back to 1 s once the chat feed is subscribed. Pure arithmetic; the
/// host owns the timer.
///
/// Many clients dropped at once (a server restart) would all come back in
/// step; [`Backoff::next_delay_jittered`] spreads them, from a random number
/// the host draws (the core has no randomness of its own).
///
/// **Terminal closes** (a ban, a refused token, a connection limit: see
/// [`super::retry_after_ms`]) come with a minimum wait, `retry_after_ms`;
/// [`Backoff::next_delay_for`] takes it and keeps a second, slower stage for
/// them: that wait, doubled on each terminal close in a row, at most six
/// times it (5, 10, 20, 30, 30 … minutes from Kick's 5), plus up to a
/// quarter more, spread. [`Backoff::reset`] ends both stages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Backoff {
    /// The first wait, ms. At least 1.
    pub initial_ms: u64,
    /// The longest wait, ms. At least `initial_ms`.
    pub max_ms: u64,
    next_ms: u64,
    /// Terminal closes since the last reset.
    #[serde(default)]
    terminal_closes: u32,
}

/// The longest terminal wait, as a multiple of the minimum: 5 min → 30 min.
const TERMINAL_CAP: u64 = 6;

impl Default for Backoff {
    fn default() -> Self {
        Self::new(1_000, 30_000)
    }
}

impl Backoff {
    /// A backoff from `initial_ms`, doubling to `max_ms`. A first wait of 0
    /// counts as 1 ms (0 would retry in a busy loop), and a longest wait
    /// below the first is raised to it.
    pub fn new(initial_ms: u64, max_ms: u64) -> Self {
        let initial_ms = initial_ms.max(1);
        let max_ms = max_ms.max(initial_ms);
        Self { initial_ms, max_ms, next_ms: initial_ms, terminal_closes: 0 }
    }

    /// The bounds as they hold, whatever the public fields were set to.
    fn bounds(&self) -> (u64, u64) {
        let initial = self.initial_ms.max(1);
        (initial, self.max_ms.max(initial))
    }

    /// How long to wait before the next attempt; the one after waits longer.
    pub fn next_delay(&mut self) -> u64 {
        let (initial, max) = self.bounds();
        let delay = self.next_ms.clamp(initial, max);
        self.next_ms = delay.saturating_mul(2).min(max);
        delay
    }

    /// [`Backoff::next_delay`], spread: somewhere from half of it to all of
    /// it, by `random`, a number the host draws uniformly from `[0, 1)`
    /// (`Math.random()`, `random.random()`, …). Out of range is clamped; NaN
    /// counts as 0. Never less than 1 ms.
    pub fn next_delay_jittered(&mut self, random: f64) -> u64 {
        let delay = self.next_delay();
        let half = delay / 2;
        (half + spread(delay - half, random)).clamp(1, delay)
    }

    /// How long to wait after a close that asks for at least
    /// `retry_after_ms` ([`super::Action::Close`]'s, or
    /// [`super::retry_after_ms`] of a socket's close code), spread by
    /// `random` from `[0, 1)` (out of range clamped, NaN as 0).
    ///
    /// - `0`, an ordinary drop: [`Backoff::next_delay_jittered`].
    /// - More, a terminal close: `retry_after_ms` doubled for each terminal
    ///   close before this one since the last [`Backoff::reset`], at most
    ///   six times `retry_after_ms`, plus `random` times a quarter of that.
    ///   Never less than `retry_after_ms`. The ordinary stage is left as it
    ///   was.
    ///
    /// With Kick's 5 minutes and `random` 0: 300 000, 600 000, 1 200 000,
    /// 1 800 000, 1 800 000 … ms; with `random` 0.5, an eighth more each.
    pub fn next_delay_for(&mut self, retry_after_ms: u64, random: f64) -> u64 {
        if retry_after_ms == 0 {
            return self.next_delay_jittered(random);
        }
        let doublings = self.terminal_closes.min(3);
        self.terminal_closes = self.terminal_closes.saturating_add(1);
        let wait = retry_after_ms.saturating_mul(1 << doublings).min(retry_after_ms.saturating_mul(TERMINAL_CAP));
        wait.saturating_add(spread(wait / 4, random))
    }

    /// The chat feed is subscribed: the next failure starts from the
    /// beginning, and so does the next terminal close.
    pub fn reset(&mut self) {
        self.next_ms = self.bounds().0;
        self.terminal_closes = 0;
    }
}

/// `random` (from `[0, 1)`; out of range clamped, NaN as 0) times `span`.
fn spread(span: u64, random: f64) -> u64 {
    let r = if random.is_nan() { 0.0 } else { random.clamp(0.0, 1.0) };
    // `span` is at most 2^64 - 1, which an f64 holds well enough for a
    // spread; the product is at most that, and the cast saturates.
    ((span as f64 * r) as u64).min(span)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doubles_to_the_cap_and_resets() {
        let mut b = Backoff::default();
        let delays: Vec<u64> = (0..7).map(|_| b.next_delay()).collect();
        assert_eq!(delays, [1_000, 2_000, 4_000, 8_000, 16_000, 30_000, 30_000]);
        b.reset();
        assert_eq!(b.next_delay(), 1_000);
    }

    #[test]
    fn bounds_are_sane() {
        let mut b = Backoff::new(0, 0);
        assert_eq!((b.initial_ms, b.max_ms), (1, 1));
        assert_eq!(b.next_delay(), 1, "never a busy loop");
        let mut b = Backoff::new(5_000, 1_000);
        assert_eq!((b.initial_ms, b.max_ms), (5_000, 5_000));
        assert_eq!(b.next_delay(), 5_000);
        let mut b = Backoff::new(u64::MAX, u64::MAX);
        assert_eq!((b.next_delay(), b.next_delay()), (u64::MAX, u64::MAX));
        // Fields set by hand are bounded too.
        let mut b = Backoff { initial_ms: 0, max_ms: 0, ..Default::default() };
        b.reset();
        assert_eq!(b.next_delay(), 1);
    }

    #[test]
    fn terminal_closes_wait_long_and_longer_until_reset() {
        let mut b = Backoff::default();
        let waits: Vec<u64> = (0..6).map(|_| b.next_delay_for(300_000, 0.0)).collect();
        assert_eq!(waits, [300_000, 600_000, 1_200_000, 1_800_000, 1_800_000, 1_800_000]);
        // The ordinary stage was left alone.
        assert_eq!(b.next_delay_for(0, 0.0), 500);
        assert_eq!(b.next_delay_for(0, 0.0), 1_000);
        // Subscribed again: both stages start over.
        b.reset();
        assert_eq!(b.next_delay_for(300_000, 0.0), 300_000);
        assert_eq!(b.next_delay(), 1_000);
        // An ordinary drop in between does not end the terminal stage.
        assert_eq!(b.next_delay_for(0, 0.0), 1_000);
        assert_eq!(b.next_delay_for(300_000, 0.0), 600_000);
    }

    #[test]
    fn terminal_waits_are_spread_upwards() {
        let mut b = Backoff::default();
        assert_eq!(b.next_delay_for(300_000, 0.5), 337_500);
        assert_eq!(b.next_delay_for(300_000, 0.999_999), 749_999);
        assert_eq!(b.next_delay_for(300_000, f64::NAN), 1_200_000);
        assert_eq!(b.next_delay_for(300_000, 7.0), 2_250_000);
        assert_eq!(b.next_delay_for(300_000, -1.0), 1_800_000);
        // Any minimum, never below it, and no overflow.
        let mut b = Backoff::default();
        assert_eq!(b.next_delay_for(1, 0.9), 1);
        assert_eq!(b.next_delay_for(1, 0.9), 2);
        let mut b = Backoff::default();
        assert_eq!(b.next_delay_for(u64::MAX, 1.0), u64::MAX);
        assert_eq!(b.next_delay_for(u64::MAX, 1.0), u64::MAX);
    }

    #[test]
    fn the_terminal_count_survives_serde_and_defaults_to_none() {
        let mut b = Backoff::default();
        b.next_delay_for(300_000, 0.0);
        let mut back: Backoff = serde_json::from_value(serde_json::to_value(&b).unwrap()).unwrap();
        assert_eq!(back.next_delay_for(300_000, 0.0), 600_000);
        // State saved before the terminal stage existed.
        let mut old: Backoff =
            serde_json::from_value(serde_json::json!({"initial_ms": 1000, "max_ms": 30000, "next_ms": 1000})).unwrap();
        assert_eq!(old.next_delay_for(300_000, 0.0), 300_000);
    }

    #[test]
    fn jitter_spreads_within_the_delay() {
        let mut b = Backoff::default();
        assert_eq!(b.next_delay_jittered(0.0), 500);
        assert_eq!(b.next_delay_jittered(0.5), 1_500);
        assert_eq!(b.next_delay_jittered(0.999_999), 3_999);
        assert_eq!(b.next_delay_jittered(f64::NAN), 4_000);
        assert_eq!(b.next_delay_jittered(7.0), 16_000);
        assert_eq!(b.next_delay_jittered(-1.0), 15_000);
        let mut b = Backoff::new(1, 1);
        assert_eq!(b.next_delay_jittered(0.0), 1);
        let mut b = Backoff::new(u64::MAX, u64::MAX);
        assert!(b.next_delay_jittered(1.0) >= u64::MAX / 2);
    }
}
