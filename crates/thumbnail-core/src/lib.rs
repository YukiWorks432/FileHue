//! 文書中のプレビューを、文書全体を読み込まずに取り出す。

use std::io;

pub mod eps;
pub mod format;
pub mod photoshop;
#[cfg(feature = "preview")]
pub mod svg;
#[cfg(feature = "preview")]
pub mod xmp;

/// アルファ乗算済みRGBA画像。
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Invalid,
    Limit,
    Unsupported,
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Invalid => formatter.write_str("invalid thumbnail data"),
            Self::Limit => formatter.write_str("thumbnail data exceeds limits"),
            Self::Unsupported => formatter.write_str("unsupported thumbnail data"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}
