//! Bit-level stream writer and reader, matching Prometheus `bstream` semantics.
//!
//! Bits are written MSB-first (left-to-right within each byte). This matches
//! the Go implementation exactly for byte-level compatibility.

/// Bit-level stream writer.
///
/// Writes bits MSB-first into an expanding byte buffer. The `count` field
/// tracks how many bits are still available for writing in the last byte
/// (0 means byte-aligned, no partial byte).
pub struct BStreamWriter {
    stream: Vec<u8>,
    count: u8,
}

impl BStreamWriter {
    /// Creates a new empty writer.
    pub fn new() -> Self {
        Self {
            stream: Vec::new(),
            count: 0,
        }
    }

    /// Creates a new writer with pre-allocated byte capacity.
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            stream: Vec::with_capacity(cap),
            count: 0,
        }
    }

    /// Write a single bit. `true` = 1, `false` = 0.
    pub fn write_bit(&mut self, bit: bool) {
        if self.count == 0 {
            self.stream.push(0);
            self.count = 8;
        }
        if bit {
            let last = self.stream.len() - 1;
            self.stream[last] |= 1 << (self.count - 1);
        }
        self.count -= 1;
    }

    /// Write a full byte, respecting current bit alignment.
    pub fn write_byte(&mut self, byt: u8) {
        if self.count == 0 {
            self.stream.push(byt);
            return;
        }
        // Split the byte across the current partial byte and a new byte.
        let last = self.stream.len() - 1;
        self.stream[last] |= byt >> (8 - self.count);
        self.stream.push(byt << self.count);
    }

    /// Write the `nbits` least-significant bits of `u` (MSB-first ordering).
    ///
    /// `nbits` must be in the range 0..=64.
    pub fn write_bits(&mut self, u: u64, nbits: u8) {
        if nbits == 0 {
            return;
        }

        // Left-align the value so the bits-to-write are at the MSB end.
        // For nbits=64, the shift is 0.
        let mut val = if nbits == 64 { u } else { u << (64 - nbits) };
        let mut remaining = nbits;

        while remaining >= 8 {
            self.write_byte((val >> 56) as u8);
            val <<= 8;
            remaining -= 8;
        }

        while remaining > 0 {
            self.write_bit(val & (1 << 63) != 0);
            val <<= 1;
            remaining -= 1;
        }
    }

    /// Returns the current byte buffer as a slice.
    pub fn bytes(&self) -> &[u8] {
        &self.stream
    }

    /// Consumes the writer and returns the byte buffer.
    pub fn into_bytes(self) -> Vec<u8> {
        self.stream
    }

    /// Returns the number of bytes in the buffer.
    pub fn len(&self) -> usize {
        self.stream.len()
    }

    /// Returns true if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.stream.is_empty()
    }

    /// Returns a mutable reference to the underlying byte buffer.
    ///
    /// Use with care — modifying the buffer directly can break bit alignment.
    pub fn stream_mut(&mut self) -> &mut Vec<u8> {
        &mut self.stream
    }
}

impl Default for BStreamWriter {
    fn default() -> Self {
        Self::new()
    }
}

/// Bit-level stream reader.
///
/// Reads bits MSB-first from a byte buffer. Uses a 64-bit internal buffer
/// for efficient multi-bit reads.
pub struct BStreamReader {
    stream: Vec<u8>,
    stream_offset: usize,
    buffer: u64,
    valid: u8,
    last: u8,
}

impl BStreamReader {
    /// Creates a new reader from a byte slice.
    ///
    /// Snapshots the last byte for concurrent-write safety (matching Go behavior).
    pub fn new(data: &[u8]) -> Self {
        let last = if data.is_empty() {
            0
        } else {
            data[data.len() - 1]
        };
        Self {
            stream: data.to_vec(),
            stream_offset: 0,
            buffer: 0,
            valid: 0,
            last,
        }
    }

    /// Read a single bit. Returns `true` for 1, `false` for 0.
    pub fn read_bit(&mut self) -> Result<bool, BStreamError> {
        if self.valid == 0 && !self.load_next_buffer() {
            return Err(BStreamError::Eof);
        }
        self.valid -= 1;
        // Bits are stored left-aligned in buffer: bit at position (63 - bits_already_consumed)
        // After decrementing valid, the bit we want is at position `valid` from the right,
        // i.e., the MSB of the remaining valid bits.
        Ok(self.buffer & (1u64 << self.valid) != 0)
    }

    /// Read `nbits` bits and return them as the least-significant bits of a u64.
    pub fn read_bits(&mut self, nbits: u8) -> Result<u64, BStreamError> {
        if nbits == 0 {
            return Ok(0);
        }

        if nbits <= self.valid {
            self.valid -= nbits;
            let mask = if nbits == 64 {
                u64::MAX
            } else {
                (1u64 << nbits) - 1
            };
            return Ok((self.buffer >> self.valid) & mask);
        }

        // Need to span a buffer load.
        let mut remaining = nbits;
        let mut v: u64 = 0;

        if self.valid > 0 {
            let mask = if self.valid == 64 {
                u64::MAX
            } else {
                (1u64 << self.valid) - 1
            };
            v = self.buffer & mask;
            remaining -= self.valid;
            self.valid = 0;
        }

        if !self.load_next_buffer() {
            return Err(BStreamError::Eof);
        }

        self.valid -= remaining;
        if remaining < 64 {
            v <<= remaining;
        } else {
            v = 0; // v was 0 anyway (no prior valid bits)
        }
        let mask = if remaining == 64 {
            u64::MAX
        } else {
            (1u64 << remaining) - 1
        };
        v |= (self.buffer >> self.valid) & mask;

        Ok(v)
    }

