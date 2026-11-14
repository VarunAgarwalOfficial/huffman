//! Library-level integration tests: round trips, damage, and code properties.

use huffman::canonical::{self, MAX_CODE_LEN};
use huffman::container::{self, encode_with_max_len};
use huffman::{decode, encode, freq, tree, Error};

/// xorshift64*: small, deterministic, good enough for test data.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn bytes(&mut self, len: usize) -> Vec<u8> {
        (0..len).map(|_| (self.next() >> 56) as u8).collect()
    }
}

const SAMPLE: &[u8] = include_bytes!("../samples/lighthouse.txt");

fn assert_round_trip(data: &[u8]) -> Vec<u8> {
    let container = encode(data).expect("encode");
    assert_eq!(decode(&container).expect("decode"), data);
    container
}

/// Bytes whose counts follow the Fibonacci numbers: the worst case for depth.
fn fibonacci_bytes(symbols: usize) -> Vec<u8> {
    let mut data = Vec::new();
    let (mut a, mut b) = (1usize, 1usize);
    for symbol in 0..symbols {
        data.extend(std::iter::repeat_n(symbol as u8, a));
        (a, b) = (b, a + b);
    }
    data
}

#[test]
fn empty_input() {
    let container = assert_round_trip(b"");
    assert_eq!(container.len(), container::FIXED_HEADER_LEN);
}

#[test]
fn one_byte() {
    for byte in [0u8, b'a', 0xff] {
        assert_round_trip(&[byte]);
    }
}

#[test]
fn one_repeated_symbol() {
    for len in [2usize, 7, 8, 9, 1000] {
        let container = assert_round_trip(&vec![b'q'; len]);
        assert_eq!(container.len(), container::header_len(1) + len.div_ceil(8));
    }
}

#[test]
fn all_256_byte_values() {
    let data: Vec<u8> = (0..=255).collect();
    let container = assert_round_trip(&data);
    // 256 symbols need a count wider than one byte; every code is 8 bits.
    assert_eq!(&container[13..15], &256u16.to_le_bytes());
    assert_eq!(container.len(), container::header_len(256) + 256);
}

#[test]
fn all_256_byte_values_with_uneven_counts() {
    let mut data = Vec::new();
    for byte in 0..=255u8 {
        data.extend(std::iter::repeat_n(byte, 1 + usize::from(byte) % 17));
    }
    assert_round_trip(&data);
}

#[test]
fn random_binary() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for len in [1usize, 2, 3, 100, 4096, 70_000] {
        assert_round_trip(&rng.bytes(len));
    }
}

#[test]
fn random_skewed_binary() {
    let mut rng = Rng(42);
    // Squaring a uniform value piles the mass onto small byte values.
    let data: Vec<u8> = (0..50_000)
        .map(|_| {
            let x = rng.next() >> 56;
            ((x * x) >> 8) as u8
        })
        .collect();
    let container = assert_round_trip(&data);
    assert!(container.len() < data.len());
}

#[test]
fn text_sample() {
    let container = assert_round_trip(SAMPLE);
    assert!(container.len() < SAMPLE.len() * 2 / 3);
}

#[test]
fn bit_length_multiple_of_eight() {
    // Four equally likely symbols: two bits each, 16 symbols, 4 payload bytes.
    let data = b"abcdabcdabcdabcd";
    let container = assert_round_trip(data);
    assert_eq!(container.len(), container::header_len(4) + 4);
}

#[test]
fn fibonacci_counts_round_trip_with_long_codes() {
    let data = fibonacci_bytes(24);
    let lengths = tree::code_lengths(&freq::count(&data));
    assert_eq!(lengths.iter().copied().max(), Some(23));
    assert_round_trip(&data);
}

#[test]
fn length_limited_codes_round_trip() {
    let data = fibonacci_bytes(20);
    let unlimited = encode(&data).unwrap();
    for max_len in [5u8, 8, 12] {
        let container = encode_with_max_len(&data, max_len).unwrap();
        let header = container::read_header(&container).unwrap();
        assert!(header.lengths.iter().all(|&l| l <= max_len));
        assert_eq!(decode(&container).unwrap(), data);
        assert!(container.len() >= unlimited.len());
    }
}

#[test]
fn codes_are_prefix_free() {
    let mut rng = Rng(7);
    let inputs: Vec<Vec<u8>> = vec![
        SAMPLE.to_vec(),
        (0..=255).collect(),
        rng.bytes(3000),
        fibonacci_bytes(16),
        b"ab".to_vec(),
    ];
    for data in inputs {
        let lengths = tree::code_lengths(&freq::count(&data));
        let codes = canonical::assign(&lengths).unwrap();
        let words: Vec<String> = codes
            .iter()
            .filter(|c| c.len > 0)
            .map(|c| c.to_bit_string())
            .collect();
        for (i, a) in words.iter().enumerate() {
            for (j, b) in words.iter().enumerate() {
                assert!(i == j || !b.starts_with(a.as_str()), "{a} prefixes {b}");
            }
        }
    }
}

