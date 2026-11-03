use super::*;
use file_icons_windows::{
    Settings,
    conflict::{self, Strategy},
};

pub(super) fn verify(
    classes: &RegKey,
    pack: &file_icons_core::ImportedPack,
    dll: &Path,
    original_icon: &str,
) -> Result<()> {
    let mut before = Snapshot::new();
    snapshot(classes, "", &mut before)?;
    let mut settings = Settings {
        preserve_existing_icons: false,
        packs: vec![pack.clone()],
        ..Default::default()
    };
    file_icons_windows::save(&settings)?;
    file_icons_windows::apply(&mut settings, &pack.id, dll)?;
    ensure!(
        settings
            .journal
            .iter()
            .filter(|c| c.key.to_lowercase() == "software\\classes\\wfiprobe.file\\defaulticon")
            .count()
            == 1,
        "duplicate class write"
    );
    ensure!(
        file_icons_windows::load()?.journal == settings.journal,
        "generated recovery record is unreadable"
    );
    let previous = settings
        .journal
        .iter()
        .find(|c| c.key.to_lowercase() == "software\\classes\\wfiprobe.file\\shellex\\iconhandler")
        .context("missing shared class")?
        .clone();
    // APIが返す綴りだけを変え、外部変更を新しい解除先にして再適用する。
    write(classes, ".wfi-known", "", "wfiprobe.file")?;
    write(classes, ".wfi-shared", "", "wfiprobe.file")?;
    let external = format!("{},1", original_icon.rsplit_once(',').unwrap().0);
    write(classes, "WfiProbe.File\\DefaultIcon", "", &external)?;
    let plan = conflict::preview(Strategy::ReapplyTheme, dll)?;
    ensure!(
        plan.entries
            .iter()
            .filter(|e| e.key.to_lowercase() == "software\\classes\\wfiprobe.file\\defaulticon")
            .count()
            == 1,
        "reapply duplicated a class"
    );
    ensure!(
        plan.entries
            .iter()
            .find(|e| e.key.to_lowercase() == previous.key.to_lowercase())
            .is_some_and(|e| e.original == previous.before),
        "reapply captured our handler as the original"
    );
    conflict::execute(Strategy::ReapplyTheme, &plan.confirmation, dll)?;
    let mut reapplied = file_icons_windows::load()?;
    file_icons_windows::restore(&mut reapplied)?;
    let restored: String = classes
        .open_subkey("WfiProbe.File\\DefaultIcon")?
        .get_value("")?;
    ensure!(
        restored == external,
        "restoration overwrote the external icon"
    );
    write(classes, ".wfi-known", "", "WfiProbe.File")?;
    write(classes, ".wfi-shared", "", "WFIPROBE.FILE")?;
    write(classes, "WfiProbe.File\\DefaultIcon", "", original_icon)?;
    let mut after = Snapshot::new();
    snapshot(classes, "", &mut after)?;
    ensure!(
        before == after,
        "case-only reapply did not restore the raw registration"
    );
    Ok(())
}
