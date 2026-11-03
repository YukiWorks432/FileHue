//! 開発専用の配布・検証処理。製品の通常依存には含めない。
pub mod documents;
pub mod notices;
pub mod package;
pub mod privacy;
pub mod process;
pub mod release;

pub fn root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}
