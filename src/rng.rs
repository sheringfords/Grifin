//! Deterministic RNG (SplitMix64) + Zipf sampler.
//!
//! No external RNG dependency: the generator must be exactly reproducible
//! across platforms given the same seed.

/// SplitMix64: simple, fast, deterministic across platforms.
#[derive(Clone, Debug)]
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        // Domain-separate seed 0 (SplitMix64 degenerates less with nonzero state).
        Self {
            state: seed.wrapping_add(0x9E37_79B9_7F4A_7C15),
        }
    }

    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        let mut z = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        self.state = z;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    #[inline]
    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    /// Uniform in [0, n). Panics on n == 0 (fail loudly).
    #[inline]
    pub fn below(&mut self, n: u64) -> u64 {
        assert!(n > 0, "rng.below(0)");
        // Lemire's nearly-divisionless reduction; bias is irrelevant for
        // synthetic workload generation but keep it small anyway.
        ((self.next_u64() as u128 * n as u128) >> 64) as u64
    }

    /// Bernoulli(p) with p in [0, 1].
    #[inline]
    pub fn bernoulli(&mut self, p: f64) -> bool {
        assert!((0.0..=1.0).contains(&p), "bernoulli p out of range: {p}");
        const SCALE: u64 = 1 << 53;
        self.below(SCALE) < (p * SCALE as f64) as u64
    }

    /// Uniform in [lo, hi).
    #[inline]
    pub fn range(&mut self, lo: u64, hi: u64) -> u64 {
        assert!(hi > lo, "rng.range with hi <= lo");
        lo + self.below(hi - lo)
    }
}

/// Precomputed Zipf CDF table over ranks 1..=n with skew `theta`.
/// Rank 0 in the table = hottest item.
#[derive(Clone, Debug)]
pub struct Zipf {
    cdf: Vec<f64>,
}

impl Zipf {
    pub fn new(n: u64, theta: f64) -> Self {
        assert!(n > 0, "Zipf::new with n == 0");
        assert!(theta >= 0.0, "Zipf::new with negative theta");
        let n_usize = n as usize;
        let mut cdf = Vec::with_capacity(n_usize);
        let mut acc = 0.0;
        for rank in 1..=n {
            acc += 1.0 / (rank as f64).powf(theta);
            cdf.push(acc);
        }
        let total = acc;
        for v in cdf.iter_mut() {
            *v /= total;
        }
        // Ensure the last entry is exactly 1.0 so sampling never runs off the end.
        if let Some(last) = cdf.last_mut() {
            *last = 1.0;
        }
        Self { cdf }
    }

    /// Sample a rank in [0, n).
    pub fn sample(&self, rng: &mut Rng) -> u64 {
        const SCALE: u64 = 1 << 53;
        let u = (rng.below(SCALE) as f64) / (SCALE as f64);
        // Binary search; u < 1.0 always, last cdf entry is 1.0, so this cannot fail.
        match self
            .cdf
            .binary_search_by(|v| v.partial_cmp(&u).unwrap_or(std::cmp::Ordering::Less))
        {
            Ok(i) | Err(i) => (i.min(self.cdf.len() - 1)) as u64,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rng_is_deterministic() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..1000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn rng_distinct_seeds_differ() {
        let mut a = Rng::new(1);
        let mut b = Rng::new(2);
        assert_ne!(a.next_u64(), b.next_u64());
    }

    #[test]
    fn zipf_skew_concentrates() {
        let z = Zipf::new(100, 1.5);
        let mut rng = Rng::new(7);
        let mut hot = 0;
        for _ in 0..2000 {
            if z.sample(&mut rng) < 5 {
                hot += 1;
            }
        }
        // theta=1.5 over 100 items: top-5 should dominate (>50%).
        assert!(hot > 1000, "hot={hot}");
    }

    #[test]
    fn zipf_uniform_covers_range() {
        let z = Zipf::new(16, 0.0);
        let mut rng = Rng::new(9);
        let mut seen = [false; 16];
        for _ in 0..2000 {
            seen[z.sample(&mut rng) as usize] = true;
        }
        assert!(seen.iter().all(|&s| s));
    }
}
