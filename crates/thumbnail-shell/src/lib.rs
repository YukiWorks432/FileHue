//! Windowsのサムネイル生成をShellへ接続する。
#![allow(non_snake_case)]

#[cfg(windows)]
mod provider;

#[cfg(windows)]
pub use provider::{DllCanUnloadNow, DllGetClassObject};

#[cfg(windows)]
pub mod pdf;

#[cfg(windows)]
pub mod raster;

#[cfg(windows)]
pub mod stream;
