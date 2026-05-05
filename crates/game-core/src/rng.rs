//! Deterministic RNG state for the engine.
//!
//! All randomness in the engine flows through [`RngState`]. The state
//! itself is a tuple of `(seed, draws)` — both serializable, both
//! trivially copyable into a snapshot — and the actual RNG is
//! reconstructed on demand from those two numbers via
//! [`rand_chacha::ChaCha8Rng`]. This keeps `GameState` cleanly
//! serializable and means the RNG state in a snapshot is just two
//! `u64`s, not a chunk of opaque RNG implementation.
//!
//! # Replay invariant
//!
//! Each `next_u64` draw advances `draws` by exactly one and consumes
//! exactly one 64-bit word from the `ChaCha8` stream. Higher-level
//! helpers (e.g. `next_index`) build on `next_u64` without consuming
//! variable bytes — that's required so replaying an action log produces
//! bit-identical state regardless of how the higher-level helpers are
//! layered.
//!
//! # Bias
//!
//! `next_index` uses simple modulo, which is biased for non-power-of-2
//! moduli. For the small moduli the engine actually uses (chaos bag
//! sizes ≤ 20, deck sizes ≤ ~50) the bias is negligible. Avoiding it
//! via rejection sampling would mean variable byte consumption per
//! draw, which breaks the replay invariant above.

use rand_chacha::rand_core::{RngCore, SeedableRng};
use serde::{Deserialize, Serialize};

/// Deterministic RNG state for [`GameState`](crate::GameState).
///
/// Constructed via [`new`](Self::new) with a seed; the engine advances
/// `draws` as it consumes random values.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RngState {
    /// Seed used to initialize the underlying `ChaCha8` stream.
    pub seed: u64,
    /// Number of `next_u64` draws performed so far.
    pub draws: u64,
}

impl RngState {
    /// Create a new RNG state at draw 0 from the given seed.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self { seed, draws: 0 }
    }

    /// Reconstruct the underlying `ChaCha8Rng` positioned at the current
    /// draw count.
    fn rng_at_current_pos(&self) -> rand_chacha::ChaCha8Rng {
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(self.seed);
        // Each `next_u64` call consumes 2 32-bit words from the
        // `ChaCha8` stream. `set_word_pos` is O(1).
        rng.set_word_pos(u128::from(self.draws) * 2);
        rng
    }

    /// Advance the stream by one 64-bit word and return it.
    ///
    /// Crate-private so all RNG access goes through engine-controlled
    /// paths; downstream code reads the result via `Event`s.
    pub(crate) fn next_u64(&mut self) -> u64 {
        let mut rng = self.rng_at_current_pos();
        let v = rng.next_u64();
        self.draws += 1;
        v
    }

    /// Pick an index in `[0, n)`. Panics if `n == 0`.
    ///
    /// Modulo-biased; see module docs for why that's deliberate.
    pub(crate) fn next_index(&mut self, n: usize) -> usize {
        assert!(n > 0, "RngState::next_index requires n > 0");
        let v_mod = self.next_u64() % n as u64;
        // v_mod is in [0, n), and n was originally a usize, so v_mod
        // fits in usize on every target.
        usize::try_from(v_mod).expect("v_mod < n which fits in usize")
    }
}

#[cfg(test)]
mod tests {
    use super::RngState;

    #[test]
    fn same_seed_same_draws_produces_same_value() {
        let mut a = RngState::new(42);
        let mut b = RngState::new(42);
        assert_eq!(a.next_u64(), b.next_u64());
        assert_eq!(a.next_u64(), b.next_u64());
        assert_eq!(a.draws, 2);
        assert_eq!(b.draws, 2);
    }

    #[test]
    fn different_seeds_diverge() {
        let mut a = RngState::new(42);
        let mut b = RngState::new(43);
        assert_ne!(a.next_u64(), b.next_u64());
    }

    #[test]
    fn advancing_draws_field_skips_ahead() {
        // Two RngStates: one drawn 5 times the slow way, one constructed
        // with draws = 5. The next draw on each should match.
        let mut slow = RngState::new(99);
        for _ in 0..5 {
            slow.next_u64();
        }
        let mut fast = RngState { seed: 99, draws: 5 };
        assert_eq!(slow.next_u64(), fast.next_u64());
    }

    #[test]
    fn next_index_stays_in_range() {
        let mut rng = RngState::new(7);
        for _ in 0..50 {
            let i = rng.next_index(13);
            assert!(i < 13);
        }
    }

    #[test]
    #[should_panic(expected = "n > 0")]
    fn next_index_panics_on_zero() {
        let mut rng = RngState::new(7);
        let _ = rng.next_index(0);
    }
}
