//! Huffman coding over arbitrary bytes, with no dependencies.
//!
//! The pieces, in the order the encoder uses them:
//!
//! - [`freq`]: count bytes, compute Shannon entropy
//! - [`tree`]: the greedy merge that yields optimal code lengths
//! - [`canonical`]: turn lengths into canonical codes and decode them
//! - [`bits`]: bit-level writer and reader

pub mod bits;
pub mod canonical;
pub mod error;
pub mod freq;
pub mod tree;

pub use error::Error;
