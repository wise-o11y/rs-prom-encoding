//! Native float histogram chunk encoding.
//!
//! Matches Prometheus `tsdb/chunkenc/float_histogram.go` (EncFloatHistogram = 3).
//!
//! The key difference from integer histograms: all numeric fields except timestamp
//! use XOR (Gorilla) encoding instead of varbit delta-of-delta.

use crate::bstream::BStreamWriter;
use crate::histogram_meta::write_histogram_chunk_layout;
use crate::histogram_types::{CounterResetHint, FloatHistogram, STALE_NAN_BITS};
use crate::varbit::put_varbit_int;
use crate::xor::{self, XOR_LEADING_SENTINEL};

/// Header size for histogram chunks: 2 bytes sample count + 1 byte flags.
const HISTOGRAM_HEADER_SIZE: usize = 3;

/// XOR state for a single float field.
struct XorValue {
    value: f64,
    leading: u8,
    trailing: u8,
}

impl XorValue {
    fn new() -> Self {
        Self {
            value: 0.0,
            leading: XOR_LEADING_SENTINEL,
            trailing: 0,
        }
    }
}

/// A Prometheus native float histogram chunk encoder.
///
/// Produces byte-identical output to Go's `chunkenc.FloatHistogramChunk`.
pub struct FloatHistogramChunk {
    bw: BStreamWriter,
    counter_reset_hint: CounterResetHint,

    // Layout (set on first sample)
    schema: i32,
    z_threshold: f64,
    p_spans: Vec<crate::histogram_types::Span>,
    n_spans: Vec<crate::histogram_types::Span>,
    custom_values: Vec<f64>,

    // State for delta-of-delta timestamp encoding
    t: i64,
    t_delta: i64,

    // XOR state for each float field
    cnt: XorValue,
    z_cnt: XorValue,
    sum: XorValue,
    p_buckets: Vec<XorValue>,
    n_buckets: Vec<XorValue>,

    num_samples: u16,
}

impl FloatHistogramChunk {
    /// Creates a new empty float histogram chunk.
    pub fn new() -> Self {
        let mut bw = BStreamWriter::with_capacity(128);
        for _ in 0..HISTOGRAM_HEADER_SIZE {
            bw.write_byte(0);
        }

        Self {
            bw,
            counter_reset_hint: CounterResetHint::UnknownCounterReset,
            schema: 0,
            z_threshold: 0.0,
            p_spans: Vec::new(),
            n_spans: Vec::new(),
            custom_values: Vec::new(),
            t: 0,
            t_delta: 0,
            cnt: XorValue::new(),
            z_cnt: XorValue::new(),
            sum: XorValue::new(),
            p_buckets: Vec::new(),
            n_buckets: Vec::new(),
            num_samples: 0,
        }
    }

    /// Sets the counter reset header hint.
    pub fn set_counter_reset_header(&mut self, hint: CounterResetHint) {
        self.counter_reset_hint = hint;
    }

    /// Appends a float histogram sample to the chunk.
    pub fn append(&mut self, t: i64, h: &FloatHistogram) {
        if self.num_samples == 0 {
            self.append_first(t, h);
        } else {
            self.append_subsequent(t, h);
        }
        self.num_samples += 1;
    }

    /// Returns the number of samples in the chunk.
    pub fn num_samples(&self) -> u16 {
        self.num_samples
    }

