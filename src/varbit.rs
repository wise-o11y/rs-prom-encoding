//! Variable-bit integer encoding matching Prometheus `varbit.go`.
//!
//! Also includes Go-compatible LEB128 varint/uvarint encoding.

use crate::bstream::{BStreamError, BStreamReader, BStreamWriter};

// ---------------------------------------------------------------------------
// bit_range — the asymmetric range check from Prometheus
// ---------------------------------------------------------------------------

/// Check if `x` fits in the asymmetric signed range for `nbits`.
///
/// Range: `[-(2^(nbits-1) - 1), 2^(nbits-1)]`
///
/// This is NOT standard two's complement. The positive bound is one larger.
#[inline]
fn bit_range(x: i64, nbits: u8) -> bool {
    let half = 1i64 << (nbits - 1);
    -(half - 1) <= x && x <= half
}

// ---------------------------------------------------------------------------
// Varbit signed encoding
// ---------------------------------------------------------------------------

/// Encoding buckets: (prefix_bits, prefix_value, value_bits)
const VARBIT_BUCKETS: &[(u8, u8, u8)] = &[
    // bit_range(val, 3): prefix "10", 3 value bits
    (2, 0b10, 3),
    // bit_range(val, 6): prefix "110", 6 value bits
    (3, 0b110, 6),
    // bit_range(val, 9): prefix "1110", 9 value bits
    (4, 0b1110, 9),
    // bit_range(val, 12): prefix "11110", 12 value bits
    (5, 0b11110, 12),
    // bit_range(val, 18): prefix "111110", 18 value bits
    (6, 0b111110, 18),
    // bit_range(val, 25): prefix "1111110", 25 value bits
    (7, 0b1111110, 25),
    // bit_range(val, 56): prefix "11111110", 56 value bits
    (8, 0b11111110, 56),
];

const VARBIT_NBITS: &[u8] = &[3, 6, 9, 12, 18, 25, 56];

/// Write a signed integer using variable-bit encoding.
pub fn put_varbit_int(w: &mut BStreamWriter, val: i64) {
    if val == 0 {
        w.write_bit(false); // single 0 bit
        return;
    }

    for (i, &nbits) in VARBIT_NBITS.iter().enumerate() {
        if bit_range(val, nbits) {
            let (prefix_bits, prefix_val, _) = VARBIT_BUCKETS[i];
            w.write_bits(prefix_val as u64, prefix_bits);
            w.write_bits(val as u64, nbits);
            return;
        }
    }

    // Full 64-bit fallback: prefix "11111111"
    w.write_bits(0xFF, 8);
    w.write_bits(val as u64, 64);
}

/// Read a signed integer using variable-bit encoding.
pub fn read_varbit_int(r: &mut BStreamReader) -> Result<i64, BStreamError> {
    // Read unary prefix: count 1-bits until a 0-bit
    let mut d: u8 = 0;
    for _ in 0..8 {
        d <<= 1;
        let bit = r.read_bit()?;
        if !bit {
            // Found the terminating 0 bit
            break;
        }
        d |= 1;
    }

    let sz: u8 = match d {
        0b0 => return Ok(0),
        0b10 => 3,
        0b110 => 6,
        0b1110 => 9,
        0b11110 => 12,
        0b111110 => 18,
        0b1111110 => 25,
        0b11111110 => 56,
        0b11111111 => {
            let bits = r.read_bits(64)?;
            return Ok(bits as i64);
        }
        _ => return Err(BStreamError::Eof),
    };

    let bits = r.read_bits(sz)?;
    // Sign extension: if bits > 2^(sz-1), subtract 2^sz
    if bits > (1u64 << (sz - 1)) {
        Ok(bits.wrapping_sub(1u64 << sz) as i64)
    } else {
        Ok(bits as i64)
    }
}

// ---------------------------------------------------------------------------
// Varbit unsigned encoding
// ---------------------------------------------------------------------------

const VARBIT_UINT_LIMITS: &[(u8, u64)] = &[
    (3, 7),
    (6, 63),
    (9, 511),
    (12, 4095),
    (18, 262143),
    (25, 33554431),
    (56, 72057594037927935),
];

/// Write an unsigned integer using variable-bit encoding.
pub fn put_varbit_uint(w: &mut BStreamWriter, val: u64) {
    if val == 0 {
        w.write_bit(false);
        return;
    }

    for (i, &(nbits, limit)) in VARBIT_UINT_LIMITS.iter().enumerate() {
        if val <= limit {
            let (prefix_bits, prefix_val, _) = VARBIT_BUCKETS[i];
            w.write_bits(prefix_val as u64, prefix_bits);
            w.write_bits(val, nbits);
            return;
        }
    }

    // Full 64-bit fallback
    w.write_bits(0xFF, 8);
    w.write_bits(val, 64);
}

