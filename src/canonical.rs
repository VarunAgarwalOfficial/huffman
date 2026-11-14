//! Canonical Huffman codes.
//!
//! Many trees share the same code lengths. The canonical code picks one fixed
//! member of that family: sort symbols by (length, byte value) and hand out
//! consecutive binary numbers, appending zeros whenever the length grows.
//! Encoder and decoder can then agree on the whole code from the lengths
//! alone, so the file only has to store one small number per symbol.

use crate::bits::BitReader;
use crate::error::Error;
use crate::freq::SYMBOLS;
use crate::tree::Lengths;

/// Longest code this crate writes or accepts.
///
/// A Huffman tree of depth d needs a total count of at least Fibonacci(d + 2),
/// so a 49-bit code would need an input of more than 20 GB with exactly the
/// wrong distribution. Above the limit the encoder falls back to
/// `tree::limited_code_lengths`.
pub const MAX_CODE_LEN: u8 = 48;

/// One code word: the lowest `len` bits of `bits`, most significant first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Code {
    pub bits: u64,
    pub len: u8,
}

impl Code {
    /// The code as a string of `0` and `1`.
    pub fn to_bit_string(&self) -> String {
        (0..self.len)
            .rev()
            .map(|i| if (self.bits >> i) & 1 == 1 { '1' } else { '0' })
            .collect()
    }
}

/// Checks that `lengths` describes a prefix code the decoder can use.
///
/// Accepted are: no symbols at all, a single symbol with a one-bit code, or
/// two or more symbols whose lengths satisfy the Kraft equality exactly
/// (the sum of 2^-length is 1), which is what every Huffman tree produces.
pub fn validate(lengths: &Lengths) -> Result<(), Error> {
    if lengths.iter().any(|&l| l > MAX_CODE_LEN) {
        return Err(Error::InvalidTable("code length exceeds 48 bits"));
    }
    let used: Vec<u8> = lengths.iter().copied().filter(|&l| l > 0).collect();
    match used.len() {
        0 => Ok(()),
        1 if used[0] == 1 => Ok(()),
        1 => Err(Error::InvalidTable(
            "a single symbol must have a one-bit code",
        )),
        _ => {
            // At most 256 terms of at most 2^47 each: no overflow.
            let kraft: u64 = used.iter().map(|&l| 1u64 << (MAX_CODE_LEN - l)).sum();
            match kraft.cmp(&(1u64 << MAX_CODE_LEN)) {
                std::cmp::Ordering::Equal => Ok(()),
                std::cmp::Ordering::Greater => Err(Error::InvalidTable(
                    "lengths are too short to form a prefix code",
                )),
                std::cmp::Ordering::Less => {
                    Err(Error::InvalidTable("lengths leave part of the code unused"))
                }
            }
        }
    }
}

/// Used symbols in canonical order: by length, then by byte value.
fn canonical_order(lengths: &Lengths) -> Vec<u8> {
    let mut symbols: Vec<u8> = (0..=255u8)
        .filter(|&s| lengths[usize::from(s)] > 0)
        .collect();
    symbols.sort_by_key(|&s| (lengths[usize::from(s)], s));
    symbols
}

/// The canonical code word for every byte value (`len == 0` if unused).
pub fn assign(lengths: &Lengths) -> Result<[Code; SYMBOLS], Error> {
    validate(lengths)?;
    let mut codes = [Code::default(); SYMBOLS];
    let mut next = 0u64;
    let mut prev_len = 0u8;
    for s in canonical_order(lengths) {
        let len = lengths[usize::from(s)];
        next <<= len - prev_len;
        codes[usize::from(s)] = Code { bits: next, len };
        next += 1;
        prev_len = len;
    }
    Ok(codes)
}

/// Turns bits back into symbols using only per-length bookkeeping.
///
/// All codes of one length are consecutive numbers, so it is enough to know,
/// for each length, the first code, how many codes there are, and where their
/// symbols start in the canonically ordered symbol list.
#[derive(Debug, Clone)]
pub struct Decoder {
    first: [u64; MAX_CODE_LEN as usize + 1],
    count: [u64; MAX_CODE_LEN as usize + 1],
    offset: [usize; MAX_CODE_LEN as usize + 1],
    symbols: Vec<u8>,
}

impl Decoder {
    pub fn new(lengths: &Lengths) -> Result<Self, Error> {
        validate(lengths)?;
        let top = MAX_CODE_LEN as usize + 1;
        let mut count = [0u64; MAX_CODE_LEN as usize + 1];
        for &l in lengths.iter().filter(|&&l| l > 0) {
            count[usize::from(l)] += 1;
        }
        let mut first = [0u64; MAX_CODE_LEN as usize + 1];
        let mut offset = [0usize; MAX_CODE_LEN as usize + 1];
        let mut code = 0u64;
        let mut seen = 0usize;
        for len in 1..top {
            code = (code + count[len - 1]) << 1;
            first[len] = code;
            offset[len] = seen;
            seen += count[len] as usize;
        }
        Ok(Self {
            first,
            count,
            offset,
            symbols: canonical_order(lengths),
        })
    }

    /// Number of symbols that have a code.
    pub fn symbol_count(&self) -> usize {
        self.symbols.len()
    }

