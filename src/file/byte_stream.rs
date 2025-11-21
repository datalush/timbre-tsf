use crate::error::{Result, TimbreError};
use std::io::{Read, Write};

/// Buffer dinámico con capacidad de lectura/escritura
/// Similar al ByteStream de C++
#[derive(Debug, Clone)]
pub struct ByteStream {
    buffer: Vec<u8>,
    read_pos: usize,
    write_pos: usize,
}

impl ByteStream {
    /// Crea un nuevo ByteStream vacío
    pub fn new() -> Self {
        Self {
            buffer: Vec::new(),
            read_pos: 0,
            write_pos: 0,
        }
    }

    /// Crea un ByteStream con capacidad inicial
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buffer: Vec::with_capacity(capacity),
            read_pos: 0,
            write_pos: 0,
        }
    }

    /// Crea un ByteStream desde un buffer existente
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        let len = bytes.len();
        Self {
            buffer: bytes,
            read_pos: 0,
            write_pos: len,
        }
    }

    /// Escribe bytes al stream
    pub fn write_bytes(&mut self, data: &[u8]) -> Result<()> {
        self.buffer.extend_from_slice(data);
        self.write_pos += data.len();
        Ok(())
    }

    /// Lee bytes del stream
    pub fn read_bytes(&mut self, len: usize) -> Result<Vec<u8>> {
        if self.read_pos + len > self.write_pos {
            return Err(TimbreError::UnexpectedEof);
        }
        let data = self.buffer[self.read_pos..self.read_pos + len].to_vec();
        self.read_pos += len;
        Ok(data)
    }

    /// Copia bytes sin avanzar la posición de lectura
    pub fn peek_bytes(&self, len: usize) -> Result<Vec<u8>> {
        if self.read_pos + len > self.write_pos {
            return Err(TimbreError::UnexpectedEof);
        }
        Ok(self.buffer[self.read_pos..self.read_pos + len].to_vec())
    }

    /// Obtiene un slice de los datos escritos
    pub fn as_slice(&self) -> &[u8] {
        &self.buffer[..self.write_pos]
    }

    /// Obtiene un slice mutable
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.buffer[..self.write_pos]
    }

    /// Tamaño total de datos escritos
    pub fn len(&self) -> usize {
        self.write_pos
    }

    /// Verifica si está vacío
    pub fn is_empty(&self) -> bool {
        self.write_pos == 0
    }

    /// Bytes disponibles para lectura
    pub fn available(&self) -> usize {
        self.write_pos.saturating_sub(self.read_pos)
    }

    /// Resetea el stream
    pub fn reset(&mut self) {
        self.buffer.clear();
        self.read_pos = 0;
        self.write_pos = 0;
    }

    /// Resetea solo la posición de lectura
    pub fn reset_read_pos(&mut self) {
        self.read_pos = 0;
    }

    /// Obtiene la posición actual de lectura
    pub fn read_position(&self) -> usize {
        self.read_pos
    }

    /// Establece la posición de lectura
    pub fn set_read_position(&mut self, pos: usize) -> Result<()> {
        if pos > self.write_pos {
            return Err(TimbreError::InvalidState(
                "Read position beyond write position".to_string(),
            ));
        }
        self.read_pos = pos;
        Ok(())
    }

    /// Consume el stream y retorna el buffer interno
    pub fn into_vec(self) -> Vec<u8> {
        let mut buffer = self.buffer;
        buffer.truncate(self.write_pos);
        buffer
    }

    /// Clona el contenido como Vec
    pub fn to_vec(&self) -> Vec<u8> {
        self.buffer[..self.write_pos].to_vec()
    }
}

impl Default for ByteStream {
    fn default() -> Self {
        Self::new()
    }
}

impl Write for ByteStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.write_bytes(buf)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Read for ByteStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let available = self.available();
        if available == 0 {
            return Ok(0);
        }

        let to_read = available.min(buf.len());
        buf[..to_read].copy_from_slice(&self.buffer[self.read_pos..self.read_pos + to_read]);
        self.read_pos += to_read;
        Ok(to_read)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_byte_stream_write_read() {
        let mut stream = ByteStream::new();
        stream.write_bytes(b"Hello").unwrap();
        stream.write_bytes(b"World").unwrap();

        assert_eq!(stream.len(), 10);
        assert_eq!(stream.available(), 10);

        stream.reset_read_pos();
        let data = stream.read_bytes(5).unwrap();
        assert_eq!(&data, b"Hello");

        let data = stream.read_bytes(5).unwrap();
        assert_eq!(&data, b"World");
    }

    #[test]
    fn test_byte_stream_peek() {
        let mut stream = ByteStream::new();
        stream.write_bytes(b"Test").unwrap();

        let peeked = stream.peek_bytes(4).unwrap();
        assert_eq!(&peeked, b"Test");
        assert_eq!(stream.read_position(), 0); // No avanzó

        let read = stream.read_bytes(4).unwrap();
        assert_eq!(&read, b"Test");
        assert_eq!(stream.read_position(), 4); // Sí avanzó
    }

    #[test]
    fn test_byte_stream_as_slice() {
        let mut stream = ByteStream::new();
        stream.write_bytes(b"Data").unwrap();

        assert_eq!(stream.as_slice(), b"Data");
    }
}
