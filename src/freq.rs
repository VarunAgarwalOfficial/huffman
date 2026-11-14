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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_every_byte() {
        let counts = count(b"abracadabra");
        assert_eq!(counts[usize::from(b'a')], 5);
        assert_eq!(counts[usize::from(b'b')], 2);
        assert_eq!(counts[usize::from(b'r')], 2);
        assert_eq!(counts[usize::from(b'c')], 1);
        assert_eq!(counts[usize::from(b'd')], 1);
        assert_eq!(counts.iter().sum::<u64>(), 11);
        assert_eq!(active(&counts), 5);
    }

    #[test]
    fn empty_input_has_no_symbols_and_zero_entropy() {
        let counts = count(b"");
        assert_eq!(active(&counts), 0);
        assert_eq!(entropy(&counts), 0.0);
    }

    #[test]
    fn entropy_of_known_distributions() {
        assert_eq!(entropy(&count(b"aaaa")), 0.0);
        assert!((entropy(&count(b"abab")) - 1.0).abs() < 1e-12);
        assert!((entropy(&count(b"abcd")) - 2.0).abs() < 1e-12);
        // 1/2, 1/4, 1/4 -> 1.5 bits
        assert!((entropy(&count(b"aabc")) - 1.5).abs() < 1e-12);
        let all: Vec<u8> = (0..=255).collect();
        assert!((entropy(&count(&all)) - 8.0).abs() < 1e-12);
    }
}
