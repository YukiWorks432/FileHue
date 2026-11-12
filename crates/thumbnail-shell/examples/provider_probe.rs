//! 指定ファイルを登録せずにサムネイルDLLへ渡し、結果と処理時間を確認する。
use file_icons_thumbnail_core::format::Format;
use std::{
    ffi::c_void,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    time::Instant,
};
use windows::{
    Win32::{
        Foundation::{FreeLibrary, HMODULE},
        Graphics::Gdi::{DeleteObject, HBITMAP, HGDIOBJ},
        System::{
            Com::{
                COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize, IClassFactory, STGM_READ,
                STGM_SHARE_DENY_NONE,
            },
            LibraryLoader::{GetProcAddress, LoadLibraryW},
            ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS},
            Threading::GetCurrentProcess,
        },
        UI::Shell::{
            IThumbnailProvider, PropertiesSystem::IInitializeWithStream, SHCreateStreamOnFileEx,
            WTS_ALPHATYPE,
        },
    },
    core::{GUID, HRESULT, Interface, PCSTR, PCWSTR, Result},
};

struct Library(HMODULE);
impl Drop for Library {
    fn drop(&mut self) {
        let _ = unsafe { FreeLibrary(self.0) };
    }
}

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let dll = PathBuf::from(args.next().expect("expected DLL path"));
    let file = PathBuf::from(args.next().expect("expected file path"));
    assert!(args.next().is_none());
    let extension = file
        .extension()
        .and_then(|part| part.to_str())
        .expect("extension missing");
    let extension = format!(".{}", extension.to_ascii_lowercase());
    let format = Format::ALL
        .into_iter()
        .find(|format| format.extension() == extension)
        .expect("unsupported extension");
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
    let result = probe(&dll, &file, format);
    unsafe { CoUninitialize() };
    result
}

fn probe(dll: &Path, path: &Path, format: Format) -> Result<()> {
    let name: Vec<u16> = dll.as_os_str().encode_wide().chain(Some(0)).collect();
    let library = Library(unsafe { LoadLibraryW(PCWSTR(name.as_ptr()))? });
    let get = unsafe { GetProcAddress(library.0, PCSTR(c"DllGetClassObject".as_ptr().cast())) }
        .expect("missing factory export");
    let get: unsafe extern "system" fn(*const GUID, *const GUID, *mut *mut c_void) -> HRESULT =
        unsafe { std::mem::transmute(get) };
    let mut raw = std::ptr::null_mut();
    unsafe {
        get(
            &GUID::from_u128(format.clsid()),
            &IClassFactory::IID,
            &mut raw,
        )
        .ok()?
    };
    let factory = unsafe { IClassFactory::from_raw(raw) };
    let provider: IInitializeWithStream = unsafe { factory.CreateInstance(None)? };
    let file: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let start = Instant::now();
    let stream = unsafe {
        SHCreateStreamOnFileEx(
            PCWSTR(file.as_ptr()),
            (STGM_READ | STGM_SHARE_DENY_NONE).0,
            0,
            false,
            None,
        )?
    };
    unsafe { provider.Initialize(&stream, 0)? };
    let thumbnail: IThumbnailProvider = provider.cast()?;
    let mut bitmap = HBITMAP::default();
    let mut alpha = WTS_ALPHATYPE::default();
    let result = unsafe { thumbnail.GetThumbnail(256, &mut bitmap, &mut alpha) };
    let elapsed = start.elapsed().as_micros();
    let mut memory = PROCESS_MEMORY_COUNTERS::default();
    let memory_size = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
    unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut memory, memory_size).ok()? };
    if result.is_ok() {
        assert!(unsafe { DeleteObject(HGDIOBJ(bitmap.0)) }.as_bool());
        println!(
            "outcome=preview elapsed_us={elapsed} peak_working_set_bytes={}",
            memory.PeakWorkingSetSize
        );
    } else {
        println!(
            "outcome=unavailable elapsed_us={elapsed} peak_working_set_bytes={}",
            memory.PeakWorkingSetSize
        );
    }
    drop(thumbnail);
    drop(provider);
    drop(factory);
    drop(library);
    Ok(())
}
