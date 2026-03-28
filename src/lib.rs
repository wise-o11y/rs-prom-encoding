//! # rs-prom-encoder
//!
//! Prometheus-compatible chunk encoder for XOR float and native histogram time series.
//!
//! This crate produces chunk binaries that are **byte-identical** to those produced by
//! Prometheus [`tsdb/chunkenc`](https://pkg.go.dev/github.com/prometheus/prometheus/tsdb/chunkenc),
//! ensuring chunks encoded in Rust can be read by any Go-based Prometheus ecosystem tool
//! (Prometheus, Thanos, Mimir, etc.).
//!
//! ## Supported Encodings
//!
//! | Encoding | Description |
//! |----------|-------------|
//! | `EncXOR` (1) | Gorilla-based XOR float chunk encoding |
//! | `EncHistogram` (2) | Native integer histogram chunk encoding |
//! | `EncFloatHistogram` (3) | Native float histogram chunk encoding |
//!
//! ## Quick Start
//!
//! ### XOR Float Chunks
//!
//! ```rust
//! use rs_prom_encoder::XORChunk;
//!
//! let mut chunk = XORChunk::new();
//! chunk.append(1000, 1.5);
//! chunk.append(2000, 2.5);
//! chunk.append(3000, 3.5);
//!
//! let bytes = chunk.encode();
//! // `bytes` is Prometheus-compatible chunk data
//! ```
//!
//! ### Native Histogram Chunks
//!
//! ```rust,no_run
//! use rs_prom_encoder::{HistogramChunk, Histogram, Span};
//!
//! let mut chunk = HistogramChunk::new();
//! chunk.append(1000, &Histogram {
//!     count: 10,
//!     zero_count: 2,
//!     sum: 18.4,
//!     schema: 3,
//!     zero_threshold: 2.938735877055719e-39, // default 2^-128
//!     positive_spans: vec![Span { offset: 0, length: 2 }],
//!     negative_spans: vec![],
//!     positive_buckets: vec![4, 4],
//!     negative_buckets: vec![],
//!     custom_values: vec![],
//!     counter_reset_hint: rs_prom_encoder::CounterResetHint::UnknownCounterReset,
//! });
//!
//! let bytes = chunk.encode();
//! ```

// Modules - low-level primitives (allow dead_code: read path is for tests/future use)
#[allow(dead_code)]
mod bstream;
#[allow(dead_code)]
mod encoding;
#[allow(dead_code)]
mod varbit;
#[allow(dead_code)]
mod xor;

// Modules - types
#[allow(dead_code)]
mod histogram_types;

// Modules - chunk encoders
#[allow(dead_code)]
mod histogram_meta;
mod xor_chunk;

// Histogram chunk encoders (depend on bstream, varbit, xor, histogram_meta)
mod float_histogram_chunk;
mod histogram_chunk;

// Public re-exports
pub use encoding::Encoding;
pub use float_histogram_chunk::FloatHistogramChunk;
pub use histogram_chunk::HistogramChunk;
pub use histogram_types::{
    CounterResetHint, FloatHistogram, Histogram, Span, CUSTOM_BUCKETS_SCHEMA, STALE_NAN_BITS,
};
pub use xor_chunk::XORChunk;

/// Check if a float64 value is the Prometheus stale NaN marker.
pub fn is_stale_nan(v: f64) -> bool {
    v.to_bits() == STALE_NAN_BITS
}

/// Return the Prometheus stale NaN marker as a float64.
pub fn stale_nan() -> f64 {
    f64::from_bits(STALE_NAN_BITS)
}
