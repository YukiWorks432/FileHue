//! PDFストリームの描画を登録なしで確認する。子プロセスから期限付きで実行する。
use std::{os::windows::ffi::OsStrExt, path::Path, time::Instant};
use windows::{
    Win32::{
        System::{
            Com::{STGM_READ, STGM_SHARE_DENY_NONE},
            WinRT::{RO_INIT_MULTITHREADED, RO_INIT_SINGLETHREADED, RoInitialize, RoUninitialize},
        },
        UI::Shell::SHCreateStreamOnFileEx,
    },
    core::{PCWSTR, Result},
};

fn main() -> Result<()> {
    let mut arguments = std::env::args_os().skip(1);
    let apartment = arguments.next().expect("expected sta or mta");
    let path = arguments.next().expect("expected PDF path");
    assert!(arguments.next().is_none());
    let mode = match apartment.to_str() {
        Some("sta") => RO_INIT_SINGLETHREADED,
        Some("mta") => RO_INIT_MULTITHREADED,
        _ => panic!("expected sta or mta"),
    };
    unsafe { RoInitialize(mode)? };
    let result = render(Path::new(&path));
    unsafe { RoUninitialize() };
    result
}

fn render(path: &Path) -> Result<()> {
    let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let start = Instant::now();
    let input = unsafe {
        SHCreateStreamOnFileEx(
            PCWSTR(name.as_ptr()),
            (STGM_READ | STGM_SHARE_DENY_NONE).0,
            0,
            false,
            None,
        )?
    };
    let output = file_icons_thumbnail_shell::pdf::render_first_page(&input, 256)?;
    println!(
        "output_bytes={} elapsed_ms={}",
        output.Size()?,
        start.elapsed().as_millis()
    );
    output.Close()?;
    Ok(())
}
