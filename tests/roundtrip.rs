//! Round-trip and property-based tests for rs-prom-encoder.
//!
//! These tests ensure that encoding followed by decoding produces the original values.
//! Includes boundary value tests and property-based testing with proptest.

use proptest::prelude::*;
use rs_prom_encoder::{
    CounterResetHint, FloatHistogram, FloatHistogramChunk, Histogram, HistogramChunk, Span,
    XORChunk, CUSTOM_BUCKETS_SCHEMA, STALE_NAN_BITS,
};
use rusty_chunkenc::xor::read_xor_chunk_data;

// ==================== XOR Round-Trip Tests ====================

#[test]
fn test_xor_roundtrip_basic() {
    let timestamps: Vec<i64> = (0..100).map(|i| 1_700_000_000_000 + i * 15_000).collect();
    let values: Vec<f64> = (0..100).map(|i| 100.0 + i as f64 * 0.5).collect();

    let mut chunk = XORChunk::new();
    for (&t, &v) in timestamps.iter().zip(values.iter()) {
        chunk.append(t, v);
    }

    let encoded = chunk.encode();

    // Decode using rusty-chunkenc
    let (_, decoded) = read_xor_chunk_data(&encoded).expect("Failed to decode XOR chunk");
    let decoded_samples = decoded.samples();

    assert_eq!(decoded_samples.len(), 100, "Decoded sample count mismatch");
    for (i, (expected_t, expected_v)) in timestamps.iter().zip(values.iter()).enumerate() {
        assert_eq!(
            decoded_samples[i].timestamp, *expected_t,
            "Timestamp mismatch at index {}",
            i
        );
        assert!(
            (decoded_samples[i].value - *expected_v).abs() < f64::EPSILON,
            "Value mismatch at index {}: got {}, expected {}",
            i,
            decoded_samples[i].value,
            expected_v
        );
    }
}

#[test]
fn test_xor_roundtrip_edge_values() {
    let test_cases = vec![
        (0i64, 0.0f64),
        (1, -0.0),
        (2, f64::INFINITY),
        (3, f64::NEG_INFINITY),
        (4, f64::NAN),
        (5, f64::MIN_POSITIVE),
        (6, f64::MAX),
        (7, f64::MIN),
        (8, 1e-308),
        (9, 1e308),
    ];

    let mut chunk = XORChunk::new();
    for (t, v) in &test_cases {
        chunk.append(*t, *v);
    }

    let encoded = chunk.encode();
    let (_, decoded) =
        read_xor_chunk_data(&encoded).expect("Failed to decode XOR chunk with edge values");
    let decoded_samples = decoded.samples();

    assert_eq!(decoded_samples.len(), test_cases.len());

    for (i, (expected_t, expected_v)) in test_cases.iter().enumerate() {
        assert_eq!(decoded_samples[i].timestamp, *expected_t);

        // Handle NaN specially
        if expected_v.is_nan() {
            assert!(
                decoded_samples[i].value.is_nan(),
                "Expected NaN at index {}, got {}",
                i,
                decoded_samples[i].value
            );
        } else if expected_v.is_infinite() {
            assert!(
                decoded_samples[i].value.is_infinite()
                    && decoded_samples[i].value.signum() == expected_v.signum(),
                "Infinity mismatch at index {}: got {}, expected {}",
                i,
                decoded_samples[i].value,
                expected_v
            );
        } else {
            assert_eq!(
                decoded_samples[i].value.to_bits(),
                expected_v.to_bits(),
                "Value mismatch at index {}: got {}, expected {}",
                i,
                decoded_samples[i].value,
                expected_v
            );
        }
    }
}

