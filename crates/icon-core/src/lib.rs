//! テーマ定義の取り込みと、Explorerから独立したアイコン解決。

pub mod bounded_json;
mod categories;
mod language_catalog;
pub mod recommendations;
pub mod registry;
pub mod storage;
pub mod theme;

pub use theme::{
    CompiledTheme, ImportWarning, ImportedPack, ThemeChoice, ThemeRules, discover, import,
    import_controlled, import_observed,
};

pub mod selection;

#[cfg(test)]
#[path = "../../../test-support/performance.rs"]
mod performance;