    /// Encodes the chunk and returns the raw chunk bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = self.bw.bytes().to_vec();
        if bytes.len() >= HISTOGRAM_HEADER_SIZE {
            bytes[0] = (self.num_samples >> 8) as u8;
            bytes[1] = self.num_samples as u8;
            bytes[2] = self.counter_reset_hint as u8;
        }
        bytes
    }

    fn append_first(&mut self, t: i64, h: &FloatHistogram) {
        self.schema = h.schema;
        self.z_threshold = h.zero_threshold;
        self.p_spans = h.positive_spans.clone();
        self.n_spans = h.negative_spans.clone();
        self.custom_values = h.custom_values.clone();
        self.counter_reset_hint = h.counter_reset_hint;

        // Write layout
        write_histogram_chunk_layout(
            &mut self.bw,
            h.schema,
            h.zero_threshold,
            &h.positive_spans,
            &h.negative_spans,
            &h.custom_values,
        );

        // Write first sample: all raw values
        put_varbit_int(&mut self.bw, t);
        self.bw.write_bits(h.count.to_bits(), 64);
        self.bw.write_bits(h.zero_count.to_bits(), 64);
        self.bw.write_bits(h.sum.to_bits(), 64);

        for &bucket in &h.positive_buckets {
            self.bw.write_bits(bucket.to_bits(), 64);
        }
        for &bucket in &h.negative_buckets {
            self.bw.write_bits(bucket.to_bits(), 64);
        }

        // Save state
        self.t = t;
        self.t_delta = 0;
        self.cnt = XorValue {
            value: h.count,
            leading: XOR_LEADING_SENTINEL,
            trailing: 0,
        };
        self.z_cnt = XorValue {
            value: h.zero_count,
            leading: XOR_LEADING_SENTINEL,
            trailing: 0,
        };
        self.sum = XorValue {
            value: h.sum,
            leading: XOR_LEADING_SENTINEL,
            trailing: 0,
        };

        self.p_buckets = h.positive_buckets.iter().map(|_| XorValue::new()).collect();
        for (i, &v) in h.positive_buckets.iter().enumerate() {
            self.p_buckets[i].value = v;
        }

        self.n_buckets = h.negative_buckets.iter().map(|_| XorValue::new()).collect();
        for (i, &v) in h.negative_buckets.iter().enumerate() {
            self.n_buckets[i].value = v;
        }
    }

    fn append_subsequent(&mut self, t: i64, h: &FloatHistogram) {
        let is_stale = h.sum.to_bits() == STALE_NAN_BITS;

        // Timestamp delta-of-delta
        let t_delta = t - self.t;
        let t_dod = t_delta - self.t_delta;
        put_varbit_int(&mut self.bw, t_dod);

        // Count (XOR)
        xor::xor_write(
            &mut self.bw,
            h.count,
            self.cnt.value,
            &mut self.cnt.leading,
            &mut self.cnt.trailing,
        );

        // Zero count (XOR)
        xor::xor_write(
            &mut self.bw,
            h.zero_count,
            self.z_cnt.value,
            &mut self.z_cnt.leading,
            &mut self.z_cnt.trailing,
        );

        // Sum (XOR)
        xor::xor_write(
            &mut self.bw,
            h.sum,
            self.sum.value,
            &mut self.sum.leading,
            &mut self.sum.trailing,
        );

        if !is_stale {
            // Positive buckets (XOR)
            for (i, &bucket) in h.positive_buckets.iter().enumerate() {
                let xv = &mut self.p_buckets[i];
                xor::xor_write(
                    &mut self.bw,
                    bucket,
                    xv.value,
                    &mut xv.leading,
                    &mut xv.trailing,
                );
                xv.value = bucket;
            }

            // Negative buckets (XOR)
            for (i, &bucket) in h.negative_buckets.iter().enumerate() {
                let xv = &mut self.n_buckets[i];
                xor::xor_write(
                    &mut self.bw,
                    bucket,
                    xv.value,
                    &mut xv.leading,
                    &mut xv.trailing,
                );
                xv.value = bucket;
            }

            self.cnt.value = h.count;
            self.z_cnt.value = h.zero_count;
        }

        self.t = t;
        self.t_delta = t_delta;
        self.sum.value = h.sum;
    }
}

impl Default for FloatHistogramChunk {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::histogram_types::Span;

    fn sample_float_histogram() -> FloatHistogram {
        FloatHistogram {
            count: 10.0,
            zero_count: 2.0,
            sum: 18.4,
            schema: 3,
            zero_threshold: 2.938_735_877_055_719e-39,
            positive_spans: vec![Span {
                offset: 0,
                length: 2,
            }],
            negative_spans: vec![],
            positive_buckets: vec![4.0, 4.0],
            negative_buckets: vec![],
            custom_values: vec![],
            counter_reset_hint: CounterResetHint::UnknownCounterReset,
        }
    }

    #[test]
    fn test_single_float_histogram_sample() {
        let mut chunk = FloatHistogramChunk::new();
        chunk.append(1000, &sample_float_histogram());

        let bytes = chunk.encode();
        assert_eq!(chunk.num_samples(), 1);
        assert!(bytes.len() > HISTOGRAM_HEADER_SIZE);
        assert_eq!(bytes[0], 0);
        assert_eq!(bytes[1], 1);
    }

    #[test]
    fn test_multiple_float_histogram_samples() {
        let mut chunk = FloatHistogramChunk::new();

        let h1 = sample_float_histogram();
        chunk.append(1000, &h1);

        let h2 = FloatHistogram {
            count: 15.0,
            zero_count: 3.0,
            sum: 28.0,
            ..h1.clone()
        };
        chunk.append(2000, &h2);

        let bytes = chunk.encode();
        assert_eq!(chunk.num_samples(), 2);
        assert_eq!(bytes[0], 0);
        assert_eq!(bytes[1], 2);
    }
}
