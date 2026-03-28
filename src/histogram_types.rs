/// A histogram span, representing a contiguous range of populated buckets.
///
/// Matches Prometheus `model/histogram.Span`.
#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    /// Offset from the previous span (or absolute bucket index for the first span).
    pub offset: i32,
    /// Number of consecutive populated buckets in this span.
    pub length: u32,
}

/// Counter reset hint stored in the histogram chunk flags byte.
///
/// Matches Prometheus `tsdb/chunkenc.CounterResetHeader`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CounterResetHint {
    /// Unknown whether a counter reset occurred.
    UnknownCounterReset = 0b0000_0000,
    /// A definite counter reset occurred.
    CounterReset = 0b1000_0000,
    /// Definitely no counter reset.
    NotCounterReset = 0b0100_0000,
    /// This is a gauge-type histogram (counter resets don't apply).
    GaugeType = 0b1100_0000,
}

/// Mask for the counter reset header bits in the flags byte.
pub const COUNTER_RESET_HEADER_MASK: u8 = 0b1100_0000;

/// A native integer histogram sample.
///
/// Matches Prometheus `model/histogram.Histogram`.
#[derive(Clone, Debug, PartialEq)]
pub struct Histogram {
    /// Total number of observations.
    pub count: u64,
    /// Number of observations in the zero bucket.
    pub zero_count: u64,
    /// Sum of all observed values.
    pub sum: f64,
    /// Resolution schema (exponential bucket layout parameter).
    pub schema: i32,
    /// Width of the zero bucket.
    pub zero_threshold: f64,
    /// Positive bucket spans.
    pub positive_spans: Vec<Span>,
    /// Negative bucket spans.
    pub negative_spans: Vec<Span>,
    /// Positive bucket deltas (delta-encoded counts).
    pub positive_buckets: Vec<i64>,
    /// Negative bucket deltas (delta-encoded counts).
    pub negative_buckets: Vec<i64>,
    /// Custom bucket boundaries (only for `CUSTOM_BUCKETS_SCHEMA`).
    pub custom_values: Vec<f64>,
    /// Counter reset hint for this sample.
    pub counter_reset_hint: CounterResetHint,
}

/// A native float histogram sample.
///
/// Matches Prometheus `model/histogram.FloatHistogram`.
#[derive(Clone, Debug, PartialEq)]
pub struct FloatHistogram {
    /// Total number of observations (as float).
    pub count: f64,
    /// Number of observations in the zero bucket (as float).
    pub zero_count: f64,
    /// Sum of all observed values.
    pub sum: f64,
    /// Resolution schema (exponential bucket layout parameter).
    pub schema: i32,
    /// Width of the zero bucket.
    pub zero_threshold: f64,
    /// Positive bucket spans.
    pub positive_spans: Vec<Span>,
    /// Negative bucket spans.
    pub negative_spans: Vec<Span>,
    /// Positive bucket absolute counts.
    pub positive_buckets: Vec<f64>,
    /// Negative bucket absolute counts.
    pub negative_buckets: Vec<f64>,
    /// Custom bucket boundaries (only for `CUSTOM_BUCKETS_SCHEMA`).
    pub custom_values: Vec<f64>,
    /// Counter reset hint for this sample.
    pub counter_reset_hint: CounterResetHint,
}

/// Schema value indicating custom bucket boundaries.
pub const CUSTOM_BUCKETS_SCHEMA: i32 = -53;

/// Bit pattern for Prometheus stale NaN marker.
///
/// This is distinct from the standard IEEE 754 NaN. Prometheus uses this
/// specific bit pattern to mark series as stale.
pub const STALE_NAN_BITS: u64 = 0x7ff0_0000_0000_0002;
