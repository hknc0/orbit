//! Message framing for length-prefixed protocol
//!
//! Handles reading and writing framed messages over streams.

#![allow(dead_code)] // Framing utilities for stream protocol

use std::io;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::game::constants::net::{MAX_DATAGRAM_SIZE, MAX_MESSAGE_SIZE};

/// Errors that can occur during message framing
#[derive(Debug, thiserror::Error)]
pub enum FramingError {
    #[error("Connection closed")]
    ConnectionClosed,
    #[error("Message too large: {0} bytes (max {1})")]
    MessageTooLarge(usize, usize),
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
}

/// Read a length-prefixed message from a stream
///
/// Format: [4 bytes little-endian length][payload]
pub async fn read_message<R: AsyncRead + Unpin>(stream: &mut R) -> Result<Vec<u8>, FramingError> {
    // Read 4-byte length prefix
    let mut len_buf = [0u8; 4];
    match stream.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
            return Err(FramingError::ConnectionClosed);
        }
        Err(e) => return Err(FramingError::Io(e)),
    }

    let len = u32::from_le_bytes(len_buf) as usize;

    // Validate length
    if len > MAX_MESSAGE_SIZE {
        return Err(FramingError::MessageTooLarge(len, MAX_MESSAGE_SIZE));
    }

    if len == 0 {
        return Ok(Vec::new());
    }

    // Read payload
    let mut buf = vec![0u8; len];
    match stream.read_exact(&mut buf).await {
        Ok(_) => Ok(buf),
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
            Err(FramingError::ConnectionClosed)
        }
        Err(e) => Err(FramingError::Io(e)),
    }
}

/// Write a length-prefixed message to a stream
///
/// Format: [4 bytes little-endian length][payload]
pub async fn write_message<W: AsyncWrite + Unpin>(
    stream: &mut W,
    data: &[u8],
) -> Result<(), FramingError> {
    if data.len() > MAX_MESSAGE_SIZE {
        return Err(FramingError::MessageTooLarge(data.len(), MAX_MESSAGE_SIZE));
    }

    // Write length prefix
    let len_bytes = (data.len() as u32).to_le_bytes();
    stream.write_all(&len_bytes).await?;

    // Write payload
    stream.write_all(data).await?;

    // Flush to ensure data is sent
    stream.flush().await?;

    Ok(())
}

/// Validate datagram size (for unreliable messages)
pub fn validate_datagram_size(data: &[u8]) -> Result<(), FramingError> {
    if data.len() > MAX_DATAGRAM_SIZE {
        Err(FramingError::MessageTooLarge(data.len(), MAX_DATAGRAM_SIZE))
    } else {
        Ok(())
    }
}

/// Frame builder for constructing messages
pub struct FrameBuilder {
    buffer: Vec<u8>,
}

impl FrameBuilder {
    pub fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(256),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buffer: Vec::with_capacity(capacity),
        }
    }

    /// Write data to the frame
    pub fn write(mut self, data: &[u8]) -> Self {
        self.buffer.extend_from_slice(data);
        self
    }

    /// Write a u8
    pub fn write_u8(mut self, value: u8) -> Self {
        self.buffer.push(value);
        self
    }

    /// Write a u16 (little-endian)
    pub fn write_u16(mut self, value: u16) -> Self {
        self.buffer.extend_from_slice(&value.to_le_bytes());
        self
    }

    /// Write a u32 (little-endian)
    pub fn write_u32(mut self, value: u32) -> Self {
        self.buffer.extend_from_slice(&value.to_le_bytes());
        self
    }

    /// Write a u64 (little-endian)
    pub fn write_u64(mut self, value: u64) -> Self {
        self.buffer.extend_from_slice(&value.to_le_bytes());
        self
    }

    /// Write a f32 (little-endian)
    pub fn write_f32(mut self, value: f32) -> Self {
        self.buffer.extend_from_slice(&value.to_le_bytes());
        self
    }

    /// Get the built frame
    pub fn build(self) -> Vec<u8> {
        self.buffer
    }

    /// Get the current length
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }
}

