//! Shellの入力ストリームからPDF先頭ページを描画する。

use crate::provider::PendingCallback;
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
use windows_future::{AsyncActionCompletedHandler, AsyncOperationCompletedHandler};

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
    let document = match wait(10_000, |callback, lease| {
        load.SetCompleted(&document_handler(callback, lease))
    }) {
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
    if let Err(error) = wait(10_000, |callback, lease| {
        rendering.SetCompleted(&action_handler(callback, lease))
    }) {
        let _ = rendering.Cancel();
        return Err(error);
    }
    page.Close()?;
    output.Seek(0)?;
    Ok(output)
}

fn document_handler(
    callback: Box<dyn Fn(Result<PdfDocument>) + Send>,
    lease: PendingCallback,
) -> AsyncOperationCompletedHandler<PdfDocument> {
    AsyncOperationCompletedHandler::new(move |operation, _| {
        let _lease = &lease;
        callback(operation.ok().and_then(|operation| operation.GetResults()));
        Ok(())
    })
}

fn action_handler(
    callback: Box<dyn Fn(Result<()>) + Send>,
    lease: PendingCallback,
) -> AsyncActionCompletedHandler {
    AsyncActionCompletedHandler::new(move |operation, _| {
        let _lease = &lease;
        callback(operation.ok().and_then(|operation| operation.GetResults()));
        Ok(())
    })
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
    timeout_ms: u32,
    register: impl FnOnce(Box<dyn Fn(Result<T>) + Send>, PendingCallback) -> Result<()>,
) -> Result<T> {
    let event = Arc::new(CompletionEvent(unsafe {
        CreateEventW(None, false, false, None)?
    }));
    let signal = event.clone();
    let (sender, receiver) = mpsc::sync_channel(1);
    register(
        Box::new(move |result| {
            let _ = sender.try_send(result);
            let _ = unsafe { SetEvent(signal.0) };
        }),
        PendingCallback::new(),
    )?;
    // STAではCOM呼び出しを処理しながら待ち、非同期処理の完了を妨げない。
    unsafe { CoWaitForMultipleHandles(0, timeout_ms, &[event.0])? };
    receiver
        .try_recv()
        .map_err(|_| Error::from_hresult(E_FAIL))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::DllCanUnloadNow;
    use std::cell::RefCell;
    use windows::Win32::{
        Foundation::{S_FALSE, S_OK},
        System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize},
    };
    use windows_future::AsyncStatus;

    #[test]
    fn timed_out_completion_keeps_dll_loaded_until_handler_is_released() {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok().unwrap() };
        let retained = RefCell::new(None);
        let result: Result<()> = wait(1, |callback, lease| {
            let handler = action_handler(callback, lease);
            *retained.borrow_mut() = Some(handler);
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(DllCanUnloadNow(), S_FALSE);
        retained
            .borrow()
            .as_ref()
            .unwrap()
            .Invoke(None, AsyncStatus::Canceled)
            .unwrap();
        assert_eq!(DllCanUnloadNow(), S_FALSE);
        drop(retained.into_inner());
        assert_eq!(DllCanUnloadNow(), S_OK);

        let retained = RefCell::new(None);
        let result: Result<PdfDocument> = wait(1, |callback, lease| {
            *retained.borrow_mut() = Some(document_handler(callback, lease));
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(DllCanUnloadNow(), S_FALSE);
        retained
            .borrow()
            .as_ref()
            .unwrap()
            .Invoke(None, AsyncStatus::Canceled)
            .unwrap();
        assert_eq!(DllCanUnloadNow(), S_FALSE);
        drop(retained.into_inner());
        assert_eq!(DllCanUnloadNow(), S_OK);
        unsafe { CoUninitialize() };
    }
}