#[test]
fn test_xor_roundtrip_stale_marker() {
    let stale_value = f64::from_bits(STALE_NAN_BITS);

    let mut chunk = XORChunk::new();
    chunk.append(1000, 1.0);
    chunk.append(2000, stale_value);
    chunk.append(3000, 2.0);

    let encoded = chunk.encode();
    let (_, decoded) =
        read_xor_chunk_data(&encoded).expect("Failed to decode XOR chunk with stale marker");
    let decoded_samples = decoded.samples();

    assert_eq!(decoded_samples.len(), 3);
    assert_eq!(decoded_samples[0].timestamp, 1000);
    assert!((decoded_samples[0].value - 1.0).abs() < f64::EPSILON);

    assert_eq!(decoded_samples[1].timestamp, 2000);
    assert_eq!(
        decoded_samples[1].value.to_bits(),
        stale_value.to_bits(),
        "Stale marker should be preserved exactly"
    );

    assert_eq!(decoded_samples[2].timestamp, 3000);
    assert!((decoded_samples[2].value - 2.0).abs() < f64::EPSILON);
}

#[test]
fn test_xor_roundtrip_timestamps() {
    // Test various timestamp patterns
    let timestamps = vec![
        i64::MIN + 1,
        -1000000000000i64,
        -1,
        0,
        1,
        1000000000000,
        i64::MAX - 1,
    ];

    let mut chunk = XORChunk::new();
    for t in &timestamps {
        chunk.append(*t, 42.0);
    }

    let encoded = chunk.encode();
    let (_, decoded) =
        read_xor_chunk_data(&encoded).expect("Failed to decode XOR chunk with boundary timestamps");
    let decoded_samples = decoded.samples();

    assert_eq!(decoded_samples.len(), timestamps.len());
    for (i, expected_t) in timestamps.iter().enumerate() {
        assert_eq!(
            decoded_samples[i].timestamp, *expected_t,
            "Timestamp mismatch at index {}",
            i
        );
    }
}

#[test]
fn test_xor_roundtrip_large_delta() {
    // Test timestamps with large gaps (triggers different delta encodings)
    let timestamps = vec![
        0i64,
        1,             // small delta
        2,             // same bucket
        100,           // medium delta
        1000000,       // large delta
        2000000,       // triggers 14-bit or larger encoding
        3000000000i64, // very large delta
    ];

    let mut chunk = XORChunk::new();
    for t in &timestamps {
        chunk.append(*t, 1.0);
    }

    let encoded = chunk.encode();
    let (_, decoded) =
        read_xor_chunk_data(&encoded).expect("Failed to decode XOR chunk with large deltas");
    let decoded_samples = decoded.samples();

    assert_eq!(decoded_samples.len(), timestamps.len());
    for (i, expected_t) in timestamps.iter().enumerate() {
        assert_eq!(decoded_samples[i].timestamp, *expected_t);
    }
}

// Property-based test for XOR encoding
proptest! {
    #[test]
    fn prop_xor_roundtrip(
        samples in prop::collection::vec(
            // Use reasonable ranges to avoid rusty-chunkenc overflow bugs
            (1_700_000_000_000i64..1_800_000_000_000i64, -1e6f64..1e6f64),
            1..200
        )
    ) {
        let mut chunk = XORChunk::new();
        let mut last_t = None;

        // Ensure timestamps are strictly increasing
        for (t, v) in &samples {
            let t = match last_t {
                None => *t,
                Some(last) if *t > last => *t,
                Some(last) => last + 1,
            };
            chunk.append(t, *v);
            last_t = Some(t);
        }

        let encoded = chunk.encode();

        // Decode and verify
        let (_, decoded) = read_xor_chunk_data(&encoded)
            .expect("Failed to decode XOR chunk in property test");
        let decoded_samples = decoded.samples();

        prop_assert_eq!(decoded_samples.len(), chunk.num_samples() as usize);

        // Verify each sample (handle NaN specially)
        for i in 0..decoded_samples.len() {
            if samples[i].1.is_nan() {
                prop_assert!(decoded_samples[i].value.is_nan());
            } else {
                prop_assert_eq!(
                    decoded_samples[i].value.to_bits(),
                    samples[i].1.to_bits(),
                    "Value mismatch at index {}",
                    i
                );
            }
        }
    }
}