impl Default for FrameBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Frame reader for parsing messages
pub struct FrameReader<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> FrameReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, position: 0 }
    }

    /// Read n bytes
    pub fn read(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.position + n > self.data.len() {
            return None;
        }
        let slice = &self.data[self.position..self.position + n];
        self.position += n;
        Some(slice)
    }

    /// Read a u8
    pub fn read_u8(&mut self) -> Option<u8> {
        self.read(1).map(|b| b[0])
    }

    /// Read a u16 (little-endian)
    pub fn read_u16(&mut self) -> Option<u16> {
        self.read(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
    }

    /// Read a u32 (little-endian)
    pub fn read_u32(&mut self) -> Option<u32> {
        self.read(4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// Read a u64 (little-endian)
    pub fn read_u64(&mut self) -> Option<u64> {
        self.read(8).map(|b| {
            u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
        })
    }

    /// Read a f32 (little-endian)
    pub fn read_f32(&mut self) -> Option<f32> {
        self.read(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// Get remaining bytes
    pub fn remaining(&self) -> &'a [u8] {
        &self.data[self.position..]
    }

    /// Check if there are more bytes to read
    pub fn has_remaining(&self) -> bool {
        self.position < self.data.len()
    }

    /// Get current position
    pub fn position(&self) -> usize {
        self.position
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[tokio::test]
    async fn test_read_write_message() {
        let data = b"Hello, World!";
        let mut buffer = Vec::new();

        // Write message
        write_message(&mut buffer, data).await.unwrap();

        // Read message back
        let mut cursor = Cursor::new(buffer);
        let result = read_message(&mut cursor).await.unwrap();

        assert_eq!(result, data);
    }

    #[tokio::test]
    async fn test_empty_message() {
        let data = b"";
        let mut buffer = Vec::new();

        write_message(&mut buffer, data).await.unwrap();

        let mut cursor = Cursor::new(buffer);
        let result = read_message(&mut cursor).await.unwrap();

        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_message_too_large() {
        let large_data = vec![0u8; MAX_MESSAGE_SIZE + 1];
        let mut buffer = Vec::new();

        let result = write_message(&mut buffer, &large_data).await;
        assert!(matches!(result, Err(FramingError::MessageTooLarge(_, _))));
    }

    #[tokio::test]
    async fn test_read_truncated_length() {
        let data = vec![0u8; 2]; // Only 2 bytes, need 4 for length
        let mut cursor = Cursor::new(data);

        let result = read_message(&mut cursor).await;
        assert!(matches!(result, Err(FramingError::ConnectionClosed)));
    }

    #[tokio::test]
    async fn test_read_truncated_payload() {
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&10u32.to_le_bytes()); // Says 10 bytes
        buffer.extend_from_slice(&[1, 2, 3]); // Only 3 bytes

        let mut cursor = Cursor::new(buffer);
        let result = read_message(&mut cursor).await;
        assert!(matches!(result, Err(FramingError::ConnectionClosed)));
    }

    #[test]
    fn test_validate_datagram_size() {
        let small = vec![0u8; 100];
        assert!(validate_datagram_size(&small).is_ok());

        let large = vec![0u8; MAX_DATAGRAM_SIZE + 1];
        assert!(validate_datagram_size(&large).is_err());
    }

    #[test]
    fn test_frame_builder() {
        let frame = FrameBuilder::new()
            .write_u8(0x01)
            .write_u16(1234)
            .write_u32(56789)
            .write_f32(3.14)
            .write(b"test")
            .build();

        assert_eq!(frame.len(), 1 + 2 + 4 + 4 + 4);
    }

    #[test]
    fn test_frame_reader() {
        let data = FrameBuilder::new()
            .write_u8(42)
            .write_u16(1000)
            .write_u32(999999)
            .write_f32(2.5)
            .build();

        let mut reader = FrameReader::new(&data);

        assert_eq!(reader.read_u8(), Some(42));
        assert_eq!(reader.read_u16(), Some(1000));
        assert_eq!(reader.read_u32(), Some(999999));
        assert!((reader.read_f32().unwrap() - 2.5).abs() < 0.001);
        assert!(!reader.has_remaining());
    }

    #[test]
    fn test_frame_reader_overflow() {
        let data = vec![1, 2, 3];
        let mut reader = FrameReader::new(&data);

        assert!(reader.read_u8().is_some());
        assert!(reader.read_u8().is_some());
        assert!(reader.read_u8().is_some());
        assert!(reader.read_u8().is_none()); // No more data
    }

    #[tokio::test]
    async fn test_multiple_messages() {
        let messages = vec![
            b"First message".to_vec(),
            b"Second".to_vec(),
            b"Third message here".to_vec(),
        ];

        let mut buffer = Vec::new();
        for msg in &messages {
            write_message(&mut buffer, msg).await.unwrap();
        }

        let mut cursor = Cursor::new(buffer);
        for expected in &messages {
            let result = read_message(&mut cursor).await.unwrap();
            assert_eq!(&result, expected);
        }
    }
}
