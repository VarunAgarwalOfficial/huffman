//! The "see it work" view: code table, tree drawing and summary as text.

use std::fmt::Write as _;

use crate::canonical::{self, Code};
use crate::container;
use crate::error::Error;
use crate::freq;
use crate::tree;

/// Above this many symbols the tree is shown grouped by depth instead of drawn.
pub const MAX_DRAWN_LEAVES: usize = 24;

/// One used byte value with its count and code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub symbol: u8,
    pub count: u64,
    pub code: Code,
}

/// Everything `inspect` shows about one input.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    /// Used symbols, most frequent first (ties by byte value).
    pub rows: Vec<Row>,
    pub input_bytes: u64,
    /// Size of the real encoded container.
    pub output_bytes: u64,
    /// Header plus code length table, part of `output_bytes`.
    pub header_bytes: u64,
    /// Shannon entropy in bits per symbol.
    pub entropy: f64,
    /// Average code length in bits per symbol.
    pub average_len: f64,
}

/// Builds the code for `data` and gathers the numbers worth showing.
pub fn analyze(data: &[u8]) -> Result<Report, Error> {
    let counts = freq::count(data);
    let lengths = tree::limited_code_lengths(&counts, canonical::MAX_CODE_LEN)?;
    let codes = canonical::assign(&lengths)?;
    let encoded = container::encode(data)?;

    let mut rows: Vec<Row> = (0..=255u8)
        .filter(|&s| counts[usize::from(s)] > 0)
        .map(|s| Row {
            symbol: s,
            count: counts[usize::from(s)],
            code: codes[usize::from(s)],
        })
        .collect();
    rows.sort_by(|a, b| b.count.cmp(&a.count).then(a.symbol.cmp(&b.symbol)));

    let average_len = if data.is_empty() {
        0.0
    } else {
        tree::total_bits(&counts, &lengths) as f64 / data.len() as f64
    };
    Ok(Report {
        input_bytes: data.len() as u64,
        output_bytes: encoded.len() as u64,
        header_bytes: container::header_len(rows.len()) as u64,
        entropy: freq::entropy(&counts),
        average_len,
        rows,
    })
}

/// A short readable name for a byte value.
pub fn symbol_label(symbol: u8) -> String {
    match symbol {
        b'\n' => "\\n".to_string(),
        b'\r' => "\\r".to_string(),
        b'\t' => "\\t".to_string(),
        0x20..=0x7e => format!("'{}'", char::from(symbol)),
        _ => format!("0x{symbol:02x}"),
    }
}

/// The full report as plain text.
pub fn render(report: &Report) -> String {
    let mut out = String::new();
    if report.rows.is_empty() {
        out.push_str("The input is empty: there are no symbols and no tree.\n\n");
    } else {
        out.push_str(&render_table(report));
        out.push('\n');
        out.push_str(&render_tree(&report.rows));
        out.push('\n');
    }
    out.push_str(&render_summary(report));
    out
}

/// Symbol, count, share, code length and code, most frequent first.
pub fn render_table(report: &Report) -> String {
    let code_width = report
        .rows
        .iter()
        .map(|r| usize::from(r.code.len))
        .max()
        .unwrap_or(0)
        .max(4);
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{:<6} {:>4} {:>10} {:>7} {:>4}  code",
        "symbol", "byte", "count", "share", "bits"
    );
    let _ = writeln!(
        out,
        "{:-<6} {:->4} {:->10} {:->7} {:->4}  {:-<code_width$}",
        "", "", "", "", "", ""
    );
    for row in &report.rows {
        let share = 100.0 * row.count as f64 / report.input_bytes as f64;
        let _ = writeln!(
            out,
            "{:<6} {:>4} {:>10} {:>6.2}% {:>4}  {}",
            symbol_label(row.symbol),
            format!("0x{:02x}", row.symbol),
            row.count,
            share,
            row.code.len,
            row.code.to_bit_string()
        );
    }
    out
}

