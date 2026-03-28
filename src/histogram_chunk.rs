//! Native integer histogram chunk encoding.
//!
//! Matches Prometheus `tsdb/chunkenc/histogram.go` (EncHistogram = 2).

use crate::bstream::BStreamWriter;
use crate::histogram_meta::{count_spans, write_histogram_chunk_layout};
use crate::histogram_types::{CounterResetHint, Histogram, STALE_NAN_BITS};
use crate::varbit::{put_varbit_int, put_varbit_uint};
use crate::xor;

/// Header size for histogram chunks: 2 bytes sample count + 1 byte flags.
const HISTOGRAM_HEADER_SIZE: usize = 3;

/// A Prometheus native integer histogram chunk encoder.
///
/// Produces byte-identical output to Go's `chunkenc.HistogramChunk`.
///
/// # Example
///
/// ```rust,no_run
/// use rs_prom_encoder::{HistogramChunk, Histogram, Span, CounterResetHint};
///
/// let mut chunk = HistogramChunk::new();
/// chunk.append(1000, &Histogram {
///     count: 10,
///     zero_count: 2,
///     sum: 18.4,
///     schema: 3,
///     zero_threshold: 2.938735877055719e-39,
///     positive_spans: vec![Span { offset: 0, length: 2 }],
///     negative_spans: vec![],
///     positive_buckets: vec![4, 4],
///     negative_buckets: vec![],
///     custom_values: vec![],
///     counter_reset_hint: CounterResetHint::UnknownCounterReset,
/// });
///
/// let bytes = chunk.encode();
/// ```
pub struct HistogramChunk {
    bw: BStreamWriter,
    counter_reset_hint: CounterResetHint,

    // Layout (set on first sample)
    schema: i32,
    z_threshold: f64,
    p_spans: Vec<crate::histogram_types::Span>,
    n_spans: Vec<crate::histogram_types::Span>,
    custom_values: Vec<f64>,

    // State for delta-of-delta encoding
    t: i64,
    cnt: u64,
    z_cnt: u64,
    t_delta: i64,
    cnt_delta: i64,
    z_cnt_delta: i64,
    p_buckets: Vec<i64>,
    n_buckets: Vec<i64>,
    p_buckets_delta: Vec<i64>,
    n_buckets_delta: Vec<i64>,

    // XOR state for sum
    sum: f64,
    leading: u8,
    trailing: u8,

    num_samples: u16,
}

