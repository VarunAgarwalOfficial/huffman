//! Bit-level output and input, most significant bit first.

/// Longest run of bits a single `BitWriter::write` call accepts.
pub const MAX_WRITE: u8 = 56;

/// Packs variable-length bit strings into bytes.
#[derive(Debug, Default)]
pub struct BitWriter {
    out: Vec<u8>,
    /// Pending bits, right-aligned. Always fewer than eight between calls.
    acc: u64,
    pending: u32,
}

impl BitWriter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends the lowest `len` bits of `bits`, highest of them first.
    ///
    /// `len` must not exceed [`MAX_WRITE`]; with at most seven bits pending
    /// this keeps the 64-bit accumulator from overflowing.
    pub fn write(&mut self, bits: u64, len: u8) {
        debug_assert!(len <= MAX_WRITE);
        let len = u32::from(len.min(MAX_WRITE));
        if len == 0 {
            return;
        }
        let mask = (1u64 << len) - 1;
        self.acc = (self.acc << len) | (bits & mask);
        self.pending += len;
        while self.pending >= 8 {
            self.pending -= 8;
            self.out.push((self.acc >> self.pending) as u8);
        }
        self.acc &= (1u64 << self.pending) - 1;
    }

    /// Number of bits written so far.
    pub fn bit_len(&self) -> u64 {
        self.out.len() as u64 * 8 + u64::from(self.pending)
    }

    /// Pads the last byte with zero bits and returns the bytes.
    pub fn finish(mut self) -> Vec<u8> {
        if self.pending > 0 {
            self.out.push((self.acc << (8 - self.pending)) as u8);
        }
        self.out
    }
}

/// Reads single bits from a byte slice.
#[derive(Debug, Clone)]
pub struct BitReader<'a> {
    data: &'a [u8],
    byte: usize,
    /// Bits already consumed from `data[byte]`, 0..=7.
    bit: u8,
}

impl<'a> BitReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            byte: 0,
            bit: 0,
        }
    }

    /// The next bit, or `None` once the data is used up.
    pub fn read_bit(&mut self) -> Option<bool> {
        let byte = *self.data.get(self.byte)?;
        let value = (byte >> (7 - self.bit)) & 1 == 1;
        self.bit += 1;
        if self.bit == 8 {
            self.bit = 0;
            self.byte += 1;
        }
        Some(value)
    }

    /// True if nothing is left except zero bits filling up the current byte.
    pub fn only_padding_left(&self) -> bool {
        if self.bit == 0 {
            return self.byte == self.data.len();
        }
        let rest = self.data[self.byte] & (0xff >> self.bit);
        rest == 0 && self.byte + 1 == self.data.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_msb_first_and_pads_with_zeros() {
        let mut w = BitWriter::new();
        w.write(0b101, 3);
        w.write(0b1, 1);
        assert_eq!(w.bit_len(), 4);
        assert_eq!(w.finish(), vec![0b1011_0000]);
    }

    #[test]
    fn exact_multiple_of_eight_adds_no_padding_byte() {
        let mut w = BitWriter::new();
        w.write(0xab, 8);
        w.write(0xcd, 8);
        assert_eq!(w.finish(), vec![0xab, 0xcd]);
    }

    #[test]
    fn nothing_written_gives_no_bytes() {
        let mut w = BitWriter::new();
        w.write(0xffff, 0);
        assert!(w.finish().is_empty());
    }

    #[test]
    fn ignores_bits_above_the_requested_length() {
        let mut w = BitWriter::new();
        w.write(u64::MAX, 2);
        w.write(0, 6);
        assert_eq!(w.finish(), vec![0b1100_0000]);
    }

    #[test]
    fn long_writes_with_pending_bits() {
        let mut w = BitWriter::new();
        w.write(0x7f, 7);
        w.write((1u64 << 56) - 1, 56);
        w.write(0, 1);
        assert_eq!(
            w.finish(),
            vec![0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xfe]
        );
    }

    #[test]
    fn reader_returns_what_the_writer_wrote() {
        let pieces: [(u64, u8); 6] = [
            (1, 1),
            (0b0110, 4),
            (0x1_2345_6789, 33),
            (0, 3),
            (0xabcd_ef01_2345, 48),
            (0b11, 2),
        ];
        let mut w = BitWriter::new();
        for &(bits, len) in &pieces {
            w.write(bits, len);
        }
        let bytes = w.finish();
        let mut r = BitReader::new(&bytes);
        for &(bits, len) in &pieces {
            let mut got = 0u64;
            for _ in 0..len {
                got = (got << 1) | u64::from(r.read_bit().unwrap());
            }
            assert_eq!(got, bits);
        }
        assert!(r.only_padding_left());
    }

    #[test]
    fn reader_stops_at_the_end() {
        let mut r = BitReader::new(&[0x80]);
        assert_eq!(r.read_bit(), Some(true));
        for _ in 0..7 {
            assert_eq!(r.read_bit(), Some(false));
        }
        assert_eq!(r.read_bit(), None);
        assert_eq!(r.read_bit(), None);
        assert!(BitReader::new(&[]).read_bit().is_none());
    }

    #[test]
    fn padding_check() {
        assert!(BitReader::new(&[]).only_padding_left());
        assert!(!BitReader::new(&[0]).only_padding_left());

        let mut r = BitReader::new(&[0b1010_0000]);
        r.read_bit();
        r.read_bit();
        assert!(!r.only_padding_left());
        r.read_bit();
        assert!(r.only_padding_left());

        let mut r = BitReader::new(&[0b1000_0000, 0]);
        r.read_bit();
        assert!(!r.only_padding_left());
    }
}
