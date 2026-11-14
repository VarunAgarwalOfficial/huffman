//! CRC-32 as used by gzip, zlib and PNG (polynomial 0xEDB88320, reflected).

/// Eight tables: `TABLES[k][b]` is the CRC of byte `b` followed by `k` zero
/// bytes, which lets `update` fold in eight input bytes per step
/// ("slicing by 8") instead of one.
const fn make_tables() -> [[u32; 256]; 8] {
    let mut tables = [[0u32; 256]; 8];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 == 1 {
                0xedb8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        tables[0][i] = c;
        i += 1;
    }
    let mut t = 1;
    while t < 8 {
        let mut i = 0;
        while i < 256 {
            let prev = tables[t - 1][i];
            tables[t][i] = tables[0][(prev & 0xff) as usize] ^ (prev >> 8);
            i += 1;
        }
        t += 1;
    }
    tables
}

static TABLES: [[u32; 256]; 8] = make_tables();

/// A running CRC-32. Feed bytes with [`Crc32::update`], read with [`Crc32::value`].
#[derive(Debug, Clone, Copy)]
pub struct Crc32(u32);

impl Default for Crc32 {
    fn default() -> Self {
        Self(!0)
    }
}

impl Crc32 {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update(&mut self, data: &[u8]) {
        let mut c = self.0;
        let (chunks, rest) = data.as_chunks::<8>();
        for chunk in chunks {
            let low = c ^ u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            c = TABLES[7][(low & 0xff) as usize]
                ^ TABLES[6][((low >> 8) & 0xff) as usize]
                ^ TABLES[5][((low >> 16) & 0xff) as usize]
                ^ TABLES[4][(low >> 24) as usize]
                ^ TABLES[3][usize::from(chunk[4])]
                ^ TABLES[2][usize::from(chunk[5])]
                ^ TABLES[1][usize::from(chunk[6])]
                ^ TABLES[0][usize::from(chunk[7])];
        }
        for &byte in rest {
            c = TABLES[0][usize::from((c as u8) ^ byte)] ^ (c >> 8);
        }
        self.0 = c;
    }

    pub fn value(&self) -> u32 {
        !self.0
    }
}

/// CRC-32 of one slice.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = Crc32::new();
    crc.update(data);
    crc.value()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_standard_check_value() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        assert_eq!(crc32(b""), 0);
        // Feeding in pieces gives the same value as feeding at once.
        let mut crc = Crc32::new();
        crc.update(b"1234");
        crc.update(b"56789");
        assert_eq!(crc.value(), 0xcbf4_3926);
        // Longer than eight bytes, so the sliced path runs: value from `gzip -l -v`.
        assert_eq!(
            crc32(b"The quick brown fox jumps over the lazy dog"),
            0x414f_a339
        );
    }
}
