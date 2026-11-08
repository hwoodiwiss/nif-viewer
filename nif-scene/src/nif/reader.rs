//! Little-endian byte cursor with graceful error handling (no panics).

use std::fmt;

/// Errors produced by the NIF/.mesh parsers.
#[derive(Debug)]
pub enum NifError {
    /// Ran out of bytes: needed `needed` more at offset `at`.
    UnexpectedEof { needed: usize, at: usize },
    /// Header magic string not recognised.
    InvalidMagic(String),
    /// NIF version not supported.
    UnsupportedVersion(u32),
    /// Malformed data with a description.
    Invalid(String),
}

impl fmt::Display for NifError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NifError::UnexpectedEof { needed, at } => {
                write!(f, "unexpected EOF: needed {} bytes at offset {}", needed, at)
            }
            NifError::InvalidMagic(s) => write!(f, "invalid header magic: {:?}", s),
            NifError::UnsupportedVersion(v) => write!(f, "unsupported NIF version 0x{:08X}", v),
            NifError::Invalid(msg) => write!(f, "malformed data: {}", msg),
        }
    }
}

impl std::error::Error for NifError {}

pub type Result<T> = std::result::Result<T, NifError>;

/// Little-endian cursor over a byte slice.
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Reader { data, pos: 0 }
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    pub fn is_eof(&self) -> bool {
        self.pos >= self.data.len()
    }

    pub fn seek(&mut self, pos: usize) -> Result<()> {
        if pos > self.data.len() {
            return Err(NifError::UnexpectedEof {
                needed: pos - self.data.len(),
                at: self.pos,
            });
        }
        self.pos = pos;
        Ok(())
    }

    pub fn skip(&mut self, n: usize) -> Result<()> {
        let new_pos = self
            .pos
            .checked_add(n)
            .ok_or(NifError::Invalid("skip overflow".to_string()))?;
        self.seek(new_pos)
    }

    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.remaining() < n {
            return Err(NifError::UnexpectedEof {
                needed: n - self.remaining(),
                at: self.pos,
            });
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.bytes(1)?[0])
    }

    pub fn u16(&mut self) -> Result<u16> {
        let b = self.bytes(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    pub fn i16(&mut self) -> Result<i16> {
        Ok(self.u16()? as i16)
    }

    pub fn u32(&mut self) -> Result<u32> {
        let b = self.bytes(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn i32(&mut self) -> Result<i32> {
        Ok(self.u32()? as i32)
    }

    pub fn u64(&mut self) -> Result<u64> {
        let b = self.bytes(8)?;
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    pub fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_bits(self.u32()?))
    }

    /// IEEE half-precision float, widened to f32.
    pub fn f16(&mut self) -> Result<f32> {
        Ok(half::f16::from_bits(self.u16()?).to_f32())
    }

    /// u32 length + bytes (no NUL).
    pub fn sized_string(&mut self) -> Result<String> {
        let len = self.u32()? as usize;
        if len > self.remaining() {
            return Err(NifError::UnexpectedEof {
                needed: len - self.remaining(),
                at: self.pos,
            });
        }
        let b = self.bytes(len)?;
        Ok(String::from_utf8_lossy(b).into_owned())
    }

    /// u8 length + bytes including trailing NUL.
    pub fn export_string(&mut self) -> Result<String> {
        let len = self.u8()? as usize;
        let b = self.bytes(len)?;
        let b = if b.last() == Some(&0) { &b[..len - 1] } else { b };
        Ok(String::from_utf8_lossy(b).into_owned())
    }

    /// Read a line terminated by 0x0A (newline not included in result).
    pub fn line(&mut self) -> Result<String> {
        let start = self.pos;
        while self.pos < self.data.len() {
            if self.data[self.pos] == 0x0A {
                let s = String::from_utf8_lossy(&self.data[start..self.pos]).into_owned();
                self.pos += 1;
                return Ok(s);
            }
            self.pos += 1;
        }
        Err(NifError::UnexpectedEof { needed: 1, at: start })
    }
}
