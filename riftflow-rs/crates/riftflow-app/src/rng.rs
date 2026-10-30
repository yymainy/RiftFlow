//! A tiny splitmix64 generator.
//!
//! The features that need randomness pick a lobby member to promote or a vote to
//! cast — anything but security. Seeding from the clock keeps the shipped binary
//! free of an RNG dependency; do not use this for tokens or identifiers.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static COUNTER: AtomicU64 = AtomicU64::new(0);

pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|since| since.as_nanos() as u64)
            .unwrap_or(0);
        let mixed = nanos
            ^ (std::process::id() as u64) << 32
            ^ COUNTER.fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed);
        Self { state: mixed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A uniform index into a slice of `count` items, or 0 when empty.
    pub fn index(&mut self, count: usize) -> usize {
        if count == 0 {
            return 0;
        }
        (self.next_u64() % count as u64) as usize
    }

    /// `count` distinct indices, uniformly — what `ChoiceMaker` with equal weights
    /// does, done as a partial Fisher-Yates shuffle.
    pub fn distinct_indices(&mut self, length: usize, count: usize) -> Vec<usize> {
        let take = count.min(length);
        let mut pool: Vec<usize> = (0..length).collect();

        for index in 0..take {
            let swap = index + self.index(pool.len() - index);
            pool.swap(index, swap);
        }

        pool.truncate(take);
        pool
    }
}

impl Default for Rng {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::Rng;

    #[test]
    fn distinct_indices_never_repeat_and_stay_in_range() {
        let mut rng = Rng::new();
        for _ in 0..200 {
            let picked = rng.distinct_indices(9, 4);
            assert_eq!(picked.len(), 4);
            let mut sorted = picked.clone();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(sorted.len(), 4, "{picked:?} had a repeat");
            assert!(sorted.iter().all(|index| *index < 9));
        }
    }

    #[test]
    fn asking_for_more_than_available_takes_everything_once() {
        let mut rng = Rng::new();
        let picked = rng.distinct_indices(3, 10);
        assert_eq!(picked.len(), 3);
        let mut sorted = picked;
        sorted.sort_unstable();
        assert_eq!(sorted, vec![0, 1, 2]);
    }

    #[test]
    fn index_spreads_over_the_range() {
        let mut rng = Rng::new();
        let mut buckets = [0; 4];
        for _ in 0..4000 {
            buckets[rng.index(4)] += 1;
        }
        assert!(
            buckets.iter().all(|count| *count > 700),
            "skewed distribution: {buckets:?}"
        );
    }
}
