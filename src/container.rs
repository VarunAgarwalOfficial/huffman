//! The file format: a small header, the code length table, then the bits.
//!
//! | Offset   | Size  | Field                                              |
//! |----------|-------|----------------------------------------------------|
//! | 0        | 4     | magic bytes `HUFF`                                 |
//! | 4        | 1     | format version, currently 1                        |
//! | 5        | 8     | original length in bytes, little endian            |
//! | 13       | 2     | number of used symbols n (0..=256), little endian  |
//! | 15       | 2 * n | (byte value, code length) pairs, ascending by byte |
//! | 15 + 2n  | rest  | code words, MSB first, last byte zero padded       |
//!
//! Codes are canonical, so the lengths are all the decoder needs.

use crate::bits::{BitReader, BitWriter};
use crate::canonical::{self, Decoder, MAX_CODE_LEN};
use crate::error::Error;
use crate::freq::{self, SYMBOLS};
use crate::tree;

/// First four bytes of every container.
pub const MAGIC: [u8; 4] = *b"HUFF";
/// Format version written by this crate.
pub const VERSION: u8 = 1;
/// Bytes before the code length table.
pub const FIXED_HEADER_LEN: usize = 15;

/// Size of header plus table for a code with `symbols` used symbols.
pub fn header_len(symbols: usize) -> usize {
    FIXED_HEADER_LEN + 2 * symbols
}

/// Compresses `data` into a container.
pub fn encode(data: &[u8]) -> Result<Vec<u8>, Error> {
    encode_with_max_len(data, MAX_CODE_LEN)
}

/// Like [`encode`], but with no code longer than `max_len` bits (1..=48).
pub fn encode_with_max_len(data: &[u8], max_len: u8) -> Result<Vec<u8>, Error> {
    let counts = freq::count(data);
    let symbols = freq::active(&counts);
    if max_len > MAX_CODE_LEN {
        return Err(Error::InvalidMaxLen { max_len, symbols });
    }
    let lengths = tree::limited_code_lengths(&counts, max_len)?;
    let codes = canonical::assign(&lengths)?;

    let payload_bits = tree::total_bits(&counts, &lengths);
    let payload_len = usize::try_from(payload_bits.div_ceil(8)).unwrap_or(0);
    let mut out = Vec::with_capacity(header_len(symbols).saturating_add(payload_len));
    out.extend_from_slice(&MAGIC);
    out.push(VERSION);
    out.extend_from_slice(&(data.len() as u64).to_le_bytes());
    out.extend_from_slice(&(symbols as u16).to_le_bytes());
    for (symbol, &len) in lengths.iter().enumerate() {
        if len > 0 {
            out.push(symbol as u8);
            out.push(len);
        }
    }

    let mut writer = BitWriter::new();
    for &byte in data {
        let code = codes[usize::from(byte)];
        writer.write(code.bits, code.len);
    }
    out.extend_from_slice(&writer.finish());
    Ok(out)
}

/// The parts of a container before the payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    /// Length of the original data in bytes.
    pub original_len: u64,
    /// Code length per byte value, zero if unused.
    pub lengths: tree::Lengths,
    /// Offset of the first payload byte.
    pub payload_start: usize,
}

