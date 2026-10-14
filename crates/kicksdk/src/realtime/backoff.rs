use serde::{Deserialize, Serialize};

/// When to try again after a failed or dropped connection: 1 s, doubling to
/// 30 s, back to 1 s once a connection has worked. Pure arithmetic; the host
/// owns the timer.
///
/// Many clients dropped at once (a server restart) would all come back in
/// step; [`Backoff::next_delay_jittered`] spreads them, from a random number
/// the host draws (the core has no randomness of its own).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Backoff {
    /// The first wait, ms. At least 1.
    pub initial_ms: u64,
    /// The longest wait, ms. At least `initial_ms`.
    pub max_ms: u64,
    next_ms: u64,
}

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
        Self { initial_ms, max_ms, next_ms: initial_ms }
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
        let r = if random.is_nan() { 0.0 } else { random.clamp(0.0, 1.0) };
        let half = delay / 2;
        // `delay - half` is at most 2^63, which an f64 holds well enough for
        // a spread; the product is at most that, so the cast cannot wrap.
        (half + ((delay - half) as f64 * r) as u64).clamp(1, delay)
    }

    /// A connection worked: the next failure starts from the beginning.
    pub fn reset(&mut self) {
        self.next_ms = self.bounds().0;
    }
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
