//! 操作の対象・進捗・結果を一つの境界から更新する。
use crate::status::Status;
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
