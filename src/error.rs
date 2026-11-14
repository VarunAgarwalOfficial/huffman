//! The single error type used across the crate.

use std::fmt;
use std::io;

/// Everything that can go wrong while building a code or reading a container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The data does not start with the `HUFF` magic bytes.
    BadMagic,
    /// The container declares a format version this build does not read.
    UnsupportedVersion(u8),
    /// The data ends before the header, a block or the end marker is complete.
    Truncated,
    /// A block header holds a value the format does not allow.
    InvalidBlock { block: u64, why: &'static str },
    /// The code length table is not a usable prefix code.
    InvalidTable(&'static str),
    /// The payload contains a bit pattern that is not a code word.
    InvalidCode,
    /// A block decoded to bytes whose CRC-32 differs from the stored one.
    ChecksumMismatch {
        block: u64,
        stored: u32,
        computed: u32,
    },
    /// The end marker disagrees with the blocks that came before it.
    EndMismatch(&'static str),
    /// Bytes or non-zero padding bits follow where the data should end.
    TrailingData,
    /// The requested maximum code length cannot hold the alphabet.
    InvalidMaxLen { max_len: u8, symbols: usize },
    /// Reading the input or writing the output failed.
    Io(io::ErrorKind, String),
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e.kind(), e.to_string())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::BadMagic => write!(f, "not a huffman container (bad magic bytes)"),
            Error::UnsupportedVersion(1) => write!(
                f,
                "container version 1 has no checksums and is no longer read; \
                 decode it with huffman 0.1 and encode it again"
            ),
            Error::UnsupportedVersion(v) => write!(
                f,
                "unsupported container version {v} (this build reads version 2)"
            ),
            Error::Truncated => write!(f, "container is truncated (it ends before the end marker)"),
            Error::InvalidBlock { block, why } => write!(f, "block {block} is damaged: {why}"),
            Error::InvalidTable(why) => write!(f, "invalid code length table: {why}"),
            Error::InvalidCode => write!(f, "payload contains a bit pattern that is not a code"),
            Error::ChecksumMismatch {
                block,
                stored,
                computed,
            } => write!(
                f,
                "checksum mismatch in block {block}: stored {stored:08x}, computed {computed:08x}; \
                 the file is corrupt"
            ),
            Error::EndMismatch(why) => write!(f, "end marker does not match the data: {why}"),
            Error::TrailingData => write!(f, "unexpected data after the end of the container"),
            Error::InvalidMaxLen { max_len, symbols } => write!(
                f,
                "a maximum code length of {max_len} bits cannot encode {symbols} distinct symbols"
            ),
            Error::Io(_, message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for Error {}
