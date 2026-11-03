//! パックごとの下書きと取り消し履歴。
use super::{BTreeSet, Selection, Target};
use file_icons_core::CompiledTheme;

pub(super) struct Draft {
    pub(super) value: Selection,
    undo: Vec<Vec<(String, Option<bool>)>>,
}

impl Draft {
    pub(super) fn new(value: Selection) -> Self {
        Self {
            value,
            undo: Vec::new(),
        }
    }

    fn remember(&mut self, previous: Vec<(String, Option<bool>)>) {
        if !previous.is_empty() {
            self.undo.push(previous);
            if self.undo.len() > 32 {
                self.undo.remove(0);
            }
        }
    }

    pub(super) fn edit(&mut self, ids: &[String], value: Option<bool>) {
        let mut previous = Vec::new();
        for id in ids {
            let before = self.value.explicit(id);
            if before != value
                && self.value.set(id, value).is_ok()
                && before != self.value.explicit(id)
            {
                previous.push((id.clone(), before));
            }
        }
        self.remember(previous);
    }

    pub(super) fn replace(&mut self, value: Selection, targets: &[Target]) {
        if self.value != value {
            let previous = targets
                .iter()
                .filter_map(|target| {
                    let before = self.value.explicit(&target.id);
                    (before != value.explicit(&target.id)).then(|| (target.id.clone(), before))
                })
                .collect();
            self.remember(previous);
            self.value = value;
        }
    }

    pub(super) fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub(super) fn undo(&mut self) {
        if let Some(previous) = self.undo.pop() {
            for (id, value) in previous.into_iter().rev() {
                let _ = self.value.set(&id, value);
            }
        }
    }

    pub(super) fn reset(&mut self, value: Selection) {
        self.value = value;
        self.undo.clear();
    }

    pub(super) fn reconcile(&mut self, theme: &CompiledTheme) {
        self.value.retain_supported(theme);
        let supported: BTreeSet<_> = theme
            .targets()
            .into_iter()
            .map(|target| target.id)
            .collect();
        for previous in &mut self.undo {
            previous.retain(|(id, _)| supported.contains(id));
        }
    }
}
