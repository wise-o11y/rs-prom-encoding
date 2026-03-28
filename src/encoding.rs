/// Prometheus chunk encoding types.
///
/// These match the constants defined in Prometheus `tsdb/chunkenc/chunk.go`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Encoding {
    /// No encoding (unused).
    None = 0,
    /// Legacy Gorilla XOR float encoding.
    XOR = 1,
    /// Native integer histogram encoding.
    Histogram = 2,
    /// Native float histogram encoding.
    FloatHistogram = 3,
}

impl Encoding {
    /// Returns true if the encoding is valid for chunk creation.
    pub fn is_valid(self) -> bool {
        matches!(self, Self::XOR | Self::Histogram | Self::FloatHistogram)
    }
}

impl std::fmt::Display for Encoding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => write!(f, "none"),
            Self::XOR => write!(f, "XOR"),
            Self::Histogram => write!(f, "histogram"),
            Self::FloatHistogram => write!(f, "float_histogram"),
        }
    }
}

/// Maximum bytes per XOR chunk before a new chunk should be started.
pub const MAX_BYTES_PER_XOR_CHUNK: usize = 1024;

/// Target bytes per histogram chunk.
pub const TARGET_BYTES_PER_HISTOGRAM_CHUNK: usize = 1024;

/// Minimum samples per histogram chunk (to avoid very small chunks).
pub const MIN_SAMPLES_PER_HISTOGRAM_CHUNK: usize = 10;
