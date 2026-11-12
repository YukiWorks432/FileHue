//! サムネイル用COMクラス。ファイル名や既定アプリの登録は変更しない。

use crate::{pdf, raster, stream::StreamReader};
use file_icons_thumbnail_core::format::Format;
use std::{
    cell::RefCell,
    ffi::c_void,
    sync::atomic::{AtomicUsize, Ordering},
};
use windows::{
    Win32::{
        Foundation::*,
        Graphics::Gdi::HBITMAP,
        System::{
            Com::*,
            WinRT::{
                CreateStreamOverRandomAccessStream, RO_INIT_MULTITHREADED, RO_INIT_SINGLETHREADED,
                RoInitialize, RoUninitialize,
            },
        },
        UI::Shell::{
            IThumbnailProvider, IThumbnailProvider_Impl,
            PropertiesSystem::{IInitializeWithStream, IInitializeWithStream_Impl},
            SHCreateMemStream, WTS_ALPHATYPE, WTSAT_ARGB, WTSAT_UNKNOWN,
        },
    },
    core::*,
};

static OBJECTS: AtomicUsize = AtomicUsize::new(0);
static LOCKS: AtomicUsize = AtomicUsize::new(0);

// WinRTが完了通知を保持している間は、通知用のCOM実装もDLL内に残す。
pub(crate) struct PendingCallback;

impl PendingCallback {
    pub(crate) fn new() -> Self {
        OBJECTS.fetch_add(1, Ordering::Relaxed);
        Self
    }
}

impl Drop for PendingCallback {
    fn drop(&mut self) {
        OBJECTS.fetch_sub(1, Ordering::Relaxed);
    }
}

#[implement(IInitializeWithStream, IThumbnailProvider)]
struct Provider {
    format: Format,
    input: RefCell<Option<IStream>>,
}

impl Provider {
    fn new(format: Format) -> Self {
        OBJECTS.fetch_add(1, Ordering::Relaxed);
        Self {
            format,
            input: RefCell::new(None),
        }
    }
}

impl Drop for Provider {
    fn drop(&mut self) {
        OBJECTS.fetch_sub(1, Ordering::Relaxed);
    }
}

impl IInitializeWithStream_Impl for Provider_Impl {
    fn Initialize(&self, input: Ref<IStream>, mode: u32) -> Result<()> {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if mode & 3 != STGM_READ.0 {
                return Err(Error::from_hresult(E_INVALIDARG));
            }
            let mut saved = self.input.borrow_mut();
            if saved.is_some() {
                return Err(Error::from_hresult(HRESULT(0x800704DFu32 as i32)));
            }
            *saved = Some(input.ok()?.clone());
            Ok(())
        }))
        .unwrap_or_else(|_| Err(Error::from_hresult(E_FAIL)))
    }
}

impl IThumbnailProvider_Impl for Provider_Impl {
    fn GetThumbnail(
        &self,
        size: u32,
        bitmap: *mut HBITMAP,
        alpha: *mut WTS_ALPHATYPE,
    ) -> Result<()> {
        if bitmap.is_null() || alpha.is_null() {
            return Err(Error::from_hresult(E_POINTER));
        }
        unsafe {
            *bitmap = HBITMAP::default();
            *alpha = WTSAT_UNKNOWN;
        }
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if size == 0 {
                return Err(Error::from_hresult(E_INVALIDARG));
            }
            let input = self
                .input
                .borrow()
                .clone()
                .ok_or_else(|| Error::from_hresult(E_UNEXPECTED))?;
            let pixels = render(self.format, input, size.min(1024))?;
            let handle = pixels.to_bitmap()?;
            unsafe {
                *bitmap = handle;
                *alpha = WTSAT_ARGB;
            }
            Ok(())
        }))
        .unwrap_or_else(|_| Err(Error::from_hresult(E_FAIL)))
    }
}

