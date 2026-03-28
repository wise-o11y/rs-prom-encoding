//! Golden compatibility tests - compare Rust encoder output against Go reference.
//!
//! These tests ensure the Rust encoder produces byte-identical chunks to Prometheus's
//! Go implementation.

use rs_prom_encoder::{
    CounterResetHint, FloatHistogram, FloatHistogramChunk, Histogram, HistogramChunk, Span,
    XORChunk, CUSTOM_BUCKETS_SCHEMA,
};
use serde::Deserialize;
use std::fs;

/// A sample from the JSON fixture.
#[derive(Debug, Deserialize)]
struct Sample {
    #[serde(rename = "t")]
    timestamp: i64,
    #[serde(rename = "v", default)]
    value: Option<f64>,
}

/// A histogram sample from the JSON fixture.
#[derive(Debug, Deserialize)]
struct HistogramSample {
    #[serde(rename = "t")]
    timestamp: i64,
    #[serde(rename = "h")]
    histogram: Option<HistogramDef>,
}

/// Histogram definition from JSON.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct HistogramDef {
    count: u64,
    zero_count: u64,
    sum: f64,
    schema: i32,
    zero_threshold: f64,
    positive_spans: Vec<SpanDef>,
    negative_spans: Vec<SpanDef>,
    positive_buckets: Vec<i64>,
    negative_buckets: Vec<i64>,
    #[serde(default)]
    custom_values: Option<Vec<f64>>,
    counter_reset_hint: u8,
}

/// Float histogram definition from JSON.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct FloatHistogramDef {
    count: f64,
    zero_count: f64,
    sum: f64,
    schema: i32,
    zero_threshold: f64,
    positive_spans: Vec<SpanDef>,
    negative_spans: Vec<SpanDef>,
    positive_buckets: Vec<f64>,
    negative_buckets: Vec<f64>,
    #[serde(default)]
    custom_values: Option<Vec<f64>>,
    counter_reset_hint: u8,
}

/// Span definition from JSON.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct SpanDef {
    offset: i32,
    length: u32,
}

/// Float histogram sample from JSON.
#[derive(Debug, Deserialize)]
struct FloatHistogramSample {
    #[serde(rename = "t")]
    timestamp: i64,
    #[serde(rename = "fh")]
    histogram: Option<FloatHistogramDef>,
}

/// Fixture metadata.
#[derive(Debug, Deserialize)]
struct FixtureMeta {
    encoding: String,
    samples: Vec<Sample>,
}

/// Histogram fixture metadata.
#[derive(Debug, Deserialize)]
struct HistogramFixtureMeta {
    encoding: String,
    samples: Vec<HistogramSample>,
}

/// Float histogram fixture metadata.
#[derive(Debug, Deserialize)]
struct FloatHistogramFixtureMeta {
    encoding: String,
    samples: Vec<FloatHistogramSample>,
}

fn load_bin(path: &str) -> Vec<u8> {
    fs::read(path).unwrap_or_else(|e| panic!("Failed to read {}: {}", path, e))
}

fn load_json<T: for<'de> Deserialize<'de>>(path: &str) -> T {
    let content =
        fs::read_to_string(path).unwrap_or_else(|e| panic!("Failed to read {}: {}", path, e));
    serde_json::from_str(&content).unwrap_or_else(|e| panic!("Failed to parse {}: {}", path, e))
}

// ==================== XOR Tests ====================

#[test]
fn test_xor_basic_golden() {
    let meta: FixtureMeta = load_json("tests/fixtures/xor_basic.json");
    let expected = load_bin("tests/fixtures/xor_basic.bin");

    let mut chunk = XORChunk::new();
    for s in &meta.samples {
        chunk.append(s.timestamp, s.value.unwrap_or(0.0));
    }

    let actual = chunk.encode();
    assert_eq!(
        actual, expected,
        "XOR basic: Rust bytes differ from Go golden"
    );
}

#[test]
fn test_xor_stale_golden() {
    let meta: FixtureMeta = load_json("tests/fixtures/xor_stale.json");
    let expected = load_bin("tests/fixtures/xor_stale.bin");

    let mut chunk = XORChunk::new();
    for s in &meta.samples {
        chunk.append(s.timestamp, s.value.unwrap_or(0.0));
    }

    let actual = chunk.encode();
    assert_eq!(
        actual, expected,
        "XOR stale: Rust bytes differ from Go golden"
    );
}

