//! レジストリ変更前に保存する復元記録。既定アプリのキーは対象にしない。
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Value {
    pub kind: u32,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    pub key: String,
    pub name: String,
    pub before: Option<Value>,
    pub after: Value,
}

#[derive(Debug)]
pub struct Conflict(pub String);
impl std::fmt::Display for Conflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Conflict {}

pub trait Registry {
    fn read(&self, key: &str, name: &str) -> Result<Option<Value>>;
    fn write(&mut self, key: &str, name: &str, value: Option<&Value>) -> Result<()>;
}

/// 完全な事前記録を永続化してから変更する。
/// 途中失敗時も記録を残し、復元または同一計画の再試行を可能にする。
pub fn apply(
    registry: &mut impl Registry,
    changes: &[Change],
    save: impl FnOnce(&[Change]) -> Result<()>,
) -> Result<()> {
    for change in changes {
        ensure!(
            registry.read(&change.key, &change.name)? == change.before,
            Conflict(format!("registry changed before apply: {}", change.key))
        );
    }
    save(changes)?;
    for change in changes {
        registry.write(&change.key, &change.name, Some(&change.after))?;
    }
    Ok(())
}

/// 本製品が書いた値だけを戻す。後から他者が変更した値は保持して競合を返す。
/// 部分適用・部分復元の後も再実行できる。
pub fn restore(registry: &mut impl Registry, changes: &[Change]) -> Result<()> {
    let mut conflicts = Vec::new();
    for change in changes.iter().rev() {
        let current = registry.read(&change.key, &change.name)?;
        if current == change.before {
            continue;
        }
        if current.as_ref() == Some(&change.after) {
            registry.write(&change.key, &change.name, change.before.as_ref())?;
        } else {
            conflicts.push(change.key.clone());
        }
    }
    ensure!(
        conflicts.is_empty(),
        Conflict(format!("restore conflicts: {}", conflicts.join(", ")))
    );
    Ok(())
}

/// 関連する登録をすべて照合し、競合がなければ逆順で復元する。
/// 復元中の書き込み失敗に備え、呼び出し元は記録を完了まで保持する。
pub fn restore_group(registry: &mut impl Registry, changes: &[Change]) -> Result<()> {
    for change in changes {
        let current = registry.read(&change.key, &change.name)?;
        ensure!(
            current == change.before || current.as_ref() == Some(&change.after),
            Conflict(format!(
                "registry group changed before restore: {}",
                change.key
            ))
        );
    }
    restore(registry, changes)
}
