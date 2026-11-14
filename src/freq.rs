//! Byte frequencies and the Shannon entropy they imply.

/// Number of distinct byte values.
pub const SYMBOLS: usize = 256;

/// Occurrence count for every byte value.
pub type Counts = [u64; SYMBOLS];

/// Counts how often each byte value occurs in `data`.
pub fn count(data: &[u8]) -> Counts {
    let mut counts = [0u64; SYMBOLS];
    for &byte in data {
        counts[usize::from(byte)] += 1;
    }
    counts
}

/// Number of byte values that occur at least once.
pub fn active(counts: &Counts) -> usize {
    counts.iter().filter(|&&c| c > 0).count()
}

/// Shannon entropy in bits per symbol. Zero for an empty table.
pub fn entropy(counts: &Counts) -> f64 {
    let total: f64 = counts.iter().map(|&c| c as f64).sum();
    if total == 0.0 {
        return 0.0;
    }
    counts
        .iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = c as f64 / total;
            -p * p.log2()
        })
        .sum()
}
