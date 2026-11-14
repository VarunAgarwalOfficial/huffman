//! The greedy Huffman merge: repeatedly join the two lightest subtrees.
//!
//! Only the depth of each leaf matters for the final code (see `canonical`),
//! so the tree is kept as a parent array instead of linked nodes.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::error::Error;
use crate::freq::{Counts, SYMBOLS};

/// Code length in bits for every byte value; zero means the byte is unused.
pub type Lengths = [u8; SYMBOLS];

/// Optimal code lengths for the given counts.
///
/// Ties are broken deterministically: between equal weights, leaves come
/// before merged nodes, lower byte values before higher ones and older merged
/// nodes before newer ones. A table with a single used symbol gets a one-bit
/// code so that every symbol still costs at least one bit.
pub fn code_lengths(counts: &Counts) -> Lengths {
    let symbols: Vec<usize> = (0..SYMBOLS).filter(|&s| counts[s] > 0).collect();
    let mut lengths = [0u8; SYMBOLS];
    match symbols.len() {
        0 => return lengths,
        1 => {
            lengths[symbols[0]] = 1;
            return lengths;
        }
        _ => {}
    }

    // Node ids: leaves first (in byte order), then merged nodes in the order
    // they are created. The heap orders by (weight, id), smallest first.
    let leaves = symbols.len();
    let mut parent = vec![usize::MAX; 2 * leaves - 1];
    let mut heap: BinaryHeap<Reverse<(u64, usize)>> = symbols
        .iter()
        .enumerate()
        .map(|(id, &s)| Reverse((counts[s], id)))
        .collect();

    let mut next = leaves;
    while let (Some(Reverse((wa, a))), Some(Reverse((wb, b)))) = (heap.pop(), heap.pop()) {
        parent[a] = next;
        parent[b] = next;
        heap.push(Reverse((wa.saturating_add(wb), next)));
        next += 1;
        if heap.len() == 1 {
            break;
        }
    }

    // A parent always has a larger id than its children, so walking ids
    // downwards from the root sees every parent before its children.
    let root = parent.len() - 1;
    let mut depth = vec![0u8; parent.len()];
    for id in (0..root).rev() {
        depth[id] = depth[parent[id]] + 1;
    }
    for (id, &s) in symbols.iter().enumerate() {
        lengths[s] = depth[id];
    }
    lengths
}

/// Code lengths with no code longer than `max_len` bits.
///
/// If the optimal tree is too deep, all counts are halved (rounding up, so no
/// used symbol drops to zero) and the tree is rebuilt, until it fits. This
/// flattens the distribution a little; the result is still a complete prefix
/// code, only no longer strictly optimal. Trees deeper than a few dozen levels
/// need Fibonacci-like counts, so in practice this never triggers.
pub fn limited_code_lengths(counts: &Counts, max_len: u8) -> Result<Lengths, Error> {
    let symbols = counts.iter().filter(|&&c| c > 0).count();
    // A balanced tree needs ceil(log2(symbols)) levels; fewer cannot work.
    let fits = max_len >= 1 && (max_len >= 8 || symbols <= 1usize << max_len);
    if !fits {
        return Err(Error::InvalidMaxLen { max_len, symbols });
    }
    let mut scaled = *counts;
    loop {
        let lengths = code_lengths(&scaled);
        if lengths.iter().all(|&l| l <= max_len) {
            return Ok(lengths);
        }
        for c in scaled.iter_mut() {
            *c = c.div_ceil(2);
        }
    }
}

/// Total encoded size in bits: the sum of count times code length.
pub fn total_bits(counts: &Counts, lengths: &Lengths) -> u64 {
    counts
        .iter()
        .zip(lengths.iter())
        .map(|(&c, &l)| c.saturating_mul(u64::from(l)))
        .fold(0, u64::saturating_add)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(pairs: &[(u8, u64)]) -> Counts {
        let mut counts = [0u64; SYMBOLS];
        for &(s, c) in pairs {
            counts[usize::from(s)] = c;
        }
        counts
    }

    #[test]
    fn textbook_example_costs_224() {
        let counts = table(&[
            (b'a', 45),
            (b'b', 13),
            (b'c', 12),
            (b'd', 16),
            (b'e', 9),
            (b'f', 5),
        ]);
        let lengths = code_lengths(&counts);
        let of = |s: u8| lengths[usize::from(s)];
        assert_eq!(
            [of(b'a'), of(b'b'), of(b'c'), of(b'd'), of(b'e'), of(b'f')],
            [1, 3, 3, 3, 4, 4]
        );
        assert_eq!(total_bits(&counts, &lengths), 224);
    }

    #[test]
    fn fibonacci_counts_are_deep_and_can_be_limited() {
        let mut counts = [0u64; SYMBOLS];
        let (mut a, mut b) = (1u64, 1u64);
        for c in counts.iter_mut().take(60) {
            *c = a;
            (a, b) = (b, a + b);
        }
        assert_eq!(code_lengths(&counts).iter().copied().max(), Some(59));

        for max_len in [6u8, 48] {
            let lengths = limited_code_lengths(&counts, max_len).unwrap();
            assert!(lengths.iter().all(|&l| l <= max_len));
            // Still a complete prefix code: the Kraft sum is exactly 1.
            let kraft: u64 = lengths
                .iter()
                .filter(|&&l| l > 0)
                .map(|&l| 1u64 << (max_len - l))
                .sum();
            assert_eq!(kraft, 1u64 << max_len);
        }
        assert!(limited_code_lengths(&counts, 5).is_err());
    }
}
