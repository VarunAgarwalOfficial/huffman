//! A plain C-ABI interface for the demo page: no wasm-bindgen, no imports.
//!
//! The page writes the text into memory obtained from [`huff_alloc`], calls
//! [`huff_analyze`], reads the JSON it returns and gives both buffers back with
//! [`huff_free`]. Everything shown on the page comes from the same library code
//! the command line tool runs.

use std::fmt::Write as _;

use huffman::{container, freq, report, tree};

/// Reserves `len` bytes and returns their address.
#[no_mangle]
pub extern "C" fn huff_alloc(len: usize) -> *mut u8 {
    let mut buffer = Vec::<u8>::with_capacity(len.max(1));
    let pointer = buffer.as_mut_ptr();
    std::mem::forget(buffer);
    pointer
}

/// Releases a buffer of `len` bytes that came from [`huff_alloc`] or [`huff_analyze`].
///
/// # Safety
/// `pointer` and `len` must describe exactly one such buffer, not yet freed.
#[no_mangle]
pub unsafe extern "C" fn huff_free(pointer: *mut u8, len: usize) {
    drop(Vec::from_raw_parts(pointer, 0, len.max(1)));
}

/// Builds the Huffman code for the `len` bytes at `pointer`.
///
/// Returns a buffer that starts with its JSON length as a little-endian u32,
/// followed by that many bytes of JSON. Free it with `huff_free(p, 4 + length)`.
///
/// # Safety
/// `pointer` must be valid for reading `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn huff_analyze(pointer: *const u8, len: usize) -> *mut u8 {
    let data = std::slice::from_raw_parts(pointer, len);
    let json = describe(data);
    let out = huff_alloc(4 + json.len());
    std::ptr::copy_nonoverlapping((json.len() as u32).to_le_bytes().as_ptr(), out, 4);
    std::ptr::copy_nonoverlapping(json.as_ptr(), out.add(4), json.len());
    out
}

/// The code for `data` as JSON: symbols in ascending byte order (their index
/// is the leaf id the merges refer to), the greedy merges in order, and the
/// totals. Text longer than one block is described by its first block.
fn describe(data: &[u8]) -> String {
    let data = &data[..data.len().min(container::BLOCK_SIZE)];
    let counts = freq::count(data);
    let merges = tree::merges(&counts);
    let Ok(summary) = report::analyze(data) else {
        return "{\"error\":true}".to_string();
    };
    let mut rows = summary.rows.clone();
    rows.sort_by_key(|row| row.symbol);

    let mut json = String::from("{\"symbols\":[");
    for (i, row) in rows.iter().enumerate() {
        let comma = if i == 0 { "" } else { "," };
        let _ = write!(
            json,
            "{comma}{{\"byte\":{},\"count\":{},\"code\":\"{}\"}}",
            row.symbol,
            row.count,
            row.code.to_bit_string()
        );
    }
    json.push_str("],\"merges\":[");
    for (i, merge) in merges.iter().enumerate() {
        let comma = if i == 0 { "" } else { "," };
        let _ = write!(
            json,
            "{comma}{{\"a\":{},\"b\":{},\"weight\":{}}}",
            merge.a, merge.b, merge.weight
        );
    }
    let payload_bits: u64 = rows.iter().map(|r| r.count * u64::from(r.code.len)).sum();
    let _ = write!(
        json,
        "],\"bytes\":{},\"payloadBits\":{payload_bits},\"containerBytes\":{},\
         \"overheadBytes\":{},\"entropy\":{:.6},\"average\":{:.6}}}",
        summary.input_bytes,
        summary.output_bytes,
        summary.overhead_bytes,
        summary.entropy,
        summary.average_len
    );
    json
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_abracadabra() {
        let json = describe(b"abracadabra");
        // a:5 is the only one-bit code; four merges join five symbols.
        assert!(json.starts_with("{\"symbols\":[{\"byte\":97,\"count\":5,\"code\":\"0\"}"));
        assert_eq!(json.matches("\"weight\"").count(), 4);
        assert!(json.contains("\"weight\":11}]"));
        assert!(json.contains("\"bytes\":11,\"payloadBits\":23,"));
        // Empty and single-symbol inputs have no merges.
        assert!(describe(b"").contains("\"symbols\":[],\"merges\":[],\"bytes\":0"));
        assert!(describe(b"zzz").contains("\"code\":\"0\"}],\"merges\":[]"));
    }
}
