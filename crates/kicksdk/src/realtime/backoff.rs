use serde::{Deserialize, Serialize};

/// When to try again after a failed or dropped connection: 1 s, doubling to
/// 30 s, back to 1 s once a connection has worked. Pure arithmetic; the host
/// owns the timer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Backoff {
    /// The first wait, ms.
    pub initial_ms: u64,
    /// The longest wait, ms.
    pub max_ms: u64,
    next_ms: u64,
}

impl Default for Backoff {
    fn default() -> Self {
        Self::new(1_000, 30_000)
    }
}

impl Backoff {
    /// A backoff from `initial_ms`, doubling to `max_ms`.
    pub fn new(initial_ms: u64, max_ms: u64) -> Self {
        Self { initial_ms, max_ms, next_ms: initial_ms }
    }

    /// How long to wait before the next attempt; the one after waits longer.
    pub fn next_delay(&mut self) -> u64 {
        let delay = self.next_ms;
        self.next_ms = (self.next_ms.saturating_mul(2)).min(self.max_ms);
        delay
    }

    /// A connection worked: the next failure starts from the beginning.
    pub fn reset(&mut self) {
        self.next_ms = self.initial_ms;
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
}
