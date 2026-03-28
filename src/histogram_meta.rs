//! Histogram chunk layout encoding: spans, zero threshold, custom bounds.
//!
//! Matches Prometheus `tsdb/chunkenc/histogram_meta.go`.

use crate::bstream::{BStreamError, BStreamReader, BStreamWriter};
use crate::histogram_types::{Span, CUSTOM_BUCKETS_SCHEMA};
use crate::varbit::{put_varbit_int, put_varbit_uint, read_varbit_int, read_varbit_uint};

/// Decoded histogram layout: (schema, zero_threshold, positive_spans, negative_spans, custom_values).
type HistogramLayout = (i32, f64, Vec<Span>, Vec<Span>, Vec<f64>);

// ---------------------------------------------------------------------------
// Layout write/read
// ---------------------------------------------------------------------------

/// Write the histogram chunk layout to the bitstream.
///
/// This is written once at the start of a histogram chunk (before the first sample).
pub fn write_histogram_chunk_layout(
    w: &mut BStreamWriter,
    schema: i32,
    zero_threshold: f64,
    positive_spans: &[Span],
    negative_spans: &[Span],
    custom_values: &[f64],
) {
    put_zero_threshold(w, zero_threshold);
    put_varbit_int(w, schema as i64);
    put_chunk_layout_spans(w, positive_spans);
    put_chunk_layout_spans(w, negative_spans);
    if schema == CUSTOM_BUCKETS_SCHEMA {
        put_chunk_layout_custom_bounds(w, custom_values);
    }
}

/// Read the histogram chunk layout from the bitstream.
pub fn read_histogram_chunk_layout(r: &mut BStreamReader) -> Result<HistogramLayout, BStreamError> {
    let zero_threshold = read_zero_threshold(r)?;
    let schema = read_varbit_int(r)? as i32;
    let positive_spans = read_chunk_layout_spans(r)?;
    let negative_spans = read_chunk_layout_spans(r)?;
    let custom_values = if schema == CUSTOM_BUCKETS_SCHEMA {
        read_chunk_layout_custom_bounds(r)?
    } else {
        vec![]
    };
    Ok((
        schema,
        zero_threshold,
        positive_spans,
        negative_spans,
        custom_values,
    ))
}

// ---------------------------------------------------------------------------
// Zero threshold encoding
// ---------------------------------------------------------------------------

/// Write a zero threshold value.
///
/// - `0.0` -> byte `0x00`
/// - Power of 2 in `[2^-243, 2^10]` -> single byte `(exponent + 243)`
/// - Otherwise -> byte `0xFF` + 8 bytes raw float64
fn put_zero_threshold(w: &mut BStreamWriter, threshold: f64) {
    if threshold == 0.0 {
        w.write_byte(0x00);
        return;
    }

    // Use frexp to decompose: threshold = frac * 2^exp, where 0.5 <= |frac| < 1.0
    let (frac, exp) = libm::frexp(threshold);
    if frac == 0.5 && (-242..=11).contains(&exp) {
        w.write_byte((exp + 243) as u8);
        return;
    }

    w.write_byte(0xFF);
    w.write_bits(threshold.to_bits(), 64);
}

/// Read a zero threshold value.
fn read_zero_threshold(r: &mut BStreamReader) -> Result<f64, BStreamError> {
    let b = r.read_byte()?;
    match b {
        0 => Ok(0.0),
        0xFF => {
            let bits = r.read_bits(64)?;
            Ok(f64::from_bits(bits))
        }
        _ => Ok(libm::ldexp(0.5, b as i32 - 243)),
    }
}

// ---------------------------------------------------------------------------
// Span encoding
// ---------------------------------------------------------------------------

/// Write histogram spans. Length is written BEFORE offset for each span.
fn put_chunk_layout_spans(w: &mut BStreamWriter, spans: &[Span]) {
    put_varbit_uint(w, spans.len() as u64);
    for span in spans {
        put_varbit_uint(w, span.length as u64);
        put_varbit_int(w, span.offset as i64);
    }
}

/// Read histogram spans.
fn read_chunk_layout_spans(r: &mut BStreamReader) -> Result<Vec<Span>, BStreamError> {
    let num = read_varbit_uint(r)? as usize;
    let mut spans = Vec::with_capacity(num);
    for _ in 0..num {
        let length = read_varbit_uint(r)? as u32;
        let offset = read_varbit_int(r)? as i32;
        spans.push(Span { offset, length });
    }
    Ok(spans)
}

// ---------------------------------------------------------------------------
// Custom bounds encoding
// ---------------------------------------------------------------------------

/// Write custom bucket boundaries.
fn put_chunk_layout_custom_bounds(w: &mut BStreamWriter, bounds: &[f64]) {
    put_varbit_uint(w, bounds.len() as u64);
    for &b in bounds {
        put_custom_bound(w, b);
    }
}