/// Reads and checks the header and code length table.
pub fn read_header(container: &[u8]) -> Result<Header, Error> {
    let magic = container.get(..4).ok_or(Error::Truncated)?;
    if magic != MAGIC {
        return Err(Error::BadMagic);
    }
    let version = *container.get(4).ok_or(Error::Truncated)?;
    if version != VERSION {
        return Err(Error::UnsupportedVersion(version));
    }
    let fixed = container.get(5..FIXED_HEADER_LEN).ok_or(Error::Truncated)?;
    let (len_bytes, count_bytes) = fixed.split_at(8);
    let original_len = u64::from_le_bytes(len_bytes.try_into().map_err(|_| Error::Truncated)?);
    let symbols = usize::from(u16::from_le_bytes(
        count_bytes.try_into().map_err(|_| Error::Truncated)?,
    ));
    if symbols > SYMBOLS {
        return Err(Error::InvalidTable("more than 256 symbols"));
    }

    let payload_start = header_len(symbols);
    let table = container
        .get(FIXED_HEADER_LEN..payload_start)
        .ok_or(Error::Truncated)?;
    let mut lengths = [0u8; SYMBOLS];
    let mut previous: Option<u8> = None;
    for &[symbol, len] in table.as_chunks::<2>().0 {
        if previous.is_some_and(|p| symbol <= p) {
            return Err(Error::InvalidTable("symbols are not in ascending order"));
        }
        if len == 0 {
            return Err(Error::InvalidTable("code length of zero"));
        }
        lengths[usize::from(symbol)] = len;
        previous = Some(symbol);
    }
    canonical::validate(&lengths)?;
    if symbols == 0 && original_len != 0 {
        return Err(Error::InvalidTable("data without any symbols"));
    }

    Ok(Header {
        original_len,
        lengths,
        payload_start,
    })
}

