//! Huffman coding over arbitrary bytes, with no dependencies.
//!
//! The pieces, in the order the encoder uses them:
//!
//! - [`freq`]: count bytes, compute Shannon entropy
//! - [`tree`]: the greedy merge that yields optimal code lengths
//! - [`canonical`]: turn lengths into canonical codes and decode them
//! - [`bits`]: bit-level writer
//! - [`crc`]: CRC-32 for the integrity check
//! - [`container`]: the file format, [`encode_stream`] and [`decode_stream`]
//! - [`report`]: the text view of table, tree and summary used by `inspect`

pub mod bits;
pub mod canonical;
pub mod container;
pub mod crc;
pub mod error;
pub mod freq;
pub mod report;
pub mod tree;

pub use container::{decode, decode_stream, encode, encode_stream};
pub use error::Error;
