//! Minimal deterministic PRNG (`SplitMix64`) for `BreakQueue` shuffling.

/// Deterministic 64-bit `SplitMix64` pseudo-random number generator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    /// Creates a new generator from an explicit seed.
    #[must_use]
    pub(crate) const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Generates the next pseudo-random `u64`.
    pub(crate) const fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Shuffles `slice` in place using the Fisher-Yates algorithm.
    pub(crate) fn shuffle<T>(&mut self, slice: &mut [T]) {
        if slice.len() <= 1 {
            return;
        }
        for i in (1..slice.len()).rev() {
            let bound = u64::try_from(i + 1).unwrap_or(u64::MAX);
            let j = usize::try_from(self.next_u64() % bound).unwrap_or(0);
            slice.swap(i, j);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SplitMix64;

    #[test]
    fn splitmix64_is_deterministic_for_same_seed() {
        let mut a = SplitMix64::new(42);
        let mut b = SplitMix64::new(42);
        for _ in 0..16 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn shuffle_preserves_all_elements_and_handles_small_slices() {
        let mut rng = SplitMix64::new(12345);
        let mut empty: [u32; 0] = [];
        rng.shuffle(&mut empty);

        let mut single = [7u32];
        rng.shuffle(&mut single);
        assert_eq!(single, [7]);

        let mut data: Vec<u32> = (0..13).collect();
        rng.shuffle(&mut data);
        let mut sorted = data.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..13).collect::<Vec<u32>>());
        assert_ne!(data, (0..13).collect::<Vec<u32>>());
    }
}
