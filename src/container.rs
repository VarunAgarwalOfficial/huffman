//! The file format: a header, independent blocks, and an end marker.
//!
//! All integers are little endian.
//!
//! | Part       | Size | Field                                               |
//! |------------|------|-----------------------------------------------------|
//! | header     | 4    | magic bytes `HUFF`                                  |
//! |            | 1    | format version, currently 2                         |
//! | data block | 1    | block type `0x01`                                   |
//! |            | 4    | original length of the block, 1 to 1,048,576 bytes  |
//! |            | 4    | CRC-32 of the block's original bytes                |
//! |            | 2    | number of used symbols n, 1 to 256                  |
//! |            | 2n   | n pairs of (byte value, code length), ascending     |
//! |            | 4    | payload length p in bytes                           |
//! |            | p    | code words, MSB first, last byte zero padded        |
//! | end marker | 1    | block type `0x00`                                   |
//! |            | 8    | total original length                               |
//! |            | 4    | number of data blocks                               |
//! |            | 4    | CRC-32 over the stored block CRCs, in order         |
//!
//! Each block carries its own code, so encoder and decoder hold one block at
//! a time and work on pipes. A block is checked against its CRC before any of
//! its bytes are written. The end marker catches a file that was cut between
//! blocks, and blocks that were dropped, repeated or reordered.

use std::io::{self, Read, Write};

use crate::bits::BitWriter;
use crate::canonical::{self, Decoder, MAX_CODE_LEN};
use crate::crc::{crc32, Crc32};
use crate::error::Error;
use crate::freq::{self, Counts, SYMBOLS};
use crate::tree::{self, Lengths};

/// First four bytes of every container.
pub const MAGIC: [u8; 4] = *b"HUFF";
/// Format version written by this crate.
pub const VERSION: u8 = 2;
/// Most original bytes one block holds.
pub const BLOCK_SIZE: usize = 1 << 20;
/// Bytes in the file header.
pub const HEADER_LEN: usize = 5;
/// Bytes in the end marker.
pub const END_LEN: usize = 17;
/// Bytes a data block spends on everything except its table and payload.
pub const BLOCK_FIXED_LEN: usize = 15;

const BLOCK_DATA: u8 = 1;
const BLOCK_END: u8 = 0;

/// What one run of the encoder or decoder processed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stats {
    /// Bytes of original data.
    pub original_bytes: u64,
    /// Bytes of container.
    pub container_bytes: u64,
    /// Container bytes that are not code words: headers, tables, checksums.
    pub overhead_bytes: u64,
    /// Number of data blocks.
    pub blocks: u64,
}

