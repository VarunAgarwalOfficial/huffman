//! The single error type used across the crate.

use std::fmt;

/// Everything that can go wrong while building a code or reading a container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The data does not start with the `HUFF` magic bytes.
    BadMagic,
    /// The container declares a format version this build does not read.
    UnsupportedVersion(u8),
    /// The data ends before the header, the table or the payload is complete.
    Truncated,
    /// The code length table is not a usable prefix code.
    InvalidTable(&'static str),
    /// The payload contains a bit pattern that is not a code word.
    InvalidCode,
    /// Bytes or non-zero padding bits follow the last encoded symbol.
    TrailingData,
    /// The requested maximum code length cannot hold the alphabet.
    InvalidMaxLen { max_len: u8, symbols: usize },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::BadMagic => write!(f, "not a huffman container (bad magic bytes)"),
            Error::UnsupportedVersion(v) => write!(f, "unsupported container version {v}"),
            Error::Truncated => write!(f, "container is truncated"),
            Error::InvalidTable(why) => write!(f, "invalid code length table: {why}"),
            Error::InvalidCode => write!(f, "payload contains a bit pattern that is not a code"),
            Error::TrailingData => write!(f, "unexpected data after the last symbol"),
            Error::InvalidMaxLen { max_len, symbols } => write!(
                f,
                "a maximum code length of {max_len} bits cannot encode {symbols} distinct symbols"
            ),
        }
    }
}

impl std::error::Error for Error {}