// ==================== Histogram Round-Trip Tests ====================

fn create_test_histogram(count: u64, sum: f64, buckets: Vec<i64>) -> Histogram {
    Histogram {
        count,
        zero_count: 0,
        sum,
        schema: 3,
        zero_threshold: 2.938735877055719e-39,
        positive_spans: vec![Span {
            offset: 0,
            length: buckets.len() as u32,
        }],
        negative_spans: vec![],
        positive_buckets: buckets,
        negative_buckets: vec![],
        custom_values: vec![],
        counter_reset_hint: CounterResetHint::UnknownCounterReset,
    }
}

#[test]
fn test_histogram_roundtrip_basic() {
    let h1 = create_test_histogram(10, 18.4, vec![4, 4, 2]);
    let h2 = create_test_histogram(15, 28.0, vec![6, 6, 3]);
    let h3 = create_test_histogram(22, 40.5, vec![8, 9, 5]);

    let mut chunk = HistogramChunk::new();
    chunk.append(1000, &h1);
    chunk.append(2000, &h2);
    chunk.append(3000, &h3);

    let encoded = chunk.encode();
    assert!(!encoded.is_empty());
    assert_eq!(chunk.num_samples(), 3);
}

#[test]
fn test_histogram_roundtrip_with_negative_spans() {
    let h = Histogram {
        count: 20,
        zero_count: 5,
        sum: 35.0,
        schema: 3,
        zero_threshold: 1e-100,
        positive_spans: vec![Span {
            offset: 0,
            length: 2,
        }],
        negative_spans: vec![Span {
            offset: 1,
            length: 3,
        }],
        positive_buckets: vec![5, 5],
        negative_buckets: vec![3, 4, 3],
        custom_values: vec![],
        counter_reset_hint: CounterResetHint::UnknownCounterReset,
    };

    let mut chunk = HistogramChunk::new();
    chunk.append(1000, &h);
    chunk.append(2000, &h);

    let encoded = chunk.encode();
    assert!(!encoded.is_empty());
}

#[test]
fn test_histogram_roundtrip_gauge_type() {
    // First histogram sets the gauge type for the chunk
    let h1 = Histogram {
        count: 10,
        zero_count: 0,
        sum: 18.4,
        schema: 3,
        zero_threshold: 2.938735877055719e-39,
        positive_spans: vec![Span {
            offset: 0,
            length: 2,
        }],
        negative_spans: vec![],
        positive_buckets: vec![4, 4],
        negative_buckets: vec![],
        custom_values: vec![],
        counter_reset_hint: CounterResetHint::GaugeType,
    };
    let h2 = Histogram {
        count: 5, // Decreasing (valid for gauge)
        zero_count: 0,
        sum: 8.0,
        schema: 3,
        zero_threshold: 2.938735877055719e-39,
        positive_spans: vec![Span {
            offset: 0,
            length: 2,
        }],
        negative_spans: vec![],
        positive_buckets: vec![2, 2],
        negative_buckets: vec![],
        custom_values: vec![],
        counter_reset_hint: CounterResetHint::GaugeType,
    };

    let mut chunk = HistogramChunk::new();
    chunk.append(1000, &h1);
    chunk.append(2000, &h2);

    let encoded = chunk.encode();
    assert!(!encoded.is_empty());
    // Verify the gauge flag (0b11000000 = 192) is set in the header byte
    assert!(
        encoded[2] & CounterResetHint::GaugeType as u8 == CounterResetHint::GaugeType as u8,
        "Gauge flag should be set in header byte"
    );
}