/// Read an unsigned integer using variable-bit encoding.
pub fn read_varbit_uint(r: &mut BStreamReader) -> Result<u64, BStreamError> {
    let mut d: u8 = 0;
    for _ in 0..8 {
        d <<= 1;
        let bit = r.read_bit()?;
        if !bit {
            break;
        }
        d |= 1;
    }

    let sz: u8 = match d {
        0b0 => return Ok(0),
        0b10 => 3,
        0b110 => 6,
        0b1110 => 9,
        0b11110 => 12,
        0b111110 => 18,
        0b1111110 => 25,
        0b11111110 => 56,
        0b11111111 => return r.read_bits(64),
        _ => return Err(BStreamError::Eof),
    };

    r.read_bits(sz)
}

// ---------------------------------------------------------------------------
// Go-compatible LEB128 varint / uvarint
// ---------------------------------------------------------------------------

/// Write an unsigned integer in Go's `encoding/binary` uvarint format.
///
/// This writes whole bytes into the bstream (byte-aligned LEB128).
pub fn put_uvarint(w: &mut BStreamWriter, mut val: u64) {
    while val >= 0x80 {
        w.write_byte((val as u8) | 0x80);
        val >>= 7;
    }
    w.write_byte(val as u8);
}

/// Write a signed integer in Go's `encoding/binary` varint format.
///
/// Uses zigzag encoding: `(v << 1) ^ (v >> 63)`, then uvarint.
pub fn put_varint(w: &mut BStreamWriter, val: i64) {
    let uv = ((val << 1) ^ (val >> 63)) as u64;
    put_uvarint(w, uv);
}

/// Read a uvarint from the reader (Go-compatible LEB128).
pub fn read_uvarint(r: &mut BStreamReader) -> Result<u64, BStreamError> {
    let mut x: u64 = 0;
    let mut s: u32 = 0;
    for _ in 0..10 {
        let b = r.read_byte()?;
        if b < 0x80 {
            return Ok(x | (b as u64) << s);
        }
        x |= ((b & 0x7f) as u64) << s;
        s += 7;
    }
    Err(BStreamError::Eof) // overflow
}

/// Read a varint from the reader (Go-compatible zigzag + LEB128).
pub fn read_varint(r: &mut BStreamReader) -> Result<i64, BStreamError> {
    let ux = read_uvarint(r)?;
    let x = (ux >> 1) as i64;
    if ux & 1 != 0 {
        Ok(!x)
    } else {
        Ok(x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_varbit_int_zero() {
        let mut w = BStreamWriter::new();
        put_varbit_int(&mut w, 0);
        let mut r = BStreamReader::new(w.bytes());
        assert_eq!(read_varbit_int(&mut r).unwrap(), 0);
    }

    #[test]
    fn test_varbit_int_roundtrip_boundaries() {
        let values: &[i64] = &[
            i64::MIN,
            -36028797018963968,
            -36028797018963967,
            -16777216,
            -16777215,
            -131072,
            -131071,
            -2048,
            -2047,
            -256,
            -255,
            -32,
            -31,
            -4,
            -3,
            0,
            4,
            5,
            32,
            33,
            256,
            257,
            2048,
            2049,
            131072,
            131073,
            16777216,
            16777217,
            36028797018963968,
            36028797018963969,
            i64::MAX,
        ];

        for &val in values {
            let mut w = BStreamWriter::new();
            put_varbit_int(&mut w, val);
            let mut r = BStreamReader::new(w.bytes());
            let decoded = read_varbit_int(&mut r).unwrap();
            assert_eq!(decoded, val, "varbit_int roundtrip failed for {val}");
        }
    }

    #[test]
    fn test_varbit_uint_roundtrip_boundaries() {
        let values: &[u64] = &[
            0,
            1,
            7,
            8,
            63,
            64,
            511,
            512,
            4095,
            4096,
            262143,
            262144,
            33554431,
            33554432,
            72057594037927935,
            72057594037927936,
            u64::MAX,
        ];

        for &val in values {
            let mut w = BStreamWriter::new();
            put_varbit_uint(&mut w, val);
            let mut r = BStreamReader::new(w.bytes());
            let decoded = read_varbit_uint(&mut r).unwrap();
            assert_eq!(decoded, val, "varbit_uint roundtrip failed for {val}");
        }
    }

    #[test]
    fn test_varint_roundtrip() {
        let values: &[i64] = &[0, 1, -1, 127, -128, 16383, -16384, i64::MIN, i64::MAX];
        for &val in values {
            let mut w = BStreamWriter::new();
            put_varint(&mut w, val);
            let mut r = BStreamReader::new(w.bytes());
            let decoded = read_varint(&mut r).unwrap();
            assert_eq!(decoded, val, "varint roundtrip failed for {val}");
        }
    }

    #[test]
    fn test_uvarint_roundtrip() {
        let values: &[u64] = &[0, 1, 127, 128, 16383, 16384, u64::MAX];
        for &val in values {
            let mut w = BStreamWriter::new();
            put_uvarint(&mut w, val);
            let mut r = BStreamReader::new(w.bytes());
            let decoded = read_uvarint(&mut r).unwrap();
            assert_eq!(decoded, val, "uvarint roundtrip failed for {val}");
        }
    }
}
