//! 開発者向けの保護設定で、既存アイコンよりパックを優先する拡張子。

pub fn development_extension(extension: &str) -> bool {
    let extension = extension.to_lowercase();
    if matches!(extension.as_str(), "woff" | "woff2" | "ttf" | "otf") {
        return false;
    }
    matches!(
        crate::categories::category("extension", &extension),
        "language" | "web" | "build" | "tools"
    ) || matches!(
        extension.as_str(),
        "json"
            | "jsonc"
            | "json5"
            | "jsonl"
            | "yaml"
            | "yml"
            | "toml"
            | "xml"
            | "ini"
            | "properties"
            | "env"
            | "md"
            | "mdx"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recommendations_cover_sources_and_development_settings() {
        for extension in [
            "c", "cpp", "h", "cs", "rs", "go", "py", "java", "kt", "swift", "rb", "lua", "dart",
            "sql", "ps1", "sh", "js", "jsx", "ts", "tsx", "mts", "cts", "d.ts", "html", "css",
            "php", "vue", "svelte", "json", "yaml", "toml", "csproj", "sln", "diff", "patch", "md",
            "mdx", "PY",
        ] {
            assert!(development_extension(extension), "{extension}");
        }
    }

    #[test]
    fn recommendations_leave_assets_and_general_documents_protected() {
        for extension in [
            "woff",
            "woff2",
            "ttf",
            "otf",
            "png",
            "psd",
            "mp4",
            "m2ts",
            "mp3",
            "pdf",
            "docx",
            "csv",
            "sqlite",
            "txt",
            "exe",
            "lnk",
            "",
            "unrecognized",
        ] {
            assert!(!development_extension(extension), "{extension}");
        }
    }
}
