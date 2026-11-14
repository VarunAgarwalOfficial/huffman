//! Canonical Huffman codes.
//!
//! Many trees share the same code lengths. The canonical code picks one fixed
//! member of that family: sort symbols by (length, byte value) and hand out
//! consecutive binary numbers, appending zeros whenever the length grows.
//! Encoder and decoder can then agree on the whole code from the lengths
//! alone, so the file only has to store one small number per symbol.

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

/// Codes up to this many bits are decoded with one table lookup.
const FAST_BITS: u32 = 11;

/// Turns bits back into symbols.
///
/// Codes of up to 11 bits, which is nearly all of them, are found with one
/// lookup in a 2048-entry table indexed by the next 11 bits. Longer codes use
/// per-length bookkeeping: all codes of one length are consecutive numbers, so
/// it is enough to know, for each length, the first code, how many codes there
/// are, and where their symbols start in the canonically ordered symbol list.
#[derive(Debug, Clone)]
pub struct Decoder {
    /// `length << 8 | symbol` for every 11-bit window that starts with a short
    /// code, zero otherwise.
    fast: Vec<u16>,
    first: [u64; MAX_CODE_LEN as usize + 1],
    count: [u64; MAX_CODE_LEN as usize + 1],
    offset: [usize; MAX_CODE_LEN as usize + 1],
    symbols: Vec<u8>,
}

impl Decoder {
    pub fn new(lengths: &Lengths) -> Result<Self, Error> {
        let codes = assign(lengths)?;
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
        let mut fast = vec![0u16; 1 << FAST_BITS];
        for (symbol, code) in codes.iter().enumerate() {
            let len = u32::from(code.len);
            if len == 0 || len > FAST_BITS {
                continue;
            }
            let start = (code.bits << (FAST_BITS - len)) as usize;
            let entry = (u16::from(code.len) << 8) | symbol as u16;
            fast[start..start + (1 << (FAST_BITS - len))].fill(entry);
        }
        Ok(Self {
            fast,
            first,
            count,
            offset,
            symbols: canonical_order(lengths),
        })
    }

    /// The symbol and length of the code at the top of `window`, for codes
    /// longer than the fast table covers.
    fn long_code(&self, window: u64) -> Result<(u8, u32), Error> {
        for len in FAST_BITS as usize + 1..=MAX_CODE_LEN as usize {
            let code = window >> (64 - len);
            if let Some(index) = code.checked_sub(self.first[len]) {
                if index < self.count[len] {
                    return Ok((self.symbols[self.offset[len] + index as usize], len as u32));
                }
            }
        }
        Err(Error::InvalidCode)
    }

    /// Decodes exactly `count` symbols from `payload` and appends them to `out`.
    ///
    /// The payload must hold those code words, most significant bit first,
    /// followed by nothing but zero bits filling up the last byte.
    pub fn decode_into(
        &self,
        payload: &[u8],
        count: usize,
        out: &mut Vec<u8>,
    ) -> Result<(), Error> {
        let total_bits = payload.len() as u64 * 8;
        let mut used = 0u64;
        // The next bits to decode sit at the top of `window`; `have` of them are real.
        let mut window = 0u64;
        let mut have = 0u32;
        let mut pos = 0usize;
        for _ in 0..count {
            while have <= 56 && pos < payload.len() {
                window |= u64::from(payload[pos]) << (56 - have);
                pos += 1;
                have += 8;
            }
            let entry = self.fast[(window >> (64 - FAST_BITS)) as usize];
            let (symbol, len) = if entry != 0 {
                (entry as u8, u32::from(entry >> 8))
            } else {
                self.long_code(window)?
            };
            used += u64::from(len);
            if used > total_bits {
                return Err(Error::Truncated);
            }
            window <<= len;
            have -= len;
            out.push(symbol);
        }
        if total_bits - used >= 8 || window != 0 {
            return Err(Error::TrailingData);
        }
        Ok(())
    }
}
