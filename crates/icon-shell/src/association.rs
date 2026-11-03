//! 関連付けの変更通知を確認してから再利用する、上限付きの照会キャッシュ。
use std::{
    collections::BTreeMap,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    sync::{
        Mutex, MutexGuard,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::{HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::{
        Registry::*,
        Threading::{CreateEventW, WaitForSingleObject},
    },
};
use winreg::{
    RegKey,
    enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_NOTIFY},
};

const CAPACITY: usize = 256;
const MAX_TEXT: usize = 1024;
// RegRestoreKeyなど通知されない変更にも、期限を超えて古い結果を保持しない。
const LIFETIME: Duration = Duration::from_secs(1);
static REVISION: AtomicU64 = AtomicU64::new(1);
static STATE: Mutex<Option<Cache>> = Mutex::new(None);

fn next_revision() -> u64 {
    REVISION.fetch_add(1, Ordering::Relaxed)
}

struct Watch {
    key: RegKey,
    event: OwnedHandle,
}
impl Watch {
    fn new(parent: &RegKey, path: &str) -> std::io::Result<Self> {
        let key = parent.open_subkey_with_flags(path, KEY_NOTIFY)?;
        // 自動リセット。消費した通知に対してだけ再登録し、待機を重複させない。
        let handle = unsafe { CreateEventW(None, false, false, None) }?;
        let watch = Self {
            key,
            event: unsafe { OwnedHandle::from_raw_handle(handle.0) },
        };
        watch.arm()?;
        Ok(watch)
    }
    fn arm(&self) -> windows::core::Result<()> {
        unsafe {
            RegNotifyChangeKeyValue(
                HKEY(self.key.raw_handle()),
                true,
                REG_NOTIFY_CHANGE_NAME
                    | REG_NOTIFY_CHANGE_LAST_SET
                    | REG_NOTIFY_CHANGE_SECURITY
                    | REG_NOTIFY_CHANGE_ATTRIBUTES
                    | REG_NOTIFY_THREAD_AGNOSTIC,
                Some(HANDLE(self.event.as_raw_handle())),
                true,
            )
            .ok()
        }
    }
    fn poll(&self) -> windows::core::Result<bool> {
        match unsafe { WaitForSingleObject(HANDLE(self.event.as_raw_handle()), 0) } {
            WAIT_TIMEOUT => Ok(false),
            WAIT_OBJECT_0 => {
                self.arm()?;
                Ok(true)
            }
            _ => Err(windows::core::Error::from_thread()),
        }
    }
}

struct Entry {
    value: String,
    stored: Instant,
}
struct Cache {
    watches: Option<Vec<Watch>>,
    entries: BTreeMap<String, Entry>,
    revision: u64,
}
impl Cache {
    fn new(roots: &[(RegKey, &str)]) -> Self {
        let watches = roots
            .iter()
            .map(|(key, path)| Watch::new(key, path))
            .collect::<std::io::Result<Vec<_>>>()
            .ok();
        Self {
            watches,
            entries: BTreeMap::new(),
            revision: next_revision(),
        }
    }
    fn production() -> Self {
        // Classes、FileExtsのUserChoice、ポリシー等を含む上位キーを監視する。
        // 細いキーの未作成・削除・再作成によって監視対象を失わない。
        Self::new(&[
            (RegKey::predef(HKEY_CURRENT_USER), "Software"),
            (RegKey::predef(HKEY_LOCAL_MACHINE), "Software"),
        ])
    }
    fn refresh(&mut self) -> bool {
        let Some(watches) = self.watches.as_ref() else {
            return false;
        };
        let result = watches.iter().try_fold(false, |changed, watch| {
            watch.poll().map(|next| changed | next)
        });
        match result {
            Ok(false) => true,
            result => {
                self.entries.clear();
                self.revision = next_revision();
                if result.is_err() {
                    self.watches = None;
                }
                self.watches.is_some()
            }
        }
    }
    fn lookup(&mut self, key: &str) -> (Option<String>, u64) {
        let hit = if self.refresh() {
            self.entries
                .get(key)
                .filter(|entry| entry.stored.elapsed() < LIFETIME)
                .map(|entry| entry.value.clone())
        } else {
            None
        };
        (hit, self.revision)
    }
    fn store(&mut self, key: String, value: &str, revision: u64) {
        if value.len() <= MAX_TEXT && self.refresh() && self.revision == revision {
            if self.entries.len() >= CAPACITY {
                self.entries.clear();
            }
            self.entries.insert(
                key,
                Entry {
                    value: value.to_owned(),
                    stored: Instant::now(),
                },
            );
        }
    }
}

fn state() -> MutexGuard<'static, Option<Cache>> {
    STATE.lock().unwrap_or_else(|error| {
        let mut state = error.into_inner();
        *state = None;
        STATE.clear_poison();
        state
    })
}

pub(super) fn resolve<E>(
    extension: &str,
    query: impl FnOnce(&str) -> Result<String, E>,
) -> Result<String, E> {
    if extension.is_empty() || extension.len() > MAX_TEXT {
        return query(extension);
    }
    let key = extension.to_ascii_lowercase();
    let (hit, revision) = { state().get_or_insert_with(Cache::production).lookup(&key) };
    if let Some(value) = hit {
        return Ok(value);
    }
    // Windows側の処理中にCOMが再入しても、このミューテックスを保持しない。
    let value = query(&key)?;
    if let Some(cache) = state().as_mut() {
        cache.store(key, &value, revision);
    }
    Ok(value)
}

/// COMがアンロード可能と判断する前に、キーとイベントを解放する。
pub(super) fn release() {
    state().take();
}

#[cfg(test)]
mod tests;
