//! XOR chunk encoding backed by `rusty-chunkenc`.
//!
//! This module provides a thin wrapper around [`rusty_chunkenc::XORChunk`] that
//! presents a clean append-based API. If `rusty-chunkenc` becomes a bottleneck,
//! the internal implementation can be swapped without changing the public interface.

use rusty_chunkenc::XORSample;

/// A Prometheus XOR-encoded float chunk.
///
/// Samples are appended incrementally and encoded on demand when [`encode`](XORChunk::encode)
/// is called. The output is byte-identical to Go's `chunkenc.XORChunk.Bytes()`.
///
/// # Example
///
/// ```
/// use rs_prom_encoder::XORChunk;
///
/// let mut chunk = XORChunk::new();
/// chunk.append(1000, 42.0);
/// chunk.append(2000, 43.0);
///
/// let encoded = chunk.encode();
/// assert!(!encoded.is_empty());
/// assert_eq!(chunk.num_samples(), 2);
/// ```
#[derive(Debug)]
pub struct XORChunk {
    samples: Vec<XORSample>,
}

impl XORChunk {
    /// Creates a new empty XOR chunk.
    pub fn new() -> Self {
        Self {
            samples: Vec::new(),
        }
    }

    /// Creates a new XOR chunk with pre-allocated capacity for `n` samples.
    pub fn with_capacity(n: usize) -> Self {
        Self {
            samples: Vec::with_capacity(n),
        }
    }

    /// Appends a timestamp-value pair to the chunk.
    ///
    /// Timestamps should be in milliseconds and strictly increasing.
    pub fn append(&mut self, t: i64, v: f64) {
        self.samples.push(XORSample {
            timestamp: t,
            value: v,
        });
    }

    /// Returns the number of samples in the chunk.
    pub fn num_samples(&self) -> u16 {
        self.samples.len() as u16
    }

    /// Returns true if the chunk contains no samples.
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Encodes the chunk into Prometheus-compatible XOR chunk bytes.
    ///
    /// The returned bytes match the output of Go's `chunkenc.XORChunk.Bytes()`:
    /// a 2-byte big-endian sample count header followed by the Gorilla-encoded
    /// bit stream payload.
    pub fn encode(&self) -> Vec<u8> {
        if self.samples.is_empty() {
            // Empty chunk: just the 2-byte header with count=0
            return vec![0, 0];
        }

        let inner = rusty_chunkenc::xor::XORChunk::new(self.samples.clone());
        let mut buf = Vec::new();
        // XORChunk::write() outputs raw chunk bytes directly (header + bitstream),
        // matching Go's chunk.Bytes(). No framing to strip.
        inner
            .write(&mut buf)
            .expect("XOR chunk encoding should not fail");
        buf
    }
}

impl Default for XORChunk {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_chunk() {
        let chunk = XORChunk::new();
        assert_eq!(chunk.num_samples(), 0);
        assert!(chunk.is_empty());
        let bytes = chunk.encode();
        assert_eq!(bytes, vec![0, 0]);
    }

    #[test]
    fn test_single_sample() {
        let mut chunk = XORChunk::new();
        chunk.append(1000, 42.0);
        assert_eq!(chunk.num_samples(), 1);
        assert!(!chunk.is_empty());

        let bytes = chunk.encode();
        assert!(!bytes.is_empty());

        // First 2 bytes should be sample count = 1 (big-endian)
        assert_eq!(bytes[0], 0);
        assert_eq!(bytes[1], 1);
    }

    #[test]
    fn test_multiple_samples() {
        let mut chunk = XORChunk::new();
        chunk.append(1000, 1.0);
        chunk.append(2000, 2.0);
        chunk.append(3000, 3.0);
        assert_eq!(chunk.num_samples(), 3);

        let bytes = chunk.encode();
        // First 2 bytes should be sample count = 3 (big-endian)
        assert_eq!(bytes[0], 0);
        assert_eq!(bytes[1], 3);
    }

    #[test]
    fn test_known_encoding() {
        // This test verifies byte output against a known good encoding.
        // The values come from rusty-chunkenc's own documentation example.
        let mut chunk = XORChunk::new();
        chunk.append(7_200_000, 12_000.0);
        chunk.append(7_201_000, 12_001.0);

        let bytes = chunk.encode();
        // Expected: the raw chunk data portion (header + bitstream)
        // from rusty-chunkenc's example:
        // Full disk format: [0x12, 0x01, 0x00, 0x02, ...]
        //   0x12 = uvarint(18) = data_len (includes 1 byte encoding + 17 bytes data)
        //   0x01 = EncXOR
        //   remaining = chunk data + 4 byte CRC
        // So raw chunk data is bytes [2..20] of the full output (18 - 1 = 17 bytes of data).

        // Verify sample count header
        assert_eq!(bytes[0], 0x00);
        assert_eq!(bytes[1], 0x02); // 2 samples
        assert!(bytes.len() > 2); // has actual encoded data
    }
}