#[test]
fn average_length_is_within_one_bit_of_entropy() {
    let mut rng = Rng(99);
    let mut inputs: Vec<Vec<u8>> = vec![
        SAMPLE.to_vec(),
        (0..=255).collect(),
        fibonacci_bytes(20),
        b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaab".to_vec(),
    ];
    for len in [2usize, 10, 500, 20_000] {
        inputs.push(rng.bytes(len));
    }
    for data in inputs {
        let counts = freq::count(&data);
        // With one distinct symbol the entropy is 0 but a code needs 1 bit.
        if freq::active(&counts) < 2 {
            continue;
        }
        let lengths = tree::code_lengths(&counts);
        let average = tree::total_bits(&counts, &lengths) as f64 / data.len() as f64;
        let entropy = freq::entropy(&counts);
        assert!(average >= entropy - 1e-9, "{average} < {entropy}");
        assert!(average < entropy + 1.0, "{average} >= {entropy} + 1");
    }
}

#[test]
fn textbook_frequencies_cost_224_bits() {
    let mut data = Vec::new();
    for (symbol, count) in [
        (b'a', 45),
        (b'b', 13),
        (b'c', 12),
        (b'd', 16),
        (b'e', 9),
        (b'f', 5),
    ] {
        data.extend(std::iter::repeat_n(symbol, count));
    }
    let container = assert_round_trip(&data);
    // 224 bits is exactly 28 payload bytes.
    assert_eq!(container.len(), container::header_len(6) + 28);
}

#[test]
fn damaged_headers_are_errors() {
    let good = encode(SAMPLE).unwrap();

    let mut bad = good.clone();
    bad[..4].copy_from_slice(b"GZIP");
    assert_eq!(decode(&bad), Err(Error::BadMagic));

    let mut bad = good.clone();
    bad[4] = 0;
    assert_eq!(decode(&bad), Err(Error::UnsupportedVersion(0)));

    let mut bad = good.clone();
    bad[container::FIXED_HEADER_LEN + 1] = MAX_CODE_LEN + 1;
    assert!(matches!(decode(&bad), Err(Error::InvalidTable(_))));

    // Claim one more byte than the payload holds.
    let mut bad = good.clone();
    bad[5..13].copy_from_slice(&(SAMPLE.len() as u64 + 1000).to_le_bytes());
    assert!(decode(&bad).is_err());

    assert_eq!(decode(b""), Err(Error::Truncated));
    assert_eq!(decode(b"HUF"), Err(Error::Truncated));
    assert_eq!(decode(SAMPLE), Err(Error::BadMagic));
}

#[test]
fn every_truncation_is_an_error() {
    let mut rng = Rng(5);
    let all: Vec<u8> = (0..=255).collect();
    for data in [SAMPLE.to_vec(), rng.bytes(700), all, vec![b'x'; 100]] {
        let container = encode(&data).unwrap();
        for cut in 0..container.len() {
            assert_eq!(
                decode(&container[..cut]),
                Err(Error::Truncated),
                "cut {cut}"
            );
        }
    }
}

#[test]
fn arbitrary_damage_never_panics() {
    // No checksum, so some payload damage decodes to different bytes. The
    // only claim here is that decode always returns.
    let mut rng = Rng(1234);
    let small: Vec<u8> = b"abacabadabacaba, a small input".to_vec();
    let containers = [
        encode(&small).unwrap(),
        encode(&rng.bytes(300)).unwrap(),
        encode(&[b'x'; 40]).unwrap(),
        encode(b"").unwrap(),
    ];
    for container in &containers {
        // Every single-byte change in the header and table, three values each.
        let header_end = container::read_header(container).unwrap().payload_start;
        for index in 0..container.len() {
            let flips: &[u8] = if index < header_end {
                &[0x01, 0x80, 0xff]
            } else {
                &[0x10]
            };
            for &flip in flips {
                let mut damaged = container.clone();
                damaged[index] ^= flip;
                let _ = decode(&damaged);
            }
        }
        // Random multi-byte damage and random tails.
        for _ in 0..2000 {
            let mut damaged = container.clone();
            for _ in 0..1 + rng.next() % 4 {
                let index = (rng.next() % damaged.len() as u64) as usize;
                damaged[index] = (rng.next() >> 56) as u8;
            }
            if rng.next().is_multiple_of(4) {
                let extra = (rng.next() % 9) as usize;
                damaged.extend(rng.bytes(extra));
            }
            let _ = decode(&damaged);
        }
    }
    // Pure noise behind a valid magic and version.
    for _ in 0..2000 {
        let mut noise = b"HUFF\x01".to_vec();
        let len = (rng.next() % 600) as usize;
        noise.extend(rng.bytes(len));
        let _ = decode(&noise);
    }
}
