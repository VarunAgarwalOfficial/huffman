//! Library-level integration tests: round trips, code properties, damage.

use huffman::container::{encode_stream_with_max_len, BLOCK_SIZE};
use huffman::{canonical, decode, encode, freq, tree, Error};

const SAMPLE: &[u8] = include_bytes!("../samples/lighthouse.txt");

/// Deterministic pseudo-random bytes (xorshift64*).
fn random_bytes(len: usize) -> Vec<u8> {
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    (0..len)
        .map(|_| {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            (state.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 56) as u8
        })
        .collect()
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
fn round_trips() {
    // (name, input, expected container size if it is known exactly)
    // 22 bytes of file header and end marker, 15 per block, 2 per table entry.
    let cases: Vec<(&str, Vec<u8>, Option<usize>)> = vec![
        ("empty", vec![], Some(22)),
        ("one byte", vec![0xff], Some(22 + 15 + 2 + 1)),
        (
            "one repeated symbol",
            vec![b'q'; 1000],
            Some(22 + 15 + 2 + 125),
        ),
        (
            "bit count divisible by 8",
            b"abcdabcd".to_vec(),
            Some(22 + 15 + 8 + 2),
        ),
        (
            "all 256 values",
            (0..=255).collect(),
            Some(22 + 15 + 512 + 256),
        ),
        ("text", SAMPLE.to_vec(), None),
        // Three blocks, the last one partial.
        (
            "random binary over several blocks",
            random_bytes(2 * BLOCK_SIZE + 4321),
            None,
        ),
    ];
    for (name, data, size) in cases {
        let container = encode(&data).unwrap();
        assert_eq!(decode(&container).unwrap(), data, "{name}");
        if let Some(size) = size {
            assert_eq!(container.len(), size, "{name}");
        }
    }
    assert!(encode(SAMPLE).unwrap().len() < SAMPLE.len() * 2 / 3);
}

#[test]
fn codes_are_prefix_free_and_within_one_bit_of_entropy() {
    for data in [SAMPLE.to_vec(), random_bytes(3000), fibonacci_bytes(16)] {
        let counts = freq::count(&data);
        let lengths = tree::code_lengths(&counts);
        let words: Vec<String> = canonical::assign(&lengths)
            .unwrap()
            .iter()
            .filter(|c| c.len > 0)
            .map(|c| c.to_bit_string())
            .collect();
        for (i, a) in words.iter().enumerate() {
            for (j, b) in words.iter().enumerate() {
                assert!(i == j || !b.starts_with(a.as_str()), "{a} prefixes {b}");
            }
        }

        let average = tree::total_bits(&counts, &lengths) as f64 / data.len() as f64;
        let entropy = freq::entropy(&counts);
        assert!(average >= entropy - 1e-9, "{average} < {entropy}");
        assert!(average < entropy + 1.0, "{average} >= {entropy} + 1");
    }
}

#[test]
fn fibonacci_counts_round_trip_with_long_and_limited_codes() {
    let data = fibonacci_bytes(24);
    let lengths = tree::code_lengths(&freq::count(&data));
    assert_eq!(lengths.iter().copied().max(), Some(23));
    assert_eq!(decode(&encode(&data).unwrap()).unwrap(), data);

    let mut limited = Vec::new();
    encode_stream_with_max_len(data.as_slice(), &mut limited, 8).unwrap();
    // The table of the only block starts after 5 + 11 bytes: 24 (symbol, length) pairs.
    assert!(limited[16..16 + 48].chunks(2).all(|pair| pair[1] <= 8));
    assert_eq!(decode(&limited).unwrap(), data);
}

#[test]
fn corrupt_and_truncated_input_is_an_error() {
    let good = encode(SAMPLE).unwrap();

    for cut in 0..good.len() {
        assert_eq!(decode(&good[..cut]), Err(Error::Truncated), "cut {cut}");
    }

    let mut bad = good.clone();
    bad[0] = b'X';
    assert_eq!(decode(&bad), Err(Error::BadMagic));
    assert_eq!(decode(SAMPLE), Err(Error::BadMagic));

    // A version 1 file is refused with a message that says what to do.
    let mut bad = good.clone();
    bad[4] = 1;
    assert_eq!(decode(&bad), Err(Error::UnsupportedVersion(1)));
    assert!(decode(&bad).unwrap_err().to_string().contains("version 1"));

    let mut bad = good.clone();
    bad.push(0);
    assert_eq!(decode(&bad), Err(Error::TrailingData));

    // A huge claimed block length is refused before anything is allocated.
    let mut bad = good.clone();
    bad[6..10].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(
        decode(&bad),
        Err(Error::InvalidBlock { block: 1, .. })
    ));

    // Any single damaged byte is an error: nothing slips past the checksums.
    for index in 0..good.len() {
        let mut bad = good.clone();
        bad[index] ^= 0x10;
        assert!(
            decode(&bad).is_err(),
            "damage at byte {index} went unnoticed"
        );
    }
}

#[test]
fn checksums_name_the_damaged_block() {
    // With all 256 values equally often every code is 8 bits, so one flipped
    // payload bit changes one symbol and still decodes: only the CRC sees it.
    let flat: Vec<u8> = (0..=255u8).cycle().take(2 * BLOCK_SIZE + 512).collect();
    let good = encode(&flat).unwrap();
    let block_len = 15 + 512 + BLOCK_SIZE;
    let mut bad = good.clone();
    bad[5 + block_len + 15 + 512 + 1000] ^= 0x01;
    assert!(matches!(
        decode(&bad),
        Err(Error::ChecksumMismatch { block: 2, .. })
    ));

    // Dropping the second block leaves two valid blocks; the end marker objects.
    let mut bad = good.clone();
    bad.drain(5 + block_len..5 + 2 * block_len);
    assert!(matches!(decode(&bad), Err(Error::EndMismatch(_))));
}