    /// Reads one code word and returns its symbol.
    pub fn decode_symbol(&self, reader: &mut BitReader<'_>) -> Result<u8, Error> {
        let mut code = 0u64;
        for len in 1..=MAX_CODE_LEN as usize {
            let bit = reader.read_bit().ok_or(Error::Truncated)?;
            code = (code << 1) | u64::from(bit);
            if let Some(index) = code.checked_sub(self.first[len]) {
                if index < self.count[len] {
                    return Ok(self.symbols[self.offset[len] + index as usize]);
                }
            }
        }
        Err(Error::InvalidCode)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bits::BitWriter;

    fn lengths(pairs: &[(u8, u8)]) -> Lengths {
        let mut lengths = [0u8; SYMBOLS];
        for &(s, l) in pairs {
            lengths[usize::from(s)] = l;
        }
        lengths
    }

    fn is_prefix_free(codes: &[Code; SYMBOLS]) -> bool {
        let words: Vec<String> = codes
            .iter()
            .filter(|c| c.len > 0)
            .map(Code::to_bit_string)
            .collect();
        words.iter().enumerate().all(|(i, a)| {
            words
                .iter()
                .enumerate()
                .all(|(j, b)| i == j || !b.starts_with(a.as_str()))
        })
    }

    #[test]
    fn textbook_lengths_give_the_expected_codes() {
        let table = lengths(&[
            (b'a', 1),
            (b'b', 3),
            (b'c', 3),
            (b'd', 3),
            (b'e', 4),
            (b'f', 4),
        ]);
        let codes = assign(&table).unwrap();
        let word = |s: u8| codes[usize::from(s)].to_bit_string();
        assert_eq!(word(b'a'), "0");
        assert_eq!(word(b'b'), "100");
        assert_eq!(word(b'c'), "101");
        assert_eq!(word(b'd'), "110");
        assert_eq!(word(b'e'), "1110");
        assert_eq!(word(b'f'), "1111");
        assert!(is_prefix_free(&codes));
    }

    #[test]
    fn single_symbol_code_is_one_zero_bit() {
        let codes = assign(&lengths(&[(b'z', 1)])).unwrap();
        assert_eq!(codes[usize::from(b'z')], Code { bits: 0, len: 1 });
    }

    #[test]
    fn empty_table_is_valid() {
        let codes = assign(&[0; SYMBOLS]).unwrap();
        assert!(codes.iter().all(|c| c.len == 0));
        assert_eq!(Decoder::new(&[0; SYMBOLS]).unwrap().symbol_count(), 0);
    }

    #[test]
    fn full_alphabet_uses_the_byte_itself() {
        let codes = assign(&[8; SYMBOLS]).unwrap();
        for (s, c) in codes.iter().enumerate() {
            assert_eq!((c.bits, c.len), (s as u64, 8));
        }
    }

    #[test]
    fn bad_tables_are_rejected() {
        // Oversubscribed: three one-bit codes cannot exist.
        assert!(matches!(
            validate(&lengths(&[(1, 1), (2, 1), (3, 1)])),
            Err(Error::InvalidTable(_))
        ));
        // Incomplete: the pattern 11 would mean nothing.
        assert!(matches!(
            validate(&lengths(&[(1, 1), (2, 2)])),
            Err(Error::InvalidTable(_))
        ));
        // A lone symbol must use exactly one bit.
        assert!(matches!(
            validate(&lengths(&[(1, 2)])),
            Err(Error::InvalidTable(_))
        ));
        // Longer than the supported maximum.
        assert!(matches!(
            validate(&lengths(&[(1, 1), (2, 49)])),
            Err(Error::InvalidTable(_))
        ));
        assert!(matches!(
            Decoder::new(&lengths(&[(1, 255)])),
            Err(Error::InvalidTable(_))
        ));
    }

    #[test]
    fn longest_supported_codes_work() {
        // A chain: lengths 1, 2, ..., 47, 48, 48.
        let mut table = [0u8; SYMBOLS];
        for (s, l) in table.iter_mut().enumerate().take(48) {
            *l = s as u8 + 1;
        }
        table[48] = 48;
        let codes = assign(&table).unwrap();
        assert!(is_prefix_free(&codes));
        assert_eq!(codes[48].to_bit_string(), "1".repeat(48));

        let decoder = Decoder::new(&table).unwrap();
        let mut w = BitWriter::new();
        let message: Vec<u8> = vec![48, 0, 47, 20, 48, 1];
        for &s in &message {
            let c = codes[usize::from(s)];
            w.write(c.bits, c.len);
        }
        let bytes = w.finish();
        let mut r = BitReader::new(&bytes);
        for &s in &message {
            assert_eq!(decoder.decode_symbol(&mut r), Ok(s));
        }
    }

    #[test]
    fn decoder_inverts_assign() {
        let table = lengths(&[
            (10, 2),
            (20, 2),
            (30, 3),
            (40, 3),
            (50, 3),
            (60, 4),
            (70, 4),
        ]);
        let codes = assign(&table).unwrap();
        let decoder = Decoder::new(&table).unwrap();
        assert_eq!(decoder.symbol_count(), 7);
        for s in [10u8, 20, 30, 40, 50, 60, 70] {
            let c = codes[usize::from(s)];
            let mut w = BitWriter::new();
            w.write(c.bits, c.len);
            let bytes = w.finish();
            assert_eq!(decoder.decode_symbol(&mut BitReader::new(&bytes)), Ok(s));
        }
    }

    #[test]
    fn decoder_reports_truncation_and_unknown_patterns() {
        let one = Decoder::new(&lengths(&[(b'q', 1)])).unwrap();
        assert_eq!(
            one.decode_symbol(&mut BitReader::new(&[])),
            Err(Error::Truncated)
        );
        assert_eq!(one.decode_symbol(&mut BitReader::new(&[0x00])), Ok(b'q'));
        // The only code is "0"; a run of ones never matches.
        assert_eq!(
            one.decode_symbol(&mut BitReader::new(&[0xff; 8])),
            Err(Error::InvalidCode)
        );
        let none = Decoder::new(&[0; SYMBOLS]).unwrap();
        assert_eq!(
            none.decode_symbol(&mut BitReader::new(&[0x00; 8])),
            Err(Error::InvalidCode)
        );
    }
}