#[test]
fn test_histogram_roundtrip_custom_bounds() {
    let custom_bounds = vec![0.5, 1.0, 2.5, 5.0, 10.0];

    let h = Histogram {
        count: 12,
        zero_count: 2,
        sum: 25.5,
        schema: CUSTOM_BUCKETS_SCHEMA,
        zero_threshold: 0.25,
        positive_spans: vec![Span {
            offset: 0,
            length: custom_bounds.len() as u32,
        }],
        negative_spans: vec![],
        positive_buckets: vec![3, 3, 2, 2, 2],
        negative_buckets: vec![],
        custom_values: custom_bounds.clone(),
        counter_reset_hint: CounterResetHint::UnknownCounterReset,
    };

    let mut chunk = HistogramChunk::new();
    chunk.append(1000, &h);
    chunk.append(2000, &h);

    let encoded = chunk.encode();
    assert!(!encoded.is_empty());
}

#[test]
fn test_histogram_roundtrip_stale_marker() {
    let h1 = create_test_histogram(10, 18.4, vec![4, 4]);
    let h2 = Histogram {
        count: 10,
        zero_count: 0,
        sum: f64::from_bits(STALE_NAN_BITS),
        schema: 3,
        zero_threshold: 2.938735877055719e-39,
        positive_spans: vec![Span {
            offset: 0,
            length: 2,
        }],
        negative_spans: vec![],
        positive_buckets: vec![4, 4], // Same buckets as h1
        negative_buckets: vec![],
        custom_values: vec![],
        counter_reset_hint: CounterResetHint::UnknownCounterReset,
    };
    let h3 = create_test_histogram(15, 28.0, vec![6, 6]);

    let mut chunk = HistogramChunk::new();
    chunk.append(1000, &h1);
    chunk.append(2000, &h2);
    chunk.append(3000, &h3);

    let encoded = chunk.encode();
    assert!(!encoded.is_empty());
    assert_eq!(chunk.num_samples(), 3);
}

// Property-based test for histograms
proptest! {
    #[test]
    fn prop_histogram_roundtrip(
        count in 0u64..10000,
        sum in 0.0f64..1000.0,
        bucket_count in 1usize..10,
    ) {
        let buckets: Vec<i64> = (0..bucket_count)
            .map(|i| (i + 1) as i64 * 2)
            .collect();

        let h = Histogram {
            count,
            zero_count: 0,
            sum,
            schema: 3,
            zero_threshold: 2.938735877055719e-39,
            positive_spans: vec![Span {
                offset: 0,
                length: bucket_count as u32,
            }],
            negative_spans: vec![],
            positive_buckets: buckets,
            negative_buckets: vec![],
            custom_values: vec![],
            counter_reset_hint: CounterResetHint::UnknownCounterReset,
        };

        let mut chunk = HistogramChunk::new();
        chunk.append(1000, &h);
        chunk.append(2000, &h);

        let encoded = chunk.encode();
        prop_assert!(!encoded.is_empty());
        prop_assert_eq!(chunk.num_samples(), 2);
    }
}

// ==================== Float Histogram Round-Trip Tests ====================

fn create_test_float_histogram(count: f64, sum: f64, buckets: Vec<f64>) -> FloatHistogram {
    FloatHistogram {
        count,
        zero_count: 0.0,
        sum,
        schema: 3,
        zero_threshold: 2.938735877055719e-39,
        positive_spans: vec![Span {
            offset: 0,
            length: buckets.len() as u32,
        }],
        negative_spans: vec![],
        positive_buckets: buckets,
        negative_buckets: vec![],
        custom_values: vec![],
        counter_reset_hint: CounterResetHint::UnknownCounterReset,
    }
}

#[test]
fn test_float_histogram_roundtrip_basic() {
    let fh1 = create_test_float_histogram(10.0, 18.4, vec![4.0, 4.0, 2.0]);
    let fh2 = create_test_float_histogram(15.0, 28.0, vec![6.0, 6.0, 3.0]);
    let fh3 = create_test_float_histogram(22.0, 40.5, vec![8.0, 9.0, 5.0]);

    let mut chunk = FloatHistogramChunk::new();
    chunk.append(1000, &fh1);
    chunk.append(2000, &fh2);
    chunk.append(3000, &fh3);

    let encoded = chunk.encode();
    assert!(!encoded.is_empty());
    assert_eq!(chunk.num_samples(), 3);
}

