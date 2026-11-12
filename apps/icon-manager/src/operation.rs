//! 操作の対象・進捗・結果を一つの境界から更新する。
use crate::status::{Failure, Status};
use crate::{Context, Manager, Settings, load};
use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    General,
    Choose,
    Import,
    Download(&'static str),
    Apply,
    SaveTargets,
    SaveProtection,
    Restore,
    Remove,
    Cleanup,
    Language,
    Details,
    InspectConflicts,
    ResolveConflicts,
    Thumbnails,
}

pub struct Operation {
    kind: Kind,
    status: Status,
    progress: Option<Arc<file_icons_catalog::Progress>>,
    suspended: Option<(Kind, Status)>,
}
impl Default for Operation {
    fn default() -> Self {
        Self::new(Status::Idle)
    }
}
impl Operation {
    pub fn new(status: Status) -> Self {
        Self {
            kind: Kind::General,
            status,
            progress: None,
            suspended: None,
        }
    }
    pub fn busy(&self) -> bool {
        matches!(self.status, Status::Working)
    }
    pub fn running(&self, kind: Kind) -> bool {
        self.busy() && self.kind == kind
    }
    pub fn failed(&self) -> bool {
        matches!(self.status(), Status::Error(..))
    }
    pub fn status(&self) -> &Status {
        self.suspended
            .as_ref()
            .map(|(_, status)| status)
            .unwrap_or(&self.status)
    }
    pub fn progress(&self) -> Option<&Arc<file_icons_catalog::Progress>> {
        self.progress.as_ref()
    }
    pub fn retry_download(&self, id: &str) -> bool {
        self.failed() && matches!(self.kind, Kind::Download(target) if target == id)
    }
    pub fn begin(&mut self, kind: Kind, progress: Option<Arc<file_icons_catalog::Progress>>) {
        self.suspended = (kind == Kind::Language).then(|| {
            (
                self.kind,
                std::mem::replace(&mut self.status, Status::Working),
            )
        });
        self.kind = kind;
        self.status = Status::Working;
        self.progress = progress;
    }
    pub fn finish(&mut self, status: Status) {
        if self.kind == Kind::Language
            && matches!(status, Status::Complete)
            && let Some((kind, previous)) = self.suspended.take()
        {
            self.kind = kind;
            self.status = previous;
        } else {
            self.suspended = None;
            self.status = status;
        }
        self.progress = None;
    }
}

impl Manager {
    pub(super) fn run(
        &mut self,
        kind: Kind,
        operation: impl FnOnce() -> Result<()> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        self.start(kind, operation, false, None, cx);
    }

    pub(super) fn start(
        &mut self,
        kind: Kind,
        operation: impl FnOnce() -> Result<()> + Send + 'static,
        recovery: bool,
        progress: Option<std::sync::Arc<file_icons_catalog::Progress>>,
        cx: &mut Context<Self>,
    ) {
        let can_recover = recovery && self._guard.is_some() && !self.settings.journal.is_empty();
        if self.operation.busy() || (!self.valid && !can_recover) {
            return;
        }
        self.operation.begin(kind, progress);
        self.progress_refresh = self.operation.progress().map(|_| {
            // 画面更新だけを共有する。完了・画面破棄でTaskを落としてタイマーも止める。
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(100))
                        .await;
                    if this.update(cx, |_, cx| cx.notify()).is_err() {
                        break;
                    }
                }
            })
        });
        let backup_hint = if kind == Kind::ResolveConflicts {
            self.recovery.backup_hint()
        } else {
            None
        };
        let guard = self._guard.clone();
        let task = cx.background_executor().spawn(async move {
            let _guard = guard;
            let result = operation();
            let loaded = load();
            let actual_backup = if result.is_ok() {
                backup_hint.and_then(|path| path.canonicalize().ok())
            } else {
                None
            };
            (result, loaded, actual_backup)
        });
        cx.spawn(async move |this, cx| {
            let (result, loaded, actual_backup) = task.await;
            let _ = this.update(cx, |this, cx| {
                this.finish_operation(kind, result, loaded, actual_backup, cx);
            });
        })
        .detach();
        cx.notify();
    }

    fn finish_operation(
        &mut self,
        kind: Kind,
        result: Result<()>,
        loaded: Result<Settings>,
        actual_backup: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let operation_error = result.as_ref().err().map(|error| format!("{error:#}"));
        let status = match result {
            Ok(()) => {
                if kind == Kind::ResolveConflicts {
                    self.recovery.complete(actual_backup);
                }
                if kind == Kind::Import {
                    self.pending = None;
                }
                Status::Complete
            }
            Err(error) if error.is::<file_icons_catalog::Cancelled>() => Status::Cancelled,
            Err(error) => Status::Error(classify_failure(kind, &error), format!("{error:#}")),
        };
        if matches!(
            status,
            Status::Error(
                Failure::RegistrationConflict | Failure::IncompleteRecovery,
                _
            )
        ) {
            self.conflict_attention = true;
        } else if matches!(status, Status::Complete)
            && matches!(kind, Kind::Apply | Kind::Restore | Kind::ResolveConflicts)
        {
            self.conflict_attention = false;
        }
        self.operation.finish(status);
        self.progress_refresh = None;
        match loaded {
            Ok(settings) => {
                self.targets.reconcile(&settings);
                self.settings = settings;
                self.valid = self._guard.is_some();
            }
            Err(error) => {
                self.valid = false;
                let detail = match operation_error {
                    Some(operation) => format!("{operation}\n\n{error:#}"),
                    None => format!("{error:#}"),
                };
                self.operation
                    .finish(Status::Error(Failure::ReloadSettings, detail));
            }
        }
        if kind == Kind::SaveProtection && !self.operation.failed() {
            self.protection.draft.enabled = self.settings.preserve_existing_icons;
            self.protection.draft.rules = self.settings.icon_protection_rules.clone();
            self.inspect_protection(cx);
        }
        cx.notify();
    }
}

fn classify_failure(kind: Kind, error: &anyhow::Error) -> Failure {
    if error.is::<file_icons_windows::conflict::ConfirmationChanged>()
        || error.is::<file_icons_core::registry::Conflict>()
        || matches!(
            error.downcast_ref::<file_icons_manager_service::Failure>(),
            Some(file_icons_manager_service::Failure::Conflict)
        )
    {
        Failure::RegistrationConflict
    } else if error.is::<file_icons_windows::PreviousPackRestored>() {
        Failure::PreviousPackRestored
    } else if error.is::<file_icons_windows::ShellCleanupPending>() {
        Failure::ShellCleanupPending
    } else if error.is::<file_icons_windows::PackCleanupPending>() {
        Failure::PackCleanupPending
    } else if matches!(kind, Kind::Download(_)) {
        Failure::Download
    } else if kind == Kind::Language {
        Failure::SaveLanguage
    } else if kind == Kind::Import {
        Failure::Import
    } else {
        Failure::Operation
    }
}