#[test]
fn test_xor_edge_cases_golden() {
    let meta: FixtureMeta = load_json("tests/fixtures/xor_edge_cases.json");
    let expected = load_bin("tests/fixtures/xor_edge_cases.bin");

    let mut chunk = XORChunk::new();
    for s in &meta.samples {
        chunk.append(s.timestamp, s.value.unwrap_or(0.0));
    }

    let actual = chunk.encode();
    assert_eq!(
        actual, expected,
        "XOR edge cases: Rust bytes differ from Go golden"
    );
}

#[test]
fn test_xor_many_golden() {
    let meta: FixtureMeta = load_json("tests/fixtures/xor_many.json");
    let expected = load_bin("tests/fixtures/xor_many.bin");

    let mut chunk = XORChunk::new();
    for s in &meta.samples {
        chunk.append(s.timestamp, s.value.unwrap_or(0.0));
    }

    let actual = chunk.encode();
    assert_eq!(
        actual, expected,
        "XOR many: Rust bytes differ from Go golden"
    );
}

// ==================== Histogram Tests ====================

fn parse_counter_reset_hint(v: u8) -> CounterResetHint {
    // Go's histogram.CounterResetHint enum values:
    // 0 = UnknownCounterReset, 1 = CounterReset, 2 = NotCounterReset, 3 = GaugeType
    match v {
        3 => CounterResetHint::GaugeType,
        1 => CounterResetHint::CounterReset,
        2 => CounterResetHint::NotCounterReset,
        _ => CounterResetHint::UnknownCounterReset,
    }
}

fn convert_span(def: &SpanDef) -> Span {
    Span {
        offset: def.offset,
        length: def.length,
    }
}

#[test]
fn test_histogram_basic_golden() {
    let meta: HistogramFixtureMeta = load_json("tests/fixtures/histogram_basic.json");
    let expected = load_bin("tests/fixtures/histogram_basic.bin");

    let mut chunk = HistogramChunk::new();
    for s in &meta.samples {
        let h_def = s.histogram.as_ref().unwrap();
        let h = Histogram {
            count: h_def.count,
            zero_count: h_def.zero_count,
            sum: h_def.sum,
            schema: h_def.schema,
            zero_threshold: h_def.zero_threshold,
            positive_spans: h_def.positive_spans.iter().map(convert_span).collect(),
            negative_spans: h_def.negative_spans.iter().map(convert_span).collect(),
            positive_buckets: h_def.positive_buckets.clone(),
            negative_buckets: h_def.negative_buckets.clone(),
            custom_values: h_def.custom_values.clone().unwrap_or_default(),
            counter_reset_hint: parse_counter_reset_hint(h_def.counter_reset_hint),
        };
        chunk.append(s.timestamp, &h);
    }

    let actual = chunk.encode();
    assert_eq!(
        actual, expected,
        "Histogram basic: Rust bytes differ from Go golden"
    );
}

#[test]
fn test_histogram_stale_golden() {
    let meta: HistogramFixtureMeta = load_json("tests/fixtures/histogram_stale.json");
    let expected = load_bin("tests/fixtures/histogram_stale.bin");

    let mut chunk = HistogramChunk::new();
    for s in &meta.samples {
        let h_def = s.histogram.as_ref().unwrap();
        let h = Histogram {
            count: h_def.count,
            zero_count: h_def.zero_count,
            sum: h_def.sum,
            schema: h_def.schema,
            zero_threshold: h_def.zero_threshold,
            positive_spans: h_def.positive_spans.iter().map(convert_span).collect(),
            negative_spans: h_def.negative_spans.iter().map(convert_span).collect(),
            positive_buckets: h_def.positive_buckets.clone(),
            negative_buckets: h_def.negative_buckets.clone(),
            custom_values: h_def.custom_values.clone().unwrap_or_default(),
            counter_reset_hint: parse_counter_reset_hint(h_def.counter_reset_hint),
        };
        chunk.append(s.timestamp, &h);
    }

    let actual = chunk.encode();
    assert_eq!(
        actual, expected,
        "Histogram stale: Rust bytes differ from Go golden"
    );
}