/// Reads until `buffer` is full or the input ends; returns the bytes read.
fn read_full(input: &mut impl Read, buffer: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buffer.len() {
        match input.read(&mut buffer[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(filled)
}

/// Like `read_exact`, but an early end of input is [`Error::Truncated`].
fn read_exact(input: &mut impl Read, buffer: &mut [u8]) -> Result<(), Error> {
    if read_full(input, buffer)? < buffer.len() {
        return Err(Error::Truncated);
    }
    Ok(())
}

fn read_array<const N: usize>(input: &mut impl Read) -> Result<[u8; N], Error> {
    let mut bytes = [0u8; N];
    read_exact(input, &mut bytes)?;
    Ok(bytes)
}

/// Size of the data block the encoder writes for a block with these counts,
/// as (total bytes, bytes that are not code words), without encoding anything.
pub fn block_len(counts: &Counts) -> Result<(u64, u64), Error> {
    let lengths = tree::limited_code_lengths(counts, MAX_CODE_LEN)?;
    let overhead = (BLOCK_FIXED_LEN + 2 * freq::active(counts)) as u64;
    Ok((
        overhead + tree::total_bits(counts, &lengths).div_ceil(8),
        overhead,
    ))
}

/// Compresses everything `input` yields into a container written to `output`.
///
/// Holds one block of input and its encoded form at a time.
pub fn encode_stream(input: impl Read, output: impl Write) -> Result<Stats, Error> {
    encode_stream_with_max_len(input, output, MAX_CODE_LEN)
}

/// Like [`encode_stream`], but with no code longer than `max_len` bits (1..=48).
pub fn encode_stream_with_max_len(
    mut input: impl Read,
    mut output: impl Write,
    max_len: u8,
) -> Result<Stats, Error> {
    if max_len > MAX_CODE_LEN {
        return Err(Error::InvalidMaxLen {
            max_len,
            symbols: 0,
        });
    }
    output.write_all(&MAGIC)?;
    output.write_all(&[VERSION])?;
    let mut stats = Stats {
        container_bytes: HEADER_LEN as u64,
        overhead_bytes: HEADER_LEN as u64,
        ..Stats::default()
    };

    let mut block = vec![0u8; BLOCK_SIZE];
    let mut payload = Vec::new();
    let mut crc_of_crcs = Crc32::new();
    loop {
        let filled = read_full(&mut input, &mut block)?;
        if filled == 0 {
            break;
        }
        let data = &block[..filled];
        let counts = freq::count(data);
        let lengths = tree::limited_code_lengths(&counts, max_len)?;
        let codes = canonical::assign(&lengths)?;
        let mut writer = BitWriter::with_buffer(payload);
        for &byte in data {
            let code = codes[usize::from(byte)];
            writer.write(code.bits, code.len);
        }
        payload = writer.finish();

        let crc = crc32(data);
        crc_of_crcs.update(&crc.to_le_bytes());
        let symbols = freq::active(&counts);
        let mut head = Vec::with_capacity(BLOCK_FIXED_LEN + 2 * symbols);
        head.push(BLOCK_DATA);
        head.extend_from_slice(&(filled as u32).to_le_bytes());
        head.extend_from_slice(&crc.to_le_bytes());
        head.extend_from_slice(&(symbols as u16).to_le_bytes());
        for (symbol, &len) in lengths.iter().enumerate() {
            if len > 0 {
                head.push(symbol as u8);
                head.push(len);
            }
        }
        head.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        output.write_all(&head)?;
        output.write_all(&payload)?;

        stats.original_bytes += filled as u64;
        stats.container_bytes += (head.len() + payload.len()) as u64;
        stats.overhead_bytes += head.len() as u64;
        stats.blocks += 1;
    }

    output.write_all(&[BLOCK_END])?;
    output.write_all(&stats.original_bytes.to_le_bytes())?;
    output.write_all(&(stats.blocks as u32).to_le_bytes())?;
    output.write_all(&crc_of_crcs.value().to_le_bytes())?;
    output.flush()?;
    stats.container_bytes += END_LEN as u64;
    stats.overhead_bytes += END_LEN as u64;
    Ok(stats)
}

/// Reads the table of a data block: `symbols` pairs of (byte value, length).
fn read_table(input: &mut impl Read, symbols: usize) -> Result<Lengths, Error> {
    if symbols == 0 {
        return Err(Error::InvalidTable("a block without any symbols"));
    }
    if symbols > SYMBOLS {
        return Err(Error::InvalidTable("more than 256 symbols"));
    }
    let mut table = [0u8; 2 * SYMBOLS];
    let table = &mut table[..2 * symbols];
    read_exact(input, table)?;
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
    Ok(lengths)
}

/// Restores the original bytes from a container read from `input`.
///
/// Holds one block at a time. A block reaches `output` only after its
/// checksum matched, so on an error the output holds a correct prefix of the
/// original and nothing else. Never panics on malformed input.
pub fn decode_stream(mut input: impl Read, mut output: impl Write) -> Result<Stats, Error> {
    let magic: [u8; 4] = read_array(&mut input)?;
    if magic != MAGIC {
        return Err(Error::BadMagic);
    }
    let [version] = read_array(&mut input)?;
    if version != VERSION {
        return Err(Error::UnsupportedVersion(version));
    }
    let mut stats = Stats {
        container_bytes: HEADER_LEN as u64,
        overhead_bytes: HEADER_LEN as u64,
        ..Stats::default()
    };

    let mut payload = Vec::new();
    let mut data = Vec::with_capacity(BLOCK_SIZE);
    let mut crc_of_crcs = Crc32::new();
    loop {
        let block = stats.blocks + 1;
        let invalid = |why| Error::InvalidBlock { block, why };
        let [kind] = read_array(&mut input)?;
        match kind {
            BLOCK_END => break,
            BLOCK_DATA => {}
            _ => return Err(invalid("unknown block type")),
        }
        let original_len = u32::from_le_bytes(read_array(&mut input)?) as usize;
        let stored = u32::from_le_bytes(read_array(&mut input)?);
        let symbols = usize::from(u16::from_le_bytes(read_array(&mut input)?));
        if original_len == 0 || original_len > BLOCK_SIZE {
            return Err(invalid("original length is outside 1 to 1,048,576"));
        }
        let lengths = read_table(&mut input, symbols)?;
        let payload_len = u32::from_le_bytes(read_array(&mut input)?) as usize;
        // Every symbol costs between 1 and 48 bits, which bounds the payload
        // before anything is allocated.
        let max_payload = original_len * usize::from(MAX_CODE_LEN) / 8 + 1;
        if payload_len > max_payload || payload_len * 8 < original_len {
            return Err(invalid("payload length does not fit the original length"));
        }
        payload.resize(payload_len, 0);
        read_exact(&mut input, &mut payload)?;

        data.clear();
        let decoder = Decoder::new(&lengths)?;
        decoder
            .decode_into(&payload, original_len, &mut data)
            .map_err(|e| match e {
                Error::InvalidCode => invalid("payload contains a bit pattern that is not a code"),
                Error::Truncated => invalid("payload ends before the last symbol"),
                Error::TrailingData => invalid("payload continues after the last symbol"),
                other => other,
            })?;
        let computed = crc32(&data);
        if computed != stored {
            return Err(Error::ChecksumMismatch {
                block,
                stored,
                computed,
            });
        }
        crc_of_crcs.update(&stored.to_le_bytes());
        output.write_all(&data)?;

        stats.original_bytes += original_len as u64;
        stats.container_bytes += (BLOCK_FIXED_LEN + 2 * symbols + payload_len) as u64;
        stats.overhead_bytes += (BLOCK_FIXED_LEN + 2 * symbols) as u64;
        stats.blocks += 1;
    }

    let total = u64::from_le_bytes(read_array(&mut input)?);
    let blocks = u32::from_le_bytes(read_array(&mut input)?);
    let crc = u32::from_le_bytes(read_array(&mut input)?);
    if total != stats.original_bytes {
        return Err(Error::EndMismatch("total length differs"));
    }
    if u64::from(blocks) != stats.blocks {
        return Err(Error::EndMismatch("number of blocks differs"));
    }
    if crc != crc_of_crcs.value() {
        return Err(Error::EndMismatch(
            "checksum over the block checksums differs",
        ));
    }
    let mut extra = [0u8; 1];
    if read_full(&mut input, &mut extra)? != 0 {
        return Err(Error::TrailingData);
    }
    output.flush()?;
    stats.container_bytes += END_LEN as u64;
    stats.overhead_bytes += END_LEN as u64;
    Ok(stats)
}

/// Compresses `data` into a container held in memory.
pub fn encode(data: &[u8]) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    encode_stream(data, &mut out)?;
    Ok(out)
}

/// Restores the original bytes from a container held in memory.
pub fn decode(container: &[u8]) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    decode_stream(container, &mut out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_bytes_for_a_tiny_input() {
        // a:3 b:2 c:1 -> lengths a=1, b=2, c=2 -> codes a=0, b=10, c=11.
        // "abacab" -> 0 10 0 11 0 10 -> 0100_1101 0(0000000)
        let crc = crc32(b"abacab").to_le_bytes();
        let expected: Vec<u8> = [
            b"HUFF".as_slice(),
            &[2],
            &[1],
            &[6, 0, 0, 0],
            &crc,
            &[3, 0],
            &[b'a', 1, b'b', 2, b'c', 2],
            &[2, 0, 0, 0],
            &[0b0100_1101, 0b0000_0000],
            &[0],
            &[6, 0, 0, 0, 0, 0, 0, 0],
            &[1, 0, 0, 0],
            &crc32(&crc).to_le_bytes(),
        ]
        .concat();
        assert_eq!(encode(b"abacab").unwrap(), expected);
        assert_eq!(decode(&expected).unwrap(), b"abacab");
    }

    #[test]
    fn rejects_invalid_tables() {
        let good = encode(b"abacab").unwrap();
        let table = HEADER_LEN + 11;
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
        // Symbol count above 256.
        assert!(invalid(
            &|c| c[table - 2..table].copy_from_slice(&257u16.to_le_bytes())
        ));
    }
}
