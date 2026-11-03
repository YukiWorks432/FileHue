//! 復元記録が変更できる登録先を、復旧方法によらず制限する。
use anyhow::{Result, ensure};
use file_icons_core::registry::Change;
use std::collections::BTreeSet;

/// 読み込んだ記録に、起動設定や無関係な値を混ぜて確認を迂回させない。
pub(crate) fn validate_key(key: &str, name: &str) -> Result<()> {
    let clsid = format!(
        "Software\\Classes\\CLSID\\{}\\InprocServer32",
        crate::CLSID_TEXT
    );
    if key == clsid && matches!(name, "" | "ThreadingModel") {
        return Ok(());
    }
    let parts = key.split('\\').collect::<Vec<_>>();
    let valid = name.is_empty()
        && parts.first() == Some(&"Software")
        && parts.get(1) == Some(&"Classes")
        && parts.get(2).is_some_and(|class| {
            !class.is_empty()
                && !class.contains(['/', '\0'])
                && !matches!(*class, "." | "..")
                && !class.eq_ignore_ascii_case("CLSID")
        })
        && ((parts.len() == 4 && parts[3] == "DefaultIcon")
            || (parts.len() == 5 && parts[3] == "shellex" && parts[4] == "IconHandler"));
    ensure!(valid, "invalid recovery registration scope: {key} [{name}]");
    Ok(())
}
pub(crate) fn validate(journal: &[Change], created_keys: &[String]) -> Result<()> {
    let mut seen = BTreeSet::new();
    for change in journal {
        validate_key(&change.key, &change.name)?;
        ensure!(
            seen.insert((change.key.to_lowercase(), change.name.to_lowercase())),
            "duplicate recovery registration"
        );
    }
    for key in created_keys {
        ensure!(
            journal
                .iter()
                .any(|change| change.key == *key || change.key.starts_with(&format!("{key}\\"))),
            "unrelated recovery directory"
        );
        ensure!(
            key.starts_with("Software\\Classes\\") && !key.contains(['/', '\0']),
            "invalid recovery directory"
        );
    }
    Ok(())
}