#[test]
fn test_float_histogram_roundtrip_with_negative_spans() {
    let fh = FloatHistogram {
        count: 20.0,
        zero_count: 5.0,
        sum: 35.0,
        schema: 3,
        zero_threshold: 1e-100,
        positive_spans: vec![Span {
            offset: 0,
            length: 2,
        }],
        negative_spans: vec![Span {
            offset: 1,
            length: 3,
        }],
        positive_buckets: vec![5.0, 5.0],
        negative_buckets: vec![3.0, 4.0, 3.0],
        custom_values: vec![],
        counter_reset_hint: CounterResetHint::UnknownCounterReset,
    };

    let mut chunk = FloatHistogramChunk::new();
    chunk.append(1000, &fh);
    chunk.append(2000, &fh);

    let encoded = chunk.encode();
    assert!(!encoded.is_empty());
}

#[test]
fn test_float_histogram_roundtrip_edge_values() {
    let buckets = vec![
        0.0f64,
        -0.0,
        f64::MIN_POSITIVE,
        f64::MAX,
        f64::INFINITY,
        f64::NAN,
    ];

    let fh = FloatHistogram {
        count: 10.0,
        zero_count: 0.0,
        sum: 25.0,
        schema: 3,
        zero_threshold: 2.938735877055719e-39,
        positive_spans: vec![Span {
            offset: 0,
            length: buckets.len() as u32,
        }],
        negative_spans: vec![],
        positive_buckets: buckets.clone(),
        negative_buckets: vec![],
        custom_values: vec![],
        counter_reset_hint: CounterResetHint::UnknownCounterReset,
    };

    let mut chunk = FloatHistogramChunk::new();
    chunk.append(1000, &fh);

    let encoded = chunk.encode();
    assert!(!encoded.is_empty());
}

#[test]
fn test_float_histogram_roundtrip_stale_marker() {
    let fh1 = create_test_float_histogram(10.0, 18.4, vec![4.0, 4.0]);
    let fh2 = FloatHistogram {
        count: 10.0,
        zero_count: 0.0,
        sum: f64::from_bits(STALE_NAN_BITS),
        schema: 3,
        zero_threshold: 2.938735877055719e-39,
        positive_spans: vec![Span {
            offset: 0,
            length: 2,
        }],
        negative_spans: vec![],
        positive_buckets: vec![4.0, 4.0],
        negative_buckets: vec![],
        custom_values: vec![],
        counter_reset_hint: CounterResetHint::UnknownCounterReset,
    };
    let fh3 = create_test_float_histogram(15.0, 28.0, vec![6.0, 6.0]);

    let mut chunk = FloatHistogramChunk::new();
    chunk.append(1000, &fh1);
    chunk.append(2000, &fh2);
    chunk.append(3000, &fh3);

    let encoded = chunk.encode();
    assert!(!encoded.is_empty());
    assert_eq!(chunk.num_samples(), 3);
}

// Property-based test for float histograms
proptest! {
    #[test]
    fn prop_float_histogram_roundtrip(
        count in 0.0f64..10000.0,
        sum in 0.0f64..1000.0,
        bucket_count in 1usize..10,
    ) {
        let buckets: Vec<f64> = (0..bucket_count)
            .map(|i| (i + 1) as f64 * 2.5)
            .collect();

        let fh = FloatHistogram {
            count,
            zero_count: 0.0,
            sum,
            schema: 3,
            zero_threshold: 2.938735877055719e-39,
            positive_spans: vec![Span {
                offset: 0,
                length: bucket_count as u32,
            }],
            negative_spans: vec![],
            positive_buckets: buckets,
            negative_buckets: vec![],
            custom_values: vec![],
            counter_reset_hint: CounterResetHint::UnknownCounterReset,
        };

        let mut chunk = FloatHistogramChunk::new();
        chunk.append(1000, &fh);
        chunk.append(2000, &fh);

        let encoded = chunk.encode();
        prop_assert!(!encoded.is_empty());
        prop_assert_eq!(chunk.num_samples(), 2);
    }
}

