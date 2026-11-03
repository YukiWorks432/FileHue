//! Explorerに読み込まれる最小限のCOMアイコンハンドラー。
#![allow(non_snake_case)]

#[cfg(test)]
#[path = "../../../test-support/process.rs"]
mod test_process;
use file_icons_windows::{CLSID, IconLocation};
use std::{
    cell::Cell,
    ffi::c_void,
    path::Path,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
};
use windows::{
    Win32::{
        Foundation::*,
        System::Com::*,
        UI::{Shell::*, WindowsAndMessaging::HICON},
    },
    core::*,
};

mod association;

static OBJECTS: AtomicUsize = AtomicUsize::new(0);
static LOCKS: AtomicUsize = AtomicUsize::new(0);
static SETTINGS: OnceLock<Mutex<file_icons_windows::SettingsCache>> = OnceLock::new();

thread_local! {
    static DELEGATING: Cell<bool> = const { Cell::new(false) };
}

/// 元のハンドラーがShellへ再入しても、委譲の無限再帰を起こさない。
struct Delegation;
impl Delegation {
    fn enter() -> Option<Self> {
        DELEGATING.with(|active| {
            if active.replace(true) {
                None
            } else {
                Some(Self)
            }
        })
    }
}
impl Drop for Delegation {
    fn drop(&mut self) {
        DELEGATING.with(|active| active.set(false));
    }
}

#[derive(Clone)]
enum IconSource {
    File(IconLocation),
    Original(IExtractIconW),
}

