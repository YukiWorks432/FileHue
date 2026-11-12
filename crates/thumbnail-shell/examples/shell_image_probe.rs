//! Windows Shellの通常のサムネイル取得を一つの実ファイルに対して確認する。
use file_icons_thumbnail_core::format::Format;
use std::{os::windows::ffi::OsStrExt, path::PathBuf, time::Instant};
use windows::{
    Win32::{
        Foundation::SIZE,
        Graphics::Gdi::{BITMAP, DeleteObject, GetObjectW, HBITMAP, HGDIOBJ},
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
            CoUninitialize,
        },
        UI::Shell::{
            BHID_ThumbnailHandler, IShellItem, IShellItemImageFactory, IThumbnailProvider,
            SHCreateItemFromParsingName, SIIGBF_THUMBNAILONLY, WTS_ALPHATYPE,
        },
    },
    core::{GUID, PCWSTR, Result},
};

fn main() -> Result<()> {
    let file = PathBuf::from(std::env::args_os().nth(1).expect("expected file path"));
    assert!(file.is_absolute());
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()? };
    let result = (|| {
        if let Some(format) =
            file.extension()
                .and_then(|value| value.to_str())
                .and_then(|extension| {
                    Format::ALL.into_iter().find(|format| {
                        format
                            .extension()
                            .trim_start_matches('.')
                            .eq_ignore_ascii_case(extension)
                    })
                })
        {
            let direct: Result<IThumbnailProvider> = unsafe {
                CoCreateInstance(&GUID::from_u128(format.clsid()), None, CLSCTX_INPROC_SERVER)
            };
            eprintln!(
                "direct_activation={}",
                if direct.is_ok() { "ok" } else { "failed" }
            );
        }
        let path: Vec<u16> = file.as_os_str().encode_wide().chain(Some(0)).collect();
        let shell_item: IShellItem =
            unsafe { SHCreateItemFromParsingName(PCWSTR(path.as_ptr()), None) }?;
        let handler: Result<IThumbnailProvider> =
            unsafe { shell_item.BindToHandler(None, &BHID_ThumbnailHandler) };
        match handler {
            Ok(handler) => {
                let mut bitmap = HBITMAP::default();
                let mut alpha = WTS_ALPHATYPE::default();
                let result = unsafe { handler.GetThumbnail(256, &mut bitmap, &mut alpha) };
                eprintln!("bound_handler={result:?}");
                if !bitmap.is_invalid() {
                    unsafe {
                        let _ = DeleteObject(HGDIOBJ(bitmap.0));
                    }
                }
            }
            Err(error) => eprintln!("bind_handler={error}"),
        }
        let item: IShellItemImageFactory =
            unsafe { SHCreateItemFromParsingName(PCWSTR(path.as_ptr()), None) }
                .inspect_err(|error| eprintln!("stage=item: {error}"))?;
        let start = Instant::now();
        let bitmap = unsafe { item.GetImage(SIZE { cx: 256, cy: 256 }, SIIGBF_THUMBNAILONLY) }
            .inspect_err(|error| eprintln!("stage=image: {error}"))?;
        let elapsed = start.elapsed().as_micros();
        let mut info = BITMAP::default();
        let count = unsafe {
            GetObjectW(
                HGDIOBJ(bitmap.0),
                std::mem::size_of::<BITMAP>() as i32,
                Some((&mut info as *mut BITMAP).cast()),
            )
        };
        assert_eq!(count as usize, std::mem::size_of::<BITMAP>());
        assert!(unsafe { DeleteObject(HGDIOBJ(bitmap.0)) }.as_bool());
        println!(
            "outcome=preview width={} height={} elapsed_us={elapsed}",
            info.bmWidth, info.bmHeight
        );
        Ok(())
    })();
    unsafe { CoUninitialize() };
    result
}