/// Restores the original bytes from a container.
///
/// Never panics on malformed input. The format has no checksum, so damage
/// inside the payload that still forms valid code words is not detected.
pub fn decode(container: &[u8]) -> Result<Vec<u8>, Error> {
    let header = read_header(container)?;
    let payload = container
        .get(header.payload_start..)
        .ok_or(Error::Truncated)?;

    // Every symbol costs at least one bit, which bounds the output size by
    // the payload size before anything is allocated.
    let payload_bits = (payload.len() as u64).saturating_mul(8);
    if header.original_len > payload_bits {
        return Err(Error::Truncated);
    }
    let original_len = usize::try_from(header.original_len).map_err(|_| Error::Truncated)?;

    let decoder = Decoder::new(&header.lengths)?;
    let mut reader = BitReader::new(payload);
    let mut out = Vec::with_capacity(original_len);
    for _ in 0..original_len {
        out.push(decoder.decode_symbol(&mut reader)?);
    }
    if !reader.only_padding_left() {
        return Err(Error::TrailingData);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_bytes_for_a_tiny_input() {
        // a:3 b:2 c:1 -> lengths a=1, b=2, c=2 -> codes a=0, b=10, c=11.
        // "abacab" -> 0 10 0 11 0 10 -> 0100_1101 0(0000000)
        let expected: Vec<u8> = [
            b"HUFF".as_slice(),
            &[1],
            &[6, 0, 0, 0, 0, 0, 0, 0],
            &[3, 0],
            &[b'a', 1, b'b', 2, b'c', 2],
            &[0b0100_1101, 0b0000_0000],
        ]
        .concat();
        assert_eq!(encode(b"abacab").unwrap(), expected);
        assert_eq!(decode(&expected).unwrap(), b"abacab");
    }

    #[test]
    fn empty_input_is_just_the_fixed_header() {
        let container = encode(b"").unwrap();
        assert_eq!(container.len(), FIXED_HEADER_LEN);
        assert_eq!(decode(&container).unwrap(), b"");
    }

    #[test]
    fn one_repeated_symbol_costs_one_bit_each() {
        let container = encode(&[b'x'; 16]).unwrap();
        assert_eq!(container.len(), header_len(1) + 2);
        assert_eq!(decode(&container).unwrap(), [b'x'; 16]);
    }

    #[test]
    fn bit_count_divisible_by_eight_adds_no_padding_byte() {
        // Two symbols, one bit each, eight symbols: exactly one payload byte.
        let container = encode(b"abababab").unwrap();
        assert_eq!(container.len(), header_len(2) + 1);
        assert_eq!(decode(&container).unwrap(), b"abababab");
    }

    #[test]
    fn header_can_be_read_on_its_own() {
        let container = encode(b"hello world").unwrap();
        let header = read_header(&container).unwrap();
        assert_eq!(header.original_len, 11);
        assert_eq!(header.payload_start, header_len(8));
        assert_eq!(header.lengths.iter().filter(|&&l| l > 0).count(), 8);
    }

    #[test]
    fn rejects_bad_magic_and_version() {
        let mut container = encode(b"hello").unwrap();
        container[0] = b'X';
        assert_eq!(decode(&container), Err(Error::BadMagic));
        assert_eq!(decode(b"PK\x03\x04 something else"), Err(Error::BadMagic));

        let mut container = encode(b"hello").unwrap();
        container[4] = 2;
        assert_eq!(decode(&container), Err(Error::UnsupportedVersion(2)));
    }

    #[test]
    fn rejects_every_truncation() {
        let container = encode(b"the quick brown fox jumps over the lazy dog").unwrap();
        for cut in 0..container.len() {
            assert_eq!(
                decode(&container[..cut]),
                Err(Error::Truncated),
                "cut {cut}"
            );
        }
    }

    #[test]
    fn rejects_trailing_bytes_and_dirty_padding() {
        let mut container = encode(b"abacab").unwrap();
        container.push(0);
        assert_eq!(decode(&container), Err(Error::TrailingData));

        let mut container = encode(b"abacab").unwrap();
        *container.last_mut().unwrap() |= 1;
        assert_eq!(decode(&container), Err(Error::TrailingData));

        let mut container = encode(b"").unwrap();
        container.push(0);
        assert_eq!(decode(&container), Err(Error::TrailingData));
    }

    #[test]
    fn rejects_invalid_tables() {
        let good = encode(b"abacab").unwrap();
        let table = FIXED_HEADER_LEN;
        let invalid = |edit: &dyn Fn(&mut Vec<u8>)| {
            let mut container = good.clone();
            edit(&mut container);
            matches!(decode(&container), Err(Error::InvalidTable(_)))
        };
        // Zero length.
        assert!(invalid(&|c| c[table + 1] = 0));
        // Longer than the maximum.
        assert!(invalid(&|c| c[table + 1] = 200));
        // Oversubscribed: lengths 1, 1, 2.
        assert!(invalid(&|c| c[table + 3] = 1));
        // Incomplete: lengths 1, 2, 3.
        assert!(invalid(&|c| c[table + 5] = 3));
        // Duplicate symbol.
        assert!(invalid(&|c| c[table + 2] = b'a'));
        // Descending symbols.
        assert!(invalid(&|c| c[table] = b'z'));
        // Symbol count above 256.
        assert!(invalid(
            &|c| c[13..15].copy_from_slice(&257u16.to_le_bytes())
        ));
    }

    #[test]
    fn rejects_length_without_symbols() {
        let mut container = encode(b"").unwrap();
        container[5] = 1;
        container.push(0);
        assert!(matches!(decode(&container), Err(Error::InvalidTable(_))));
    }

    #[test]
    fn huge_claimed_length_is_rejected_before_allocating() {
        let mut container = encode(&[b'x'; 16]).unwrap();
        container[5..13].copy_from_slice(&u64::MAX.to_le_bytes());
        assert_eq!(decode(&container), Err(Error::Truncated));
    }

    #[test]
    fn single_symbol_payload_with_a_one_bit_is_invalid() {
        let mut container = encode(&[b'x'; 64]).unwrap();
        for byte in &mut container[header_len(1)..] {
            *byte = 0xff;
        }
        assert_eq!(decode(&container), Err(Error::InvalidCode));
    }

    #[test]
    fn max_len_is_checked() {
        assert!(matches!(
            encode_with_max_len(b"abc", 49),
            Err(Error::InvalidMaxLen { .. })
        ));
        assert!(matches!(
            encode_with_max_len(b"abc", 1),
            Err(Error::InvalidMaxLen { .. })
        ));
        let container = encode_with_max_len(b"abc", 2).unwrap();
        assert_eq!(decode(&container).unwrap(), b"abc");
    }
}