#[implement(IPersistFile, IExtractIconW)]
struct IconHandler {
    source: Mutex<Option<IconSource>>,
}
impl IconHandler {
    fn new() -> Self {
        OBJECTS.fetch_add(1, Ordering::Relaxed);
        Self {
            source: Mutex::new(None),
        }
    }
}
impl Drop for IconHandler {
    fn drop(&mut self) {
        OBJECTS.fetch_sub(1, Ordering::Relaxed);
    }
}
impl IPersist_Impl for IconHandler_Impl {
    fn GetClassID(&self) -> Result<GUID> {
        Ok(CLSID)
    }
}
impl IPersistFile_Impl for IconHandler_Impl {
    fn IsDirty(&self) -> HRESULT {
        S_FALSE
    }
    fn Load(&self, filename: &PCWSTR, mode: STGM) -> Result<()> {
        let filename = unsafe { filename.to_string()? };
        let name = Path::new(&filename)
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?;
        let settings_path = file_icons_windows::home()
            .map_err(|_| Error::from_hresult(E_FAIL))?
            .join("settings.json");
        let settings = SETTINGS
            .get_or_init(|| Mutex::new(file_icons_windows::SettingsCache::default()))
            .lock()
            .map_err(|_| Error::from_hresult(E_FAIL))?
            .get(&settings_path)
            .map_err(|_| Error::from_hresult(E_FAIL))?;
        let themed = settings
            .active
            .as_ref()
            .and_then(|id| settings.packs.iter().find(|pack| &pack.id == id))
            .and_then(|pack| {
                settings
                    .active_selection
                    .resolve(&pack.theme, &filename)
                    .map(|(path, _)| path)
            })
            .map(|path| IconLocation {
                path: path.to_string_lossy().into_owned(),
                index: 0,
            });
        let protected = if themed.is_some() && !settings.icon_exclusions.is_empty() {
            let extension = Path::new(name)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");
            let class = association::resolve(extension, |ext| {
                file_icons_windows::association(ext).map_err(|_| Error::from_hresult(E_FAIL))
            })?;
            let lower_name = name.to_ascii_lowercase();
            settings.icon_exclusions.iter().any(|item| {
                (item.reason == file_icons_windows::ProtectionReason::ProtectedFileType
                    && item.class.eq_ignore_ascii_case(&class))
                    || (!item.extension.is_empty()
                        && lower_name
                            .strip_suffix(&item.extension)
                            .is_some_and(|prefix| prefix.ends_with('.')))
                    || (item.extension.is_empty()
                        && item.class.eq_ignore_ascii_case("Unknown")
                        && class.starts_with('.')
                        && settings.fallback(&class).is_none())
                    || (item.extension.is_empty()
                        && item.class.eq_ignore_ascii_case("Unknown")
                        && (extension.is_empty() || class.eq_ignore_ascii_case("Unknown")))
            })
        } else {
            false
        };
        let source = if let Some(themed) = themed.filter(|_| !protected) {
            Some(IconSource::File(themed))
        } else {
            let ext = Path::new(name)
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or("");
            let progid = association::resolve(ext, |extension| {
                file_icons_windows::association(extension).map_err(|_| Error::from_hresult(E_FAIL))
            })?;
            let class = if settings.fallback(&progid).is_some() {
                progid.as_str()
            } else if progid.starts_with('.') {
                "Unknown"
            } else {
                progid.as_str()
            };
            let extension_key = format!(".{}", ext.to_ascii_lowercase());
            let original = settings
                .original_handler(&extension_key)
                .or_else(|| settings.original_handler(class))
                .and_then(|id| {
                    let _delegation = Delegation::enter()?;
                    let id = file_icons_windows::handler_clsid(id).ok()?;
                    if id == CLSID {
                        return None;
                    }
                    let persist: IPersistFile =
                        unsafe { CoCreateInstance(&id, None, CLSCTX_INPROC_SERVER).ok()? };
                    let wide = filename.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
                    unsafe {
                        persist.Load(PCWSTR(wide.as_ptr()), mode).ok()?;
                    }
                    persist.cast::<IExtractIconW>().ok()
                });
            original.map(IconSource::Original).or_else(|| {
                let mut icon = settings
                    .fallback(&extension_key)
                    .or_else(|| settings.fallback(class))?
                    .clone();
                if icon.path == "%1" {
                    icon.path = filename.clone();
                }
                Some(IconSource::File(icon))
            })
        };
        *self
            .source
            .lock()
            .map_err(|_| Error::from_hresult(E_FAIL))? = source;
        Ok(())
    }
    fn Save(&self, _: &PCWSTR, _: BOOL) -> Result<()> {
        Err(Error::from_hresult(E_NOTIMPL))
    }
    fn SaveCompleted(&self, _: &PCWSTR) -> Result<()> {
        Err(Error::from_hresult(E_NOTIMPL))
    }
    fn GetCurFile(&self) -> Result<PWSTR> {
        Err(Error::from_hresult(E_NOTIMPL))
    }
}
impl IExtractIconW_Impl for IconHandler_Impl {
    fn GetIconLocation(
        &self,
        request: u32,
        output: PWSTR,
        capacity: u32,
        index: *mut i32,
        flags: *mut u32,
    ) -> Result<()> {
        if output.is_null() || index.is_null() || flags.is_null() {
            return Err(Error::from_hresult(E_POINTER));
        }
        let source = self
            .source
            .lock()
            .map_err(|_| Error::from_hresult(E_FAIL))?
            .clone();
        let location = match source {
            Some(IconSource::File(location)) => location,
            Some(IconSource::Original(original)) => {
                // S_FALSEなどもABIへそのまま返す。委譲先の呼び出し中はロックを保持しない。
                let _delegation = Delegation::enter().ok_or_else(|| Error::from_hresult(E_FAIL))?;
                let result = unsafe {
                    (original.vtable().GetIconLocation)(
                        original.as_raw(),
                        // 委譲先のCOMはApartmentのため、他スレッドでの再利用を要求しない。
                        request & !GIL_ASYNC,
                        output,
                        capacity,
                        index,
                        flags,
                    )
                };
                if result == S_OK {
                    // このクラスにはテーマの専用名もあるため、クラス共通のキャッシュにはしない。
                    unsafe {
                        *flags = (*flags & !GIL_PERCLASS) | GIL_PERINSTANCE;
                    }
                }
                return preserve_hresult(result);
            }
            None => return Err(Error::from_hresult(E_FAIL)),
        };
        let path = location
            .path
            .encode_utf16()
            .chain(Some(0))
            .collect::<Vec<_>>();
        if path.len() > capacity as usize {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        unsafe {
            std::ptr::copy_nonoverlapping(path.as_ptr(), output.0, path.len());
            *index = location.index;
            *flags = GIL_PERINSTANCE;
        }
        Ok(())
    }
    fn Extract(
        &self,
        file: &PCWSTR,
        index: u32,
        large: *mut HICON,
        small: *mut HICON,
        size: u32,
    ) -> Result<()> {
        let source = self
            .source
            .lock()
            .map_err(|_| Error::from_hresult(E_FAIL))?
            .clone();
        if let Some(IconSource::Original(original)) = source {
            let _delegation = Delegation::enter().ok_or_else(|| Error::from_hresult(E_FAIL))?;
            let result = unsafe {
                (original.vtable().Extract)(original.as_raw(), *file, index, large, small, size)
            };
            return preserve_hresult(result);
        }
        unsafe {
            if !large.is_null() {
                *large = HICON::default();
            }
            if !small.is_null() {
                *small = HICON::default();
            }
        }
        // ファイルからの抽出はShellに任せる。
        Err(Error::from_hresult(S_FALSE))
    }
}

// windows-rsの通常のResult変換では成功コードの違いが失われる。
fn preserve_hresult(result: HRESULT) -> Result<()> {
    if result == S_OK {
        Ok(())
    } else {
        Err(Error::from_hresult(result))
    }
}

#[implement(IClassFactory)]
struct Factory;
impl Factory {
    fn new() -> Self {
        OBJECTS.fetch_add(1, Ordering::Relaxed);
        Self
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
        unsafe {
            *output = std::ptr::null_mut();
        }
        if outer.is_some() {
            return Err(Error::from_hresult(CLASS_E_NOAGGREGATION));
        }
        let object: IUnknown = IconHandler::new().into();
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

/// COMから渡されるポインターは呼び出し側の契約に従う。
///
/// # Safety
/// `clsid`と`iid`は有効なGUIDを指し、`output`は書き込み可能な
/// インターフェースポインターの領域を指さなければならない。
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllGetClassObject(
    clsid: *const GUID,
    iid: *const GUID,
    output: *mut *mut c_void,
) -> HRESULT {
    if clsid.is_null() || iid.is_null() || output.is_null() {
        return E_POINTER;
    }
    unsafe {
        *output = std::ptr::null_mut();
    }
    if unsafe { *clsid } != CLSID {
        return CLASS_E_CLASSNOTAVAILABLE;
    }
    std::panic::catch_unwind(|| {
        let factory: IUnknown = Factory::new().into();
        unsafe { factory.query(iid, output) }
    })
    .unwrap_or(E_FAIL)
}

#[unsafe(no_mangle)]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    if OBJECTS.load(Ordering::Relaxed) == 0 && LOCKS.load(Ordering::Relaxed) == 0 {
        association::release();
        if let Some(settings) = SETTINGS.get() {
            *settings.lock().unwrap_or_else(|error| error.into_inner()) = Default::default();
        }
        if OBJECTS.load(Ordering::Relaxed) == 0 && LOCKS.load(Ordering::Relaxed) == 0 {
            S_OK
        } else {
            S_FALSE
        }
    } else {
        S_FALSE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[implement(IExtractIconW)]
    struct OriginalProbe {
        result: HRESULT,
    }
    impl IExtractIconW_Impl for OriginalProbe_Impl {
        fn GetIconLocation(
            &self,
            request: u32,
            output: PWSTR,
            capacity: u32,
            index: *mut i32,
            flags: *mut u32,
        ) -> Result<()> {
            assert_eq!(request, GIL_FORSHORTCUT);
            let text = "foreign identifier"
                .encode_utf16()
                .chain(Some(0))
                .collect::<Vec<_>>();
            assert!(capacity as usize >= text.len());
            unsafe {
                std::ptr::copy_nonoverlapping(text.as_ptr(), output.0, text.len());
                *index = 19;
                *flags = GIL_NOTFILENAME | GIL_DONTCACHE | GIL_PERCLASS;
            }
            preserve_hresult(self.result)
        }
        fn Extract(
            &self,
            file: &PCWSTR,
            index: u32,
            large: *mut HICON,
            small: *mut HICON,
            size: u32,
        ) -> Result<()> {
            assert_eq!(unsafe { file.to_string()? }, "foreign identifier");
            assert_eq!(index, 19);
            assert_eq!(size, 32 | (16 << 16));
            unsafe {
                *large = HICON::default();
                *small = HICON::default();
            }
            preserve_hresult(self.result)
        }
    }

    #[test]
    fn com_factory_and_icon_contract_across_abi() {
        unsafe {
            let mut raw = std::ptr::null_mut();
            assert_eq!(
                DllGetClassObject(&CLSID, &IClassFactory::IID, &mut raw),
                S_OK
            );
            let factory = IClassFactory::from_raw(raw);
            assert_eq!(DllCanUnloadNow(), S_FALSE);
            let persist: IPersistFile = factory.CreateInstance(None).unwrap();
            assert_eq!(persist.GetClassID().unwrap(), CLSID);
            assert!(persist.cast::<IExtractIconW>().is_ok());
            drop(persist);
            drop(factory);
            assert_eq!(DllCanUnloadNow(), S_OK);

            let handler = IconHandler::new();
            *handler.source.lock().unwrap() = Some(IconSource::File(IconLocation {
                path: "C:\\test\\icon.ico".into(),
                index: 3,
            }));
            let extractor: IExtractIconW = handler.into();
            let mut buffer = [0u16; 260];
            let mut index = 0;
            let mut flags = 0;
            extractor
                .GetIconLocation(0, &mut buffer, &mut index, &mut flags)
                .unwrap();
            assert_eq!(
                String::from_utf16(&buffer[..buffer.iter().position(|value| *value == 0).unwrap()])
                    .unwrap(),
                "C:\\test\\icon.ico"
            );
            assert_eq!(index, 3);
            assert_eq!(flags & GIL_PERINSTANCE, GIL_PERINSTANCE);
            assert!(
                extractor
                    .GetIconLocation(0, &mut [0u16; 1], &mut index, &mut flags)
                    .is_err()
            );
            let mut large = HICON::default();
            let mut small = HICON::default();
            // Result<()>はS_FALSEも成功として扱うため、ABIのHRESULTを直接確認する。
            let hr = (extractor.vtable().Extract)(
                extractor.as_raw(),
                PCWSTR(buffer.as_ptr()),
                3,
                &mut large,
                &mut small,
                32,
            );
            assert_eq!(hr, S_FALSE);
            assert!(large.is_invalid() && small.is_invalid());
            // 戻り値、仮想的な位置、キャッシュ用フラグ、サイズ指定を透過的に委譲する。
            for result in [S_OK, S_FALSE, E_FAIL] {
                let original: IExtractIconW = OriginalProbe { result }.into();
                let handler = IconHandler::new();
                *handler.source.lock().unwrap() = Some(IconSource::Original(original));
                let delegated: IExtractIconW = handler.into();
                assert_eq!(
                    (delegated.vtable().GetIconLocation)(
                        delegated.as_raw(),
                        GIL_FORSHORTCUT | GIL_ASYNC,
                        PWSTR(buffer.as_mut_ptr()),
                        buffer.len() as u32,
                        &mut index,
                        &mut flags
                    ),
                    result
                );
                assert_eq!(index, 19);
                assert_eq!(
                    flags,
                    GIL_NOTFILENAME
                        | GIL_DONTCACHE
                        | if result == S_OK {
                            GIL_PERINSTANCE
                        } else {
                            GIL_PERCLASS
                        }
                );
                assert_eq!(
                    (delegated.vtable().Extract)(
                        delegated.as_raw(),
                        PCWSTR(buffer.as_ptr()),
                        19,
                        &mut large,
                        &mut small,
                        32 | (16 << 16)
                    ),
                    result
                );
            }
            let guard = Delegation::enter().unwrap();
            assert!(Delegation::enter().is_none());
            drop(guard);
            assert!(Delegation::enter().is_some());
        }
    }
}
