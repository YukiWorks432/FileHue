use file_icons_core::registry::{self, Change, Registry, Value};
use std::{cell::Cell, collections::BTreeMap};

#[derive(Default)]
struct Store {
    values: BTreeMap<(String, String), Value>,
    writes: usize,
    fail_at: Option<usize>,
}
impl Registry for Store {
    fn read(&self, key: &str, name: &str) -> anyhow::Result<Option<Value>> {
        Ok(self.values.get(&(key.into(), name.into())).cloned())
    }
    fn write(&mut self, key: &str, name: &str, value: Option<&Value>) -> anyhow::Result<()> {
        self.writes += 1;
        anyhow::ensure!(self.fail_at != Some(self.writes), "simulated write failure");
        let key = (key.into(), name.into());
        if let Some(value) = value {
            self.values.insert(key, value.clone());
        } else {
            self.values.remove(&key);
        }
        Ok(())
    }
}
fn value(text: &str) -> Value {
    Value {
        kind: 1,
        bytes: text.as_bytes().into(),
    }
}
fn changes() -> Vec<Change> {
    vec![
        Change {
            key: "Classes/Test/DefaultIcon".into(),
            name: "".into(),
            before: Some(value("original")),
            after: value("%1"),
        },
        Change {
            key: "Classes/Test/ShellEx/IconHandler".into(),
            name: "".into(),
            before: None,
            after: value("our-handler"),
        },
    ]
}
fn original() -> Store {
    let mut registry = Store::default();
    registry
        .write("Classes/Test/DefaultIcon", "", Some(&value("original")))
        .unwrap();
    registry
        .write("Classes/.json", "", Some(&value("original-default-app")))
        .unwrap();
    registry.writes = 0;
    registry
}
#[test]
fn saves_before_writes_and_roundtrips_without_touching_default_application() {
    let mut registry = original();
    let before = registry.values.clone();
    let saved = Cell::new(false);
    registry::apply(&mut registry, &changes(), |_| {
        saved.set(true);
        Ok(())
    })
    .unwrap();
    assert!(saved.get());
    assert_eq!(
        registry.read("Classes/.json", "").unwrap(),
        Some(value("original-default-app"))
    );
    registry::restore(&mut registry, &changes()).unwrap();
    assert_eq!(registry.values, before);
    registry::restore(&mut registry, &changes()).unwrap();
}
#[test]
fn failed_backup_makes_no_changes_and_partial_install_can_be_restored() {
    let mut registry = original();
    let before = registry.values.clone();
    assert!(registry::apply(&mut registry, &changes(), |_| anyhow::bail!("disk full")).is_err());
    assert_eq!(registry.values, before);
    registry.fail_at = Some(2);
    assert!(registry::apply(&mut registry, &changes(), |_| Ok(())).is_err());
    registry.fail_at = None;
    registry::restore(&mut registry, &changes()).unwrap();
    assert_eq!(registry.values, before);
}
#[test]
fn refuses_stale_install_plan_and_preserves_external_edit_during_restore() {
    let mut registry = original();
    registry::apply(&mut registry, &changes(), |_| Ok(())).unwrap();
    registry
        .write("Classes/Test/DefaultIcon", "", Some(&value("another-app")))
        .unwrap();
    assert!(registry::restore(&mut registry, &changes()).is_err());
    assert_eq!(
        registry.read("Classes/Test/DefaultIcon", "").unwrap(),
        Some(value("another-app"))
    );
    assert_eq!(
        registry
            .read("Classes/Test/ShellEx/IconHandler", "")
            .unwrap(),
        None
    );
    assert!(registry::apply(&mut registry, &changes(), |_| Ok(())).is_err());
}
