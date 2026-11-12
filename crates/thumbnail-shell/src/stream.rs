//! Shellから渡されたIStreamを、範囲読み取り可能なRustの入力として扱う。

use std::io::{self, Read, Seek, SeekFrom};
use windows::Win32::System::Com::{IStream, STREAM_SEEK_CUR, STREAM_SEEK_END, STREAM_SEEK_SET};

pub struct StreamReader {
    stream: IStream,
}

impl StreamReader {
    pub fn new(stream: IStream) -> Self {
        Self { stream }
    }
}

impl Read for StreamReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let length = buffer.len().min(u32::MAX as usize) as u32;
        let mut read = 0u32;
        unsafe {
            self.stream
                .Read(buffer.as_mut_ptr().cast(), length, Some(&mut read))
                .ok()
                .map_err(io::Error::other)?;
        }
        if read > length {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid stream read length",
            ));
        }
        Ok(read as usize)
    }
}

impl Seek for StreamReader {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let (offset, origin) = match position {
            SeekFrom::Start(offset) => (
                i64::try_from(offset).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidInput, "stream offset too large")
                })?,
                STREAM_SEEK_SET,
            ),
            SeekFrom::Current(offset) => (offset, STREAM_SEEK_CUR),
            SeekFrom::End(offset) => (offset, STREAM_SEEK_END),
        };
        let mut next = 0u64;
        unsafe {
            self.stream
                .Seek(offset, origin, Some(&mut next))
                .map_err(io::Error::other)?;
        }
        Ok(next)
    }
}
