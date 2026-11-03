use serde::Deserialize;

#[derive(Deserialize)]
pub(crate) struct Catalog {
    pub languages: Vec<Language>,
}

#[derive(Deserialize)]
pub(crate) struct Language {
    pub id: String,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub filenames: Vec<String>,
    #[serde(default, rename = "filenamePatterns", alias = "patterns")]
    pub patterns: Vec<String>,
}
