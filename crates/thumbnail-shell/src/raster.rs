//! 埋め込み画像をWindows標準デコーダーで縮小し、Shell向け画素へ変換する。

use windows::{
    Win32::{
        Foundation::{E_FAIL, E_INVALIDARG},
        Graphics::{
            Gdi::{
                BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateDIBSection, DIB_RGB_COLORS,
                DeleteObject, HBITMAP, HGDIOBJ,
            },
            Imaging::{
                CLSID_WICImagingFactory, CLSID_WICJpegDecoder, CLSID_WICPngDecoder,
                CLSID_WICTiffDecoder, GUID_WICPixelFormat24bppBGR, GUID_WICPixelFormat24bppRGB,
                GUID_WICPixelFormat32bppPBGRA, IWICBitmapDecoder, IWICBitmapSource,
                IWICImagingFactory, WICBitmapDitherTypeNone, WICBitmapInterpolationModeFant,
                WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
            },
        },
        System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, IStream},
        UI::Shell::SHCreateMemStream,
    },
    core::{Error, Result},
};

#[derive(Clone, Copy)]
pub enum Format {
    Jpeg,
    Png,
    Tiff,
}

/// 先頭行から順に、アルファ乗算済みのBGRAを保持する。
pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub bytes: Vec<u8>,
}

impl Pixels {
    /// Shellへ所有権を渡せる32ビットDIBを作る。
    pub fn to_bitmap(&self) -> Result<HBITMAP> {
        if self.width == 0
            || self.height == 0
            || self.width > 1024
            || self.height > 1024
            || self.bytes.len() != (self.width as usize) * (self.height as usize) * 4
        {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: self.width as i32,
                biHeight: -(self.height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                biSizeImage: self.bytes.len() as u32,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let bitmap = unsafe { CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0)? };
        if bits.is_null() {
            let _ = unsafe { DeleteObject(HGDIOBJ(bitmap.0)) };
            return Err(Error::from_hresult(E_FAIL));
        }
        unsafe {
            std::ptr::copy_nonoverlapping(self.bytes.as_ptr(), bits.cast(), self.bytes.len())
        };
        Ok(bitmap)
    }
}

/// 呼び出し元はCOMを初期化し、読み取り位置を先頭にしておく。
pub fn decode(input: &IStream, format: Format, max_side: u32) -> Result<Pixels> {
    if max_side == 0 || max_side > 1024 {
        return Err(Error::from_hresult(E_INVALIDARG));
    }
    let decoder_id = match format {
        Format::Jpeg => CLSID_WICJpegDecoder,
        Format::Png => CLSID_WICPngDecoder,
        Format::Tiff => CLSID_WICTiffDecoder,
    };
    // 文書の種類ごとにWindows標準のデコーダーを指定する。
    let decoder: IWICBitmapDecoder =
        unsafe { CoCreateInstance(&decoder_id, None, CLSCTX_INPROC_SERVER)? };
    unsafe { decoder.Initialize(input, WICDecodeMetadataCacheOnDemand)? };
    let frame = unsafe { decoder.GetFrame(0)? };
    convert(&frame, max_side)
}

/// Photoshopが保存したJPEGまたは非圧縮画像を表示用画素へ変換する。
pub fn photoshop(
    thumbnail: &file_icons_thumbnail_core::photoshop::Thumbnail,
    max_side: u32,
) -> Result<Pixels> {
    use file_icons_thumbnail_core::photoshop::{ColorOrder, ThumbnailData};
    match &thumbnail.data {
        ThumbnailData::Jpeg(data) => {
            if data.len() > 16 * 1024 * 1024 {
                return Err(Error::from_hresult(E_INVALIDARG));
            }
            let stream = unsafe { SHCreateMemStream(Some(data)) }
                .ok_or_else(|| Error::from_hresult(E_FAIL))?;
            let mut pixels = decode(&stream, Format::Jpeg, max_side)?;
            if thumbnail.color_order == ColorOrder::Bgr {
                for pixel in pixels.bytes.as_chunks_mut::<4>().0 {
                    pixel.swap(0, 2);
                }
            }
            Ok(pixels)
        }
        ThumbnailData::Raw { row_bytes, pixels } => {
            if thumbnail.width == 0
                || thumbnail.height == 0
                || thumbnail.width > 4096
                || thumbnail.height > 4096
                || u64::from(*row_bytes) < u64::from(thumbnail.width) * 3
                || u64::from(*row_bytes) * u64::from(thumbnail.height) != pixels.len() as u64
                || pixels.len() > 16 * 1024 * 1024
            {
                return Err(Error::from_hresult(E_INVALIDARG));
            }
            let format = match thumbnail.color_order {
                ColorOrder::Rgb => GUID_WICPixelFormat24bppRGB,
                ColorOrder::Bgr => GUID_WICPixelFormat24bppBGR,
            };
            let factory: IWICImagingFactory =
                unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)? };
            let source = unsafe {
                factory.CreateBitmapFromMemory(
                    thumbnail.width,
                    thumbnail.height,
                    &format,
                    *row_bytes,
                    pixels,
                )?
            };
            convert(&source, max_side)
        }
    }
}

fn convert(frame: &IWICBitmapSource, max_side: u32) -> Result<Pixels> {
    if max_side == 0 || max_side > 1024 {
        return Err(Error::from_hresult(E_INVALIDARG));
    }
    let (mut width, mut height) = (0, 0);
    unsafe { frame.GetSize(&mut width, &mut height)? };
    if width == 0 || height == 0 || width > 4096 || height > 4096 {
        return Err(Error::from_hresult(E_INVALIDARG));
    }
    let longest = width.max(height);
    if longest > max_side {
        width = ((u64::from(width) * u64::from(max_side) + u64::from(longest) / 2)
            / u64::from(longest))
        .max(1) as u32;
        height = ((u64::from(height) * u64::from(max_side) + u64::from(longest) / 2)
            / u64::from(longest))
        .max(1) as u32;
    }
    let factory: IWICImagingFactory =
        unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)? };
    let scaler = unsafe { factory.CreateBitmapScaler()? };
    unsafe { scaler.Initialize(frame, width, height, WICBitmapInterpolationModeFant)? };
    let converter = unsafe { factory.CreateFormatConverter()? };
    unsafe {
        converter.Initialize(
            &scaler,
            &GUID_WICPixelFormat32bppPBGRA,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeCustom,
        )?
    };
    let stride = width * 4;
    let mut bytes = vec![0; (stride * height) as usize];
    unsafe { converter.CopyPixels(std::ptr::null(), stride, &mut bytes)? };
    Ok(Pixels {
        width,
        height,
        bytes,
    })
}