#[test]
fn test_histogram_gauge_golden() {
    let meta: HistogramFixtureMeta = load_json("tests/fixtures/histogram_gauge.json");
    let expected = load_bin("tests/fixtures/histogram_gauge.bin");

    let mut chunk = HistogramChunk::new();
    // Don't manually set counter reset - let the histogram's CounterResetHint handle it

    for s in &meta.samples {
        let h_def = s.histogram.as_ref().unwrap();
        let h = Histogram {
            count: h_def.count,
            zero_count: h_def.zero_count,
            sum: h_def.sum,
            schema: h_def.schema,
            zero_threshold: h_def.zero_threshold,
            positive_spans: h_def.positive_spans.iter().map(convert_span).collect(),
            negative_spans: h_def.negative_spans.iter().map(convert_span).collect(),
            positive_buckets: h_def.positive_buckets.clone(),
            negative_buckets: h_def.negative_buckets.clone(),
            custom_values: h_def.custom_values.clone().unwrap_or_default(),
            counter_reset_hint: parse_counter_reset_hint(h_def.counter_reset_hint),
        };
        chunk.append(s.timestamp, &h);
    }

    let actual = chunk.encode();
    assert_eq!(
        actual, expected,
        "Histogram gauge: Rust bytes differ from Go golden"
    );
}

#[test]
fn test_histogram_custom_bounds_golden() {
    let meta: HistogramFixtureMeta = load_json("tests/fixtures/histogram_custom_bounds.json");
    let expected = load_bin("tests/fixtures/histogram_custom_bounds.bin");

    let mut chunk = HistogramChunk::new();
    for s in &meta.samples {
        let h_def = s.histogram.as_ref().unwrap();
        let h = Histogram {
            count: h_def.count,
            zero_count: h_def.zero_count,
            sum: h_def.sum,
            schema: CUSTOM_BUCKETS_SCHEMA, // Force custom buckets schema
            zero_threshold: h_def.zero_threshold,
            positive_spans: h_def.positive_spans.iter().map(convert_span).collect(),
            negative_spans: h_def.negative_spans.iter().map(convert_span).collect(),
            positive_buckets: h_def.positive_buckets.clone(),
            negative_buckets: h_def.negative_buckets.clone(),
            custom_values: h_def.custom_values.clone().unwrap_or_default(),
            counter_reset_hint: parse_counter_reset_hint(h_def.counter_reset_hint),
        };
        chunk.append(s.timestamp, &h);
    }

    let actual = chunk.encode();
    assert_eq!(
        actual, expected,
        "Histogram custom bounds: Rust bytes differ from Go golden"
    );
}

// ==================== Float Histogram Tests ====================

#[test]
fn test_float_histogram_basic_golden() {
    let meta: FloatHistogramFixtureMeta = load_json("tests/fixtures/float_histogram_basic.json");
    let expected = load_bin("tests/fixtures/float_histogram_basic.bin");

    let mut chunk = FloatHistogramChunk::new();
    for s in &meta.samples {
        let h_def = s.histogram.as_ref().unwrap();
        let h = FloatHistogram {
            count: h_def.count,
            zero_count: h_def.zero_count,
            sum: h_def.sum,
            schema: h_def.schema,
            zero_threshold: h_def.zero_threshold,
            positive_spans: h_def.positive_spans.iter().map(convert_span).collect(),
            negative_spans: h_def.negative_spans.iter().map(convert_span).collect(),
            positive_buckets: h_def.positive_buckets.clone(),
            negative_buckets: h_def.negative_buckets.clone(),
            custom_values: h_def.custom_values.clone().unwrap_or_default(),
            counter_reset_hint: parse_counter_reset_hint(h_def.counter_reset_hint),
        };
        chunk.append(s.timestamp, &h);
    }

    let actual = chunk.encode();
    assert_eq!(
        actual, expected,
        "Float histogram basic: Rust bytes differ from Go golden"
    );
}

#[test]
fn test_float_histogram_stale_golden() {
    let meta: FloatHistogramFixtureMeta = load_json("tests/fixtures/float_histogram_stale.json");
    let expected = load_bin("tests/fixtures/float_histogram_stale.bin");

    let mut chunk = FloatHistogramChunk::new();
    for s in &meta.samples {
        let h_def = s.histogram.as_ref().unwrap();
        let h = FloatHistogram {
            count: h_def.count,
            zero_count: h_def.zero_count,
            sum: h_def.sum,
            schema: h_def.schema,
            zero_threshold: h_def.zero_threshold,
            positive_spans: h_def.positive_spans.iter().map(convert_span).collect(),
            negative_spans: h_def.negative_spans.iter().map(convert_span).collect(),
            positive_buckets: h_def.positive_buckets.clone(),
            negative_buckets: h_def.negative_buckets.clone(),
            custom_values: h_def.custom_values.clone().unwrap_or_default(),
            counter_reset_hint: parse_counter_reset_hint(h_def.counter_reset_hint),
        };
        chunk.append(s.timestamp, &h);
    }

    let actual = chunk.encode();
    assert_eq!(
        actual, expected,
        "Float histogram stale: Rust bytes differ from Go golden"
    );
}
