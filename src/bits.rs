//! Bit-level output, most significant bit first.

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

    /// Starts a writer that reuses `buffer` (cleared first) for its output.
    pub fn with_buffer(mut buffer: Vec<u8>) -> Self {
        buffer.clear();
        Self {
            out: buffer,
            acc: 0,
            pending: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(w.bit_len(), 91);
        let bytes = w.finish();
        assert_eq!(bytes.len(), 12);
        // Read the pieces back by walking the bytes bit by bit.
        let mut position = 0usize;
        for &(bits, len) in &pieces {
            let mut got = 0u64;
            for _ in 0..len {
                let bit = (bytes[position / 8] >> (7 - position % 8)) & 1;
                got = (got << 1) | u64::from(bit);
                position += 1;
            }
            assert_eq!(got, bits);
        }
        // The five padding bits are zero.
        assert_eq!(bytes[11] & 0x1f, 0);
    }
}