/// Read custom bucket boundaries.
fn read_chunk_layout_custom_bounds(r: &mut BStreamReader) -> Result<Vec<f64>, BStreamError> {
    let num = read_varbit_uint(r)? as usize;
    let mut bounds = Vec::with_capacity(num);
    for _ in 0..num {
        bounds.push(read_custom_bound(r)?);
    }
    Ok(bounds)
}

/// Write a single custom bound value.
///
/// If `f * 1000` is a whole number in `[0, 33554430]`, encode compactly.
/// Otherwise, write a 0 bit + 64-bit raw float.
fn put_custom_bound(w: &mut BStreamWriter, f: f64) {
    let tf = f * 1000.0;
    if (0.0..=33554430.0).contains(&tf) && is_whole_when_multiplied(f) {
        put_varbit_uint(w, tf.round() as u64 + 1);
    } else {
        w.write_bit(false); // 0 bit prefix
        w.write_bits(f.to_bits(), 64);
    }
}

/// Read a single custom bound value.
fn read_custom_bound(r: &mut BStreamReader) -> Result<f64, BStreamError> {
    let v = read_varbit_uint(r)?;
    match v {
        0 => {
            let bits = r.read_bits(64)?;
            Ok(f64::from_bits(bits))
        }
        _ => Ok((v - 1) as f64 / 1000.0),
    }
}

/// Check if `f * 1000` is a whole number with exact float64 round-trip.
fn is_whole_when_multiplied(f: f64) -> bool {
    let rounded = (f * 1000.0).round() as u64;
    (rounded as f64) / 1000.0 == f
}

/// Count total number of buckets across all spans.
pub fn count_spans(spans: &[Span]) -> usize {
    spans.iter().map(|s| s.length as usize).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zero_threshold_zero() {
        let mut w = BStreamWriter::new();
        put_zero_threshold(&mut w, 0.0);
        let mut r = BStreamReader::new(w.bytes());
        assert_eq!(read_zero_threshold(&mut r).unwrap(), 0.0);
    }

    #[test]
    fn test_zero_threshold_power_of_two() {
        // Default Prometheus threshold: 2^-128
        let threshold = 2.938_735_877_055_719e-39_f64; // 2^-128
        let mut w = BStreamWriter::new();
        put_zero_threshold(&mut w, threshold);

        // Should encode as single byte
        assert_eq!(w.len(), 1);

        let mut r = BStreamReader::new(w.bytes());
        let decoded = read_zero_threshold(&mut r).unwrap();
        assert_eq!(decoded, threshold);
    }

    #[test]
    fn test_zero_threshold_arbitrary() {
        let threshold = 0.001;
        let mut w = BStreamWriter::new();
        put_zero_threshold(&mut w, threshold);

        // Should encode as 0xFF + 8 bytes
        assert_eq!(w.len(), 9);

        let mut r = BStreamReader::new(w.bytes());
        assert_eq!(read_zero_threshold(&mut r).unwrap(), threshold);
    }

    #[test]
    fn test_spans_roundtrip() {
        let spans = vec![
            Span {
                offset: 0,
                length: 2,
            },
            Span {
                offset: 3,
                length: 1,
            },
            Span {
                offset: -1,
                length: 5,
            },
        ];

        let mut w = BStreamWriter::new();
        put_chunk_layout_spans(&mut w, &spans);

        let mut r = BStreamReader::new(w.bytes());
        let decoded = read_chunk_layout_spans(&mut r).unwrap();
        assert_eq!(decoded, spans);
    }

    #[test]
    fn test_custom_bound_compact() {
        let mut w = BStreamWriter::new();
        put_custom_bound(&mut w, 1.5); // 1.5 * 1000 = 1500, whole number

        let mut r = BStreamReader::new(w.bytes());
        assert_eq!(read_custom_bound(&mut r).unwrap(), 1.5);
    }

    #[test]
    fn test_custom_bound_raw() {
        let val = std::f64::consts::PI;
        let mut w = BStreamWriter::new();
        put_custom_bound(&mut w, val);

        let mut r = BStreamReader::new(w.bytes());
        assert_eq!(read_custom_bound(&mut r).unwrap(), val);
    }

    #[test]
    fn test_layout_roundtrip() {
        let schema = 3;
        let zero_threshold = 2.938_735_877_055_719e-39_f64;
        let positive_spans = vec![
            Span {
                offset: 0,
                length: 2,
            },
            Span {
                offset: 1,
                length: 3,
            },
        ];
        let negative_spans = vec![Span {
            offset: 0,
            length: 1,
        }];
        let custom_values = vec![];

        let mut w = BStreamWriter::new();
        write_histogram_chunk_layout(
            &mut w,
            schema,
            zero_threshold,
            &positive_spans,
            &negative_spans,
            &custom_values,
        );

        let mut r = BStreamReader::new(w.bytes());
        let (s, zt, ps, ns, cv) = read_histogram_chunk_layout(&mut r).unwrap();
        assert_eq!(s, schema);
        assert_eq!(zt, zero_threshold);
        assert_eq!(ps, positive_spans);
        assert_eq!(ns, negative_spans);
        assert_eq!(cv, custom_values);
    }
}