/// The code tree: drawn branch by branch for small alphabets, grouped by
/// depth for large ones. Numbers are subtree weights (summed counts).
///
/// The drawing is derived from the canonical codes, so following the branch
/// labels from the root spells exactly the code shown in the table. It has the
/// same leaf depths as the tree the greedy merge built, but not necessarily
/// the same left-to-right arrangement.
pub fn render_tree(rows: &[Row]) -> String {
    let mut sorted: Vec<&Row> = rows.iter().collect();
    sorted.sort_by_key(|r| (r.code.len, r.symbol));
    let mut out = String::new();
    if sorted.is_empty() {
        return out;
    }
    if sorted.len() > MAX_DRAWN_LEAVES {
        let _ = writeln!(
            out,
            "tree ({} leaves, too many to draw; symbols grouped by depth)",
            sorted.len()
        );
        let mut start = 0;
        while start < sorted.len() {
            let len = sorted[start].code.len;
            let end = start
                + sorted[start..]
                    .iter()
                    .take_while(|r| r.code.len == len)
                    .count();
            let _ = writeln!(out, "  depth {len:>2}: {:>3} symbols", end - start);
            for chunk in sorted[start..end].chunks(12) {
                let labels: Vec<String> = chunk.iter().map(|r| symbol_label(r.symbol)).collect();
                let _ = writeln!(out, "            {}", labels.join(" "));
            }
            start = end;
        }
        return out;
    }

    out.push_str("tree (branch labels are code bits, numbers are weights)\n");
    let total: u64 = sorted.iter().map(|r| r.count).sum();
    let _ = writeln!(out, "({total})");
    draw_children(&sorted, 0, "", &mut out);
    out
}

/// Draws the two subtrees below a node whose leaves are `rows`, all sharing
/// the first `depth` code bits. Canonical order keeps each subtree contiguous.
fn draw_children(rows: &[&Row], depth: u8, prefix: &str, out: &mut String) {
    let bit_at = |r: &Row| (r.code.bits >> (r.code.len - depth - 1)) & 1;
    let split = rows.partition_point(|r| bit_at(r) == 0);
    let (zeros, ones) = rows.split_at(split);
    for (bit, side, last) in [(0, zeros, ones.is_empty()), (1, ones, true)] {
        if side.is_empty() {
            continue;
        }
        let (branch, next_prefix) = if last {
            ("`-", format!("{prefix}     "))
        } else {
            ("+-", format!("{prefix}|    "))
        };
        if side.len() == 1 && side[0].code.len == depth + 1 {
            let _ = writeln!(
                out,
                "{prefix}{branch}{bit}- {} {}",
                symbol_label(side[0].symbol),
                side[0].count
            );
        } else {
            let weight: u64 = side.iter().map(|r| r.count).sum();
            let _ = writeln!(out, "{prefix}{branch}{bit}- ({weight})");
            draw_children(side, depth + 1, &next_prefix, out);
        }
    }
}

/// Sizes, ratio and how close the code gets to the entropy bound.
pub fn render_summary(report: &Report) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "input bytes     {}", report.input_bytes);
    let _ = writeln!(
        out,
        "output bytes    {} ({} header and table, {} payload)",
        report.output_bytes,
        report.header_bytes,
        report.output_bytes - report.header_bytes
    );
    if report.input_bytes == 0 {
        let _ = writeln!(out, "ratio           n/a (empty input)");
    } else {
        let ratio = report.output_bytes as f64 / report.input_bytes as f64;
        let _ = writeln!(
            out,
            "ratio           {:.3} (output / input, {:+.1}% size change)",
            ratio,
            (ratio - 1.0) * 100.0
        );
    }
    let _ = writeln!(out, "distinct bytes  {}", report.rows.len());
    let _ = writeln!(out, "entropy         {:.4} bits/symbol", report.entropy);
    let _ = writeln!(
        out,
        "average code    {:.4} bits/symbol ({:+.4} above entropy)",
        report.average_len,
        report.average_len - report.entropy
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_drawing_for_the_textbook_example() {
        let mut data = Vec::new();
        for (symbol, count) in [
            (b'a', 45),
            (b'b', 13),
            (b'c', 12),
            (b'd', 16),
            (b'e', 9),
            (b'f', 5),
        ] {
            data.extend(std::iter::repeat_n(symbol, count));
        }
        let report = analyze(&data).unwrap();
        let expected = "\
tree (branch labels are code bits, numbers are weights)
(100)
+-0- 'a' 45
`-1- (55)
     +-0- (25)
     |    +-0- 'b' 13
     |    `-1- 'c' 12
     `-1- (30)
          +-0- 'd' 16
          `-1- (14)
               +-0- 'e' 9
               `-1- 'f' 5
";
        assert_eq!(render_tree(&report.rows), expected);
        assert!(report.average_len >= report.entropy);
    }
}
