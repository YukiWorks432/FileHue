//! Shellの入力ストリームからPDF先頭ページを描画する。

use std::sync::{Arc, mpsc};
use windows::{
    Data::Pdf::{PdfDocument, PdfPageRenderOptions},
    Storage::Streams::{IRandomAccessStream, InMemoryRandomAccessStream},
    Win32::{
        Foundation::{CloseHandle, E_FAIL, E_INVALIDARG, HANDLE},
        System::{
            Com::{CoWaitForMultipleHandles, IStream},
            Threading::{CreateEventW, SetEvent},
            WinRT::{BSOS_DEFAULT, CreateRandomAccessStreamOverStream},
        },
    },
    core::{Error, Result},
};

/// 呼び出し元はWindows Runtimeを初期化し、完了までストリームを保持する。
/// 描画サイズはDIPで指定され、出力ピクセル数は表示倍率によって変わる。
/// Shellへ渡す前に要求されたピクセル数へ縮小する必要がある。
pub fn render_first_page(input: &IStream, max_side: u32) -> Result<InMemoryRandomAccessStream> {
    if max_side == 0 || max_side > 1024 {
        return Err(Error::from_hresult(E_INVALIDARG));
    }
    let stream: IRandomAccessStream =
        unsafe { CreateRandomAccessStreamOverStream(input, BSOS_DEFAULT)? };
    let load = PdfDocument::LoadFromStreamAsync(&stream)?;
    let document = match wait(|callback| load.when(callback)) {
        Ok(document) => document,
        Err(error) => {
            let _ = load.Cancel();
            return Err(error);
        }
    };
    if document.PageCount()? == 0 {
        return Err(Error::from_hresult(E_INVALIDARG));
    }
    let page = document.GetPage(0)?;
    let size = page.Size()?;
    if !size.Width.is_finite()
        || !size.Height.is_finite()
        || size.Width <= 0.0
        || size.Height <= 0.0
    {
        return Err(Error::from_hresult(E_INVALIDARG));
    }
    let scale = max_side as f32 / size.Width.max(size.Height);
    let options = PdfPageRenderOptions::new()?;
    options.SetDestinationWidth(((size.Width * scale).round() as u32).max(1))?;
    options.SetDestinationHeight(((size.Height * scale).round() as u32).max(1))?;
    let output = InMemoryRandomAccessStream::new()?;
    let rendering = page.RenderWithOptionsToStreamAsync(&output, &options)?;
    if let Err(error) = wait(|callback| rendering.when(callback)) {
        let _ = rendering.Cancel();
        return Err(error);
    }
    page.Close()?;
    output.Seek(0)?;
    Ok(output)
}

struct CompletionEvent(HANDLE);

// Windowsのイベントは複数スレッドから通知できる。Arcは通知完了までハンドルを保持する。
unsafe impl Send for CompletionEvent {}
unsafe impl Sync for CompletionEvent {}

impl Drop for CompletionEvent {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

fn wait<T: Send + 'static>(
    register: impl FnOnce(Box<dyn FnOnce(Result<T>) + Send>) -> Result<()>,
) -> Result<T> {
    let event = Arc::new(CompletionEvent(unsafe {
        CreateEventW(None, false, false, None)?
    }));
    let signal = event.clone();
    let (sender, receiver) = mpsc::sync_channel(1);
    register(Box::new(move |result| {
        let _ = sender.send(result);
        let _ = unsafe { SetEvent(signal.0) };
    }))?;
    // STAではCOM呼び出しを処理しながら待ち、非同期処理の完了を妨げない。
    unsafe { CoWaitForMultipleHandles(0, 10_000, &[event.0])? };
    receiver
        .try_recv()
        .map_err(|_| Error::from_hresult(E_FAIL))?
}