fn render(format: Format, input: IStream, size: u32) -> Result<raster::Pixels> {
    unsafe { input.Seek(0, STREAM_SEEK_SET, None)? };
    match format {
        Format::Psd | Format::Psb => {
            let mut stream = StreamReader::new(input);
            let thumbnail = file_icons_thumbnail_core::photoshop::extract(&mut stream)
                .map_err(|_| Error::from_hresult(E_FAIL))?
                .ok_or_else(|| Error::from_hresult(E_FAIL))?;
            let expected = match format {
                Format::Psd => file_icons_thumbnail_core::photoshop::DocumentVersion::Psd,
                Format::Psb => file_icons_thumbnail_core::photoshop::DocumentVersion::Psb,
                _ => unreachable!(),
            };
            if thumbnail.version != expected {
                return Err(Error::from_hresult(E_FAIL));
            }
            raster::photoshop(&thumbnail, size)
        }
        Format::Eps => {
            let mut stream = StreamReader::new(input);
            match file_icons_thumbnail_core::eps::extract_tiff(&mut stream) {
                Ok(Some(preview)) => {
                    let memory = unsafe { SHCreateMemStream(Some(&preview)) }
                        .ok_or_else(|| Error::from_hresult(E_FAIL))?;
                    raster::decode(&memory, raster::Format::Tiff, size)
                }
                Ok(None) | Err(file_icons_thumbnail_core::Error::Unsupported) => {
                    match file_icons_thumbnail_core::eps::extract_epsi(&mut stream, size) {
                        Ok(Some(image)) => Ok(raster::Pixels {
                            width: image.width,
                            height: image.height,
                            bytes: image.rgba,
                        }),
                        Ok(None)
                        | Err(
                            file_icons_thumbnail_core::Error::Unsupported
                            | file_icons_thumbnail_core::Error::Limit,
                        ) => xmp(&mut stream, size),
                        Err(_) => Err(Error::from_hresult(E_FAIL)),
                    }
                }
                _ => Err(Error::from_hresult(E_FAIL)),
            }
        }
        Format::Pdf => {
            let _runtime = Runtime::initialize()?;
            let output = pdf::render_first_page(&input, size)?;
            let stream: IStream = unsafe { CreateStreamOverRandomAccessStream(&output)? };
            raster::decode(&stream, raster::Format::Png, size)
        }
        Format::Svg => {
            let mut stream = StreamReader::new(input);
            let mut image = file_icons_thumbnail_core::svg::render(&mut stream, size)
                .map_err(|_| Error::from_hresult(E_FAIL))?;
            for pixel in image.rgba.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
            }
            Ok(raster::Pixels {
                width: image.width,
                height: image.height,
                bytes: image.rgba,
            })
        }
        // PDF非互換の説明ページを避け、確認できた埋め込み画像だけを返す。
        Format::Ai => {
            let mut stream = StreamReader::new(input);
            match file_icons_thumbnail_core::eps::extract_tiff(&mut stream) {
                Ok(Some(preview)) => {
                    let memory = unsafe { SHCreateMemStream(Some(&preview)) }
                        .ok_or_else(|| Error::from_hresult(E_FAIL))?;
                    raster::decode(&memory, raster::Format::Tiff, size)
                }
                Ok(None) | Err(file_icons_thumbnail_core::Error::Unsupported) => {
                    xmp(&mut stream, size)
                }
                Err(_) => Err(Error::from_hresult(E_FAIL)),
            }
        }
    }
}

fn xmp(stream: &mut StreamReader, size: u32) -> Result<raster::Pixels> {
    let image = file_icons_thumbnail_core::xmp::extract(stream)
        .map_err(|_| Error::from_hresult(E_FAIL))?
        .ok_or_else(|| Error::from_hresult(E_FAIL))?;
    let memory =
        unsafe { SHCreateMemStream(Some(&image)) }.ok_or_else(|| Error::from_hresult(E_FAIL))?;
    raster::decode(&memory, raster::Format::Jpeg, size)
}

struct Runtime;
impl Runtime {
    fn initialize() -> Result<Self> {
        let mut apartment = APTTYPE::default();
        let mut qualifier = APTTYPEQUALIFIER::default();
        unsafe { CoGetApartmentType(&mut apartment, &mut qualifier)? };
        let mode = match apartment {
            APTTYPE_STA | APTTYPE_MAINSTA => RO_INIT_SINGLETHREADED,
            APTTYPE_MTA => RO_INIT_MULTITHREADED,
            _ => return Err(Error::from_hresult(E_FAIL)),
        };
        unsafe { RoInitialize(mode)? };
        Ok(Self)
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        unsafe { RoUninitialize() }
    }
}

#[implement(IClassFactory)]
struct Factory {
    format: Format,
}
impl Factory {
    fn new(format: Format) -> Self {
        OBJECTS.fetch_add(1, Ordering::Relaxed);
        Self { format }
    }
}
impl Drop for Factory {
    fn drop(&mut self) {
        OBJECTS.fetch_sub(1, Ordering::Relaxed);
    }
}
impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(
        &self,
        outer: Ref<IUnknown>,
        iid: *const GUID,
        output: *mut *mut c_void,
    ) -> Result<()> {
        if output.is_null() || iid.is_null() {
            return Err(Error::from_hresult(E_POINTER));
        }
        unsafe { *output = std::ptr::null_mut() };
        if outer.is_some() {
            return Err(Error::from_hresult(CLASS_E_NOAGGREGATION));
        }
        let object: IUnknown = Provider::new(self.format).into();
        unsafe { object.query(iid, output).ok() }
    }
    fn LockServer(&self, lock: BOOL) -> Result<()> {
        if lock.as_bool() {
            LOCKS.fetch_add(1, Ordering::Relaxed);
        } else {
            let _ = LOCKS.try_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
                count.checked_sub(1)
            });
        }
        Ok(())
    }
}

/// # Safety
/// ポインターはCOMの契約に従い、有効な入力と書き込み可能な出力を指す。
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllGetClassObject(
    clsid: *const GUID,
    iid: *const GUID,
    output: *mut *mut c_void,
) -> HRESULT {
    if clsid.is_null() || iid.is_null() || output.is_null() {
        return E_POINTER;
    }
    unsafe { *output = std::ptr::null_mut() };
    let Some(format) = Format::from_clsid(unsafe { *clsid }.to_u128()) else {
        return CLASS_E_CLASSNOTAVAILABLE;
    };
    std::panic::catch_unwind(|| {
        let factory: IUnknown = Factory::new(format).into();
        unsafe { factory.query(iid, output) }
    })
    .unwrap_or(E_FAIL)
}

#[unsafe(no_mangle)]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    if OBJECTS.load(Ordering::Relaxed) == 0 && LOCKS.load(Ordering::Relaxed) == 0 {
        S_OK
    } else {
        S_FALSE
    }
}