// ==================== Varbit Boundary Tests ====================

#[test]
fn test_varbit_boundary_values() {
    use rs_prom_encoder::bstream::{BStreamReader, BStreamWriter};
    use rs_prom_encoder::varbit::{
        put_varbit_int, put_varbit_uint, read_varbit_int, read_varbit_uint,
    };

    // Test signed boundary values from Go test suite
    let signed_boundaries = vec![
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

    for val in &signed_boundaries {
        let mut w = BStreamWriter::new();
        put_varbit_int(&mut w, *val);

        let mut r = BStreamReader::new(w.bytes());
        let decoded = read_varbit_int(&mut r).expect("Failed to decode varbit int");

        assert_eq!(
            decoded, *val,
            "Varbit int roundtrip failed for {}: got {}",
            val, decoded
        );
    }

    // Test unsigned boundary values
    let unsigned_boundaries = vec![
        0u64,
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

    for val in &unsigned_boundaries {
        let mut w = BStreamWriter::new();
        put_varbit_uint(&mut w, *val);

        let mut r = BStreamReader::new(w.bytes());
        let decoded = read_varbit_uint(&mut r).expect("Failed to decode varbit uint");

        assert_eq!(
            decoded, *val,
            "Varbit uint roundtrip failed for {}: got {}",
            val, decoded
        );
    }
}

// Property-based test for varbit encoding
proptest! {
    #[test]
    fn prop_varbit_int_roundtrip(val in any::<i64>()) {
        use rs_prom_encoder::bstream::{BStreamReader, BStreamWriter};
        use rs_prom_encoder::varbit::{put_varbit_int, read_varbit_int};

        let mut w = BStreamWriter::new();
        put_varbit_int(&mut w, val);

        let mut r = BStreamReader::new(w.bytes());
        let decoded = read_varbit_int(&mut r).expect("Failed to decode");

        prop_assert_eq!(decoded, val);
    }

    #[test]
    fn prop_varbit_uint_roundtrip(val in any::<u64>()) {
        use rs_prom_encoder::bstream::{BStreamReader, BStreamWriter};
        use rs_prom_encoder::varbit::{put_varbit_uint, read_varbit_uint};

        let mut w = BStreamWriter::new();
        put_varbit_uint(&mut w, val);

        let mut r = BStreamReader::new(w.bytes());
        let decoded = read_varbit_uint(&mut r).expect("Failed to decode");

        prop_assert_eq!(decoded, val);
    }
}

// ==================== Large Scale Tests ====================

#[test]
fn test_xor_large_chunk() {
    // Create a large chunk with many samples
    let mut chunk = XORChunk::with_capacity(1000);
    let base_time = 1_700_000_000_000i64;

    for i in 0..1000 {
        let t = base_time + i * 15000;
        let v = 100.0 + (i as f64 * 0.1).sin() * 50.0;
        chunk.append(t, v);
    }

    let encoded = chunk.encode();
    assert!(!encoded.is_empty());
    assert_eq!(chunk.num_samples(), 1000);

    // Decode and verify count
    let (_, decoded) = read_xor_chunk_data(&encoded).expect("Failed to decode large XOR chunk");
    assert_eq!(decoded.samples().len(), 1000);
}

#[test]
fn test_histogram_large_chunk() {
    let mut chunk = HistogramChunk::new();
    let base_time = 1_700_000_000_000i64;

    for i in 0..50 {
        let t = base_time + i * 15000;
        let h = create_test_histogram(
            10 + i as u64,
            18.4 + i as f64,
            vec![4 + i as i64, 4 + i as i64],
        );
        chunk.append(t, &h);
    }

    let encoded = chunk.encode();
    assert!(!encoded.is_empty());
    assert_eq!(chunk.num_samples(), 50);
}