impl HistogramChunk {
    /// Creates a new empty histogram chunk.
    pub fn new() -> Self {
        // Pre-allocate header bytes (zeroed)
        let mut bw = BStreamWriter::with_capacity(128);
        // Write 3 header bytes (will be overwritten on encode)
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
            cnt: 0,
            z_cnt: 0,
            t_delta: 0,
            cnt_delta: 0,
            z_cnt_delta: 0,
            p_buckets: Vec::new(),
            n_buckets: Vec::new(),
            p_buckets_delta: Vec::new(),
            n_buckets_delta: Vec::new(),
            sum: 0.0,
            leading: xor::XOR_LEADING_SENTINEL,
            trailing: 0,
            num_samples: 0,
        }
    }

    /// Sets the counter reset header hint.
    pub fn set_counter_reset_header(&mut self, hint: CounterResetHint) {
        self.counter_reset_hint = hint;
    }

    /// Appends a histogram sample to the chunk.
    pub fn append(&mut self, t: i64, h: &Histogram) {
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
    ///
    /// The returned bytes match Go's `HistogramChunk.Bytes()`:
    /// `[2 byte count][1 byte flags][bitstream payload]`.
    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = self.bw.bytes().to_vec();
        if bytes.len() >= HISTOGRAM_HEADER_SIZE {
            // Write sample count (big-endian u16) at bytes[0..2]
            bytes[0] = (self.num_samples >> 8) as u8;
            bytes[1] = self.num_samples as u8;
            // Write flags byte at bytes[2]
            bytes[2] = self.counter_reset_hint as u8;
        }
        bytes
    }

    fn append_first(&mut self, t: i64, h: &Histogram) {
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

        // Write first sample data
        put_varbit_int(&mut self.bw, t);
        put_varbit_uint(&mut self.bw, h.count);
        put_varbit_uint(&mut self.bw, h.zero_count);
        self.bw.write_bits(h.sum.to_bits(), 64);

        for &bucket in &h.positive_buckets {
            put_varbit_int(&mut self.bw, bucket);
        }
        for &bucket in &h.negative_buckets {
            put_varbit_int(&mut self.bw, bucket);
        }

        // Save state
        self.t = t;
        self.cnt = h.count;
        self.z_cnt = h.zero_count;
        self.sum = h.sum;
        self.p_buckets = h.positive_buckets.clone();
        self.n_buckets = h.negative_buckets.clone();
        self.p_buckets_delta = vec![0; count_spans(&h.positive_spans)];
        self.n_buckets_delta = vec![0; count_spans(&h.negative_spans)];
        self.t_delta = 0;
        self.cnt_delta = 0;
        self.z_cnt_delta = 0;
    }

    fn append_subsequent(&mut self, t: i64, h: &Histogram) {
        let is_stale = h.sum.to_bits() == STALE_NAN_BITS;

        // Timestamp delta-of-delta
        let t_delta = t - self.t;
        let t_dod = t_delta - self.t_delta;
        put_varbit_int(&mut self.bw, t_dod);

        // Count delta-of-delta
        let cnt_delta = if is_stale {
            self.cnt_delta // Force dod=0
        } else {
            h.count as i64 - self.cnt as i64
        };
        let cnt_dod = cnt_delta - self.cnt_delta;
        put_varbit_int(&mut self.bw, if is_stale { 0 } else { cnt_dod });

        // Zero count delta-of-delta
        let z_cnt_delta = if is_stale {
            self.z_cnt_delta
        } else {
            h.zero_count as i64 - self.z_cnt as i64
        };
        let z_cnt_dod = z_cnt_delta - self.z_cnt_delta;
        put_varbit_int(&mut self.bw, if is_stale { 0 } else { z_cnt_dod });

        // Sum (XOR encoded)
        xor::xor_write(
            &mut self.bw,
            h.sum,
            self.sum,
            &mut self.leading,
            &mut self.trailing,
        );

        if !is_stale {
            // Positive bucket delta-of-deltas
            for (i, &bucket) in h.positive_buckets.iter().enumerate() {
                let delta = bucket - self.p_buckets[i];
                let dod = delta - self.p_buckets_delta[i];
                put_varbit_int(&mut self.bw, dod);
                self.p_buckets_delta[i] = delta;
            }

            // Negative bucket delta-of-deltas
            for (i, &bucket) in h.negative_buckets.iter().enumerate() {
                let delta = bucket - self.n_buckets[i];
                let dod = delta - self.n_buckets_delta[i];
                put_varbit_int(&mut self.bw, dod);
                self.n_buckets_delta[i] = delta;
            }

            self.p_buckets = h.positive_buckets.clone();
            self.n_buckets = h.negative_buckets.clone();
            self.cnt = h.count;
            self.z_cnt = h.zero_count;
            self.cnt_delta = cnt_delta;
            self.z_cnt_delta = z_cnt_delta;
        }

        self.t = t;
        self.t_delta = t_delta;
        self.sum = h.sum;
    }
}

impl Default for HistogramChunk {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::histogram_types::Span;

    fn sample_histogram() -> Histogram {
        Histogram {
            count: 10,
            zero_count: 2,
            sum: 18.4,
            schema: 3,
            zero_threshold: 2.938_735_877_055_719e-39,
            positive_spans: vec![Span {
                offset: 0,
                length: 2,
            }],
            negative_spans: vec![],
            positive_buckets: vec![4, 4],
            negative_buckets: vec![],
            custom_values: vec![],
            counter_reset_hint: CounterResetHint::UnknownCounterReset,
        }
    }

    #[test]
    fn test_single_histogram_sample() {
        let mut chunk = HistogramChunk::new();
        chunk.append(1000, &sample_histogram());

        let bytes = chunk.encode();
        assert_eq!(chunk.num_samples(), 1);
        assert!(bytes.len() > HISTOGRAM_HEADER_SIZE);

        // Check header
        assert_eq!(bytes[0], 0);
        assert_eq!(bytes[1], 1); // 1 sample
    }

    #[test]
    fn test_multiple_histogram_samples() {
        let mut chunk = HistogramChunk::new();

        let h1 = sample_histogram();
        chunk.append(1000, &h1);

        let h2 = Histogram {
            count: 15,
            zero_count: 3,
            sum: 28.0,
            ..h1.clone()
        };
        chunk.append(2000, &h2);

        let h3 = Histogram {
            count: 22,
            zero_count: 5,
            sum: 40.5,
            ..h1
        };
        chunk.append(3000, &h3);

        let bytes = chunk.encode();
        assert_eq!(chunk.num_samples(), 3);
        assert_eq!(bytes[0], 0);
        assert_eq!(bytes[1], 3); // 3 samples
    }
}
