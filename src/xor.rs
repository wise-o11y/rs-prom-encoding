//! XOR (Gorilla) value encoding for float64 fields.
//!
//! This is used by histogram chunks for encoding the `sum` field (integer histograms)
//! and all float fields (float histograms: count, zero_count, sum, buckets).
//!
//! The encoding matches Prometheus `tsdb/chunkenc.xorWrite` / `xorRead`.

use crate::bstream::{BStreamError, BStreamReader, BStreamWriter};

/// Sentinel value for `leading` indicating no previous XOR window exists.
pub const XOR_LEADING_SENTINEL: u8 = 0xFF;

/// Write XOR-encoded float64 delta.
///
/// `leading` and `trailing` track the previous XOR window state.
/// Initialize `leading` to [`XOR_LEADING_SENTINEL`] and `trailing` to 0
/// before the first call.
pub fn xor_write(
    w: &mut BStreamWriter,
    new_value: f64,
    current_value: f64,
    leading: &mut u8,
    trailing: &mut u8,
) {
    let delta = new_value.to_bits() ^ current_value.to_bits();

    if delta == 0 {
        w.write_bit(false); // 0 = values are identical
        return;
    }

    w.write_bit(true); // 1 = values differ

    let mut new_leading = delta.leading_zeros() as u8;
    let new_trailing = delta.trailing_zeros() as u8;

    // Clamp leading zeros to 5-bit max (0..31)
    if new_leading >= 32 {
        new_leading = 31;
    }

    if *leading != XOR_LEADING_SENTINEL && new_leading >= *leading && new_trailing >= *trailing {
        // Reuse previous leading/trailing window
        w.write_bit(false); // 0 = reuse window
        let sigbits = 64 - *leading - *trailing;
        w.write_bits(delta >> *trailing, sigbits);
    } else {
        // New window
        *leading = new_leading;
        *trailing = new_trailing;

        w.write_bit(true); // 1 = new window

        w.write_bits(new_leading as u64, 5);

        let sigbits = 64 - new_leading - new_trailing;
        // 0 in the 6-bit field encodes 64 significant bits
        w.write_bits(sigbits as u64, 6);
        w.write_bits(delta >> new_trailing, sigbits);
    }
}

/// Read XOR-encoded float64 delta.
///
/// Updates `value` in place. `leading` and `trailing` track the window state.
pub fn xor_read(
    r: &mut BStreamReader,
    value: &mut f64,
    leading: &mut u8,
    trailing: &mut u8,
) -> Result<(), BStreamError> {
    let bit = r.read_bit()?;
    if !bit {
        // Value unchanged
        return Ok(());
    }

    let bit = r.read_bit()?;
    if !bit {
        // Reuse previous window
        let mbits = 64 - *leading - *trailing;
        let bits = r.read_bits(mbits)?;
        let vbits = value.to_bits() ^ (bits << *trailing);
        *value = f64::from_bits(vbits);
    } else {
        // New window
        let new_leading = r.read_bits(5)? as u8;
        let mut mbits = r.read_bits(6)? as u8;
        if mbits == 0 {
            mbits = 64; // 0 encodes 64
        }
        let new_trailing = 64 - new_leading - mbits;

        let bits = r.read_bits(mbits)?;
        let vbits = value.to_bits() ^ (bits << new_trailing);
        *value = f64::from_bits(vbits);

        *leading = new_leading;
        *trailing = new_trailing;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xor_identical_values() {
        let mut w = BStreamWriter::new();
        let mut leading = XOR_LEADING_SENTINEL;
        let mut trailing = 0u8;

        xor_write(&mut w, 42.0, 42.0, &mut leading, &mut trailing);

        let mut r = BStreamReader::new(w.bytes());
        let mut value = 42.0;
        xor_read(&mut r, &mut value, &mut 0, &mut 0).unwrap();
        assert_eq!(value, 42.0);
    }

    #[test]
    fn test_xor_roundtrip_sequence() {
        let values = [1.0, 1.5, 2.0, 100.0, 100.001, 0.0, -1.0, f64::INFINITY];

        // Encode: XOR each value against its predecessor
        let mut w = BStreamWriter::new();
        let mut leading = XOR_LEADING_SENTINEL;
        let mut trailing = 0u8;

        for i in 1..values.len() {
            xor_write(
                &mut w,
                values[i],
                values[i - 1],
                &mut leading,
                &mut trailing,
            );
        }

        // Decode: xor_read updates dec_value in-place via XOR against current value
        let mut r = BStreamReader::new(w.bytes());
        let mut dec_leading = 0u8;
        let mut dec_trailing = 0u8;
        let mut dec_value = values[0]; // start from first value (written raw in real chunk)

        for &expected in &values[1..] {
            xor_read(&mut r, &mut dec_value, &mut dec_leading, &mut dec_trailing).unwrap();
            assert_eq!(
                dec_value.to_bits(),
                expected.to_bits(),
                "xor roundtrip failed: expected {expected}, got {dec_value}"
            );
        }
    }
}