    /// Read a single byte.
    pub fn read_byte(&mut self) -> Result<u8, BStreamError> {
        self.read_bits(8).map(|v| v as u8)
    }

    /// Load the next chunk of bytes into the internal buffer.
    ///
    /// Convention: bits are packed so that the first-to-read bit is at position
    /// `valid - 1` of the buffer. This matches Go's bstreamReader.
    fn load_next_buffer(&mut self) -> bool {
        if self.stream_offset >= self.stream.len() {
            return false;
        }

        let remaining = self.stream.len() - self.stream_offset;

        if self.stream_offset + 8 < self.stream.len() {
            // Fast path: read 8 bytes as big-endian u64 (never touches the last byte).
            let bytes = &self.stream[self.stream_offset..self.stream_offset + 8];
            self.buffer = u64::from_be_bytes(bytes.try_into().unwrap());
            self.stream_offset += 8;
            self.valid = 64;
        } else {
            // Slow path: read remaining bytes, substituting last byte snapshot.
            // Pack bytes big-endian style: first byte in highest position.
            let nbytes = remaining.min(8);
            self.buffer = 0;
            for i in 0..nbytes {
                let b = if self.stream_offset + i == self.stream.len() - 1 {
                    self.last
                } else {
                    self.stream[self.stream_offset + i]
                };
                self.buffer = (self.buffer << 8) | b as u64;
            }
            self.stream_offset += nbytes;
            self.valid = (nbytes * 8) as u8;
        }

        true
    }
}

/// Errors from bit stream operations.
#[derive(Debug, PartialEq, Eq)]
pub enum BStreamError {
    /// Attempted to read past the end of the stream.
    Eof,
}

impl std::fmt::Display for BStreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Eof => write!(f, "unexpected end of stream"),
        }
    }
}

impl std::error::Error for BStreamError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_read_bits_roundtrip() {
        let mut w = BStreamWriter::new();
        w.write_bits(0b1101, 4);
        w.write_bits(0b10, 2);
        w.write_bits(0xFF, 8);
        w.write_bits(0, 2);

        let bytes = w.bytes();
        let mut r = BStreamReader::new(bytes);

        assert_eq!(r.read_bits(4).unwrap(), 0b1101);
        assert_eq!(r.read_bits(2).unwrap(), 0b10);
        assert_eq!(r.read_bits(8).unwrap(), 0xFF);
        assert_eq!(r.read_bits(2).unwrap(), 0);
    }

    #[test]
    fn test_write_read_single_bits() {
        let mut w = BStreamWriter::new();
        w.write_bit(true);
        w.write_bit(false);
        w.write_bit(true);
        w.write_bit(true);

        let bytes = w.bytes();
        // First byte should be 0b1011_0000
        assert_eq!(bytes[0], 0b1011_0000);

        let mut r = BStreamReader::new(bytes);
        assert!(r.read_bit().unwrap());
        assert!(!r.read_bit().unwrap());
        assert!(r.read_bit().unwrap());
        assert!(r.read_bit().unwrap());
    }

    #[test]
    fn test_write_byte_aligned() {
        let mut w = BStreamWriter::new();
        w.write_byte(0xAB);
        w.write_byte(0xCD);

        assert_eq!(w.bytes(), &[0xAB, 0xCD]);
    }

    #[test]
    fn test_write_byte_unaligned() {
        let mut w = BStreamWriter::new();
        w.write_bit(true); // 1 bit used in first byte
        w.write_byte(0xFF);

        // First byte: 1_1111111 = 0xFF
        // Second byte: 1_0000000 = 0x80
        assert_eq!(w.bytes(), &[0xFF, 0x80]);
    }

    #[test]
    fn test_write_64_bits() {
        let mut w = BStreamWriter::new();
        let val: u64 = 0x4037000000000000; // f64::to_bits(23.0)
        w.write_bits(val, 64);

        assert_eq!(w.bytes().len(), 8);
        let mut r = BStreamReader::new(w.bytes());
        assert_eq!(r.read_bits(64).unwrap(), val);
    }

    #[test]
    fn test_write_read_mixed() {
        let mut w = BStreamWriter::new();
        w.write_bit(true);
        w.write_bits(0b110, 3);
        w.write_byte(0xAB);
        w.write_bits(0b01, 2);

        let mut r = BStreamReader::new(w.bytes());
        assert!(r.read_bit().unwrap());
        assert_eq!(r.read_bits(3).unwrap(), 0b110);
        assert_eq!(r.read_bits(8).unwrap(), 0xAB);
        assert_eq!(r.read_bits(2).unwrap(), 0b01);
    }
}
