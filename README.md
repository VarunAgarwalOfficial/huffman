# huffman

A Huffman coder for arbitrary files, written in Rust with no dependencies.
It compresses, decompresses, and can show the code it built for a file: the
table of symbols, the tree, and how close the result is to the entropy bound.

## The idea

This started as a data structures course project at university. The lecture
proved that a greedy rule (always merge the two rarest symbols) gives the
best possible prefix code, and the project turned that proof into a program
that actually made a file smaller. It was written twice: first in C, with an
encoder, a decoder and a small menu, and a year later in Python, as a
shorter encoder that wrote the code table to a JSON file next to the bits.
This version keeps the idea and rebuilds the rest: one library, one command
line tool, any bytes rather than only text, and a decoder that checks what it
reads.

## How it works

**Greedy merge.** Count how often each byte value occurs. Put every used
byte into a min-heap as a one-leaf tree weighted by its count. Take out the
two lightest trees, join them under a new node whose weight is their sum, put
that back, and repeat until one tree is left. The depth of a leaf is the
length of its code. Ties are broken by a fixed rule (leaves before merged
nodes, then lower byte value, then older node), so the same input always
gives the same code.

**Why it is optimal.** By an exchange argument: in any optimal tree the two
rarest symbols can be swapped into the two deepest sibling positions without
increasing the cost, so some optimal tree has them as siblings. Treating that
pair as one symbol leaves a smaller problem of the same kind, and induction
does the rest. The tests check the textbook frequencies
45, 13, 12, 16, 9, 5, which must cost 224 bits.

**Canonical codes.** Many trees share the same code lengths. Sort the symbols
by (length, byte value) and hand out consecutive binary numbers, appending
zeros when the length grows. Both sides can rebuild this code from the
lengths alone, so the file stores one length per used symbol instead of a
tree, and the decoder needs only a small table per code length.

**Edge cases.** An empty input has no symbols and no payload. An input with
one distinct byte gets a one-bit code, so every symbol still costs a bit.
Codes longer than 48 bits would need more than 20 GB of input with
Fibonacci-like counts; if that ever happens the counts are halved and the
tree rebuilt until it fits, which stays a valid prefix code but is no longer
strictly optimal.

**File format.** All integers are little endian.

| Offset  | Size | Field                                               |
|---------|------|-----------------------------------------------------|
| 0       | 4    | magic bytes `HUFF`                                  |
| 4       | 1    | format version, currently 1                         |
| 5       | 8    | original length in bytes                            |
| 13      | 2    | number of used symbols n, 0 to 256                  |
| 15      | 2n   | n pairs of (byte value, code length), ascending     |
| 15 + 2n | rest | code words, most significant bit first, zero padded |

The decoder rejects a wrong magic or version, a table that is not a complete
prefix code, data that ends early, and anything after the last symbol. There
is no checksum, so a flipped bit inside the payload can decode to different
bytes without an error.

**Entropy bound.** For symbol probabilities p, the Shannon entropy
H = -sum(p * log2(p)) is a lower bound on the average code length of any
prefix code, and with two or more distinct symbols a Huffman code is within
one bit of it: H <= average < H + 1. (A single distinct byte has H = 0 but
still costs one bit per symbol.) `inspect` prints both numbers. The gap comes from code
lengths being whole bits, and it is largest when one symbol dominates.

## Run

Needs a Rust toolchain (1.88 or newer).

```
cargo build --release

target/release/huffman encode samples/lighthouse.txt /tmp/lighthouse.huf
target/release/huffman decode /tmp/lighthouse.huf /tmp/lighthouse.txt
cmp samples/lighthouse.txt /tmp/lighthouse.txt

target/release/huffman inspect samples/lighthouse.txt
```

Use `-` as a file name for standard input or output:

```
printf 'abracadabra' | target/release/huffman inspect -
```

```
symbol byte      count   share bits  code
------ ---- ---------- ------- ----  ----
'a'    0x61          5  45.45%    1  0
'b'    0x62          2  18.18%    3  100
'r'    0x72          2  18.18%    3  111
'c'    0x63          1   9.09%    3  101
'd'    0x64          1   9.09%    3  110

tree (branch labels are code bits, numbers are weights)
(11)
+-0- 'a' 5
`-1- (6)
     +-0- (3)
     |    +-0- 'b' 2
     |    `-1- 'c' 1
     `-1- (3)
          +-0- 'd' 1
          `-1- 'r' 2

input bytes     11
output bytes    28 (25 header and table, 3 payload)
ratio           2.545 (output / input, +154.5% size change)
distinct bytes  5
entropy         2.0404 bits/symbol
average code    2.0909 bits/symbol (+0.0505 above entropy)
```

Small inputs grow, because the header and table cost more than the payload
saves. With more than 24 distinct bytes the tree is listed by depth instead
of drawn. `inspect` takes an uncompressed file. Exit status is 0 on success,
1 for a file that cannot be read, written or decoded, 2 for a wrong command
line.

## Test

```
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

A few unit tests sit next to the modules in `src/`; `tests/roundtrip.rs`
covers the library end to end and `tests/cli.rs` runs the built binary.

## Layout

```
src/freq.rs       byte counts and entropy
src/tree.rs       greedy merge on a binary heap, code lengths, length limit
src/canonical.rs  canonical code assignment and decoding
src/bits.rs       bit writer and reader
src/container.rs  file format, encode and decode
src/report.rs     text for inspect: table, tree, summary
src/main.rs       command line
```

## Credit

Based on a data structures course project by Varun Agarwal and Shreya Atul
Sharma: C version 2020, Python rewrite 2021.
