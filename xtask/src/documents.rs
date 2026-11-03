use anyhow::{Result, ensure};
use percent_encoding::percent_decode_str;
use pulldown_cmark::{Event, LinkType, Parser, Tag};
use std::{fs, path::Path};

/// 配布する案内文書のリンクを、実際に同梱したファイルへ照合する。
pub fn verify(root: &Path) -> Result<usize> {
    let root = root.canonicalize()?;
    let mut documents = Vec::new();
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        if entry.file_type()?.is_file()
            && entry.path().extension().and_then(|v| v.to_str()) == Some("md")
        {
            documents.push(entry.path());
        }
    }
    if root.join("docs").is_dir() {
        documents.extend(
            crate::package::files(&root.join("docs"))?
                .into_iter()
                .filter(|p| p.extension().and_then(|v| v.to_str()) == Some("md"))
                .map(|p| root.join("docs").join(p)),
        );
    }
    let mut count = 0;
    for document in documents {
        let text = fs::read_to_string(&document)?;
        let parser = Parser::new(&text);
        let mut targets: Vec<_> = parser
            .reference_definitions()
            .iter()
            .map(|(_, definition)| definition.dest.to_string())
            .collect();
        for event in parser {
            let Event::Start(
                Tag::Link {
                    dest_url,
                    link_type,
                    ..
                }
                | Tag::Image {
                    dest_url,
                    link_type,
                    ..
                },
            ) = event
            else {
                continue;
            };
            if matches!(
                link_type,
                LinkType::Inline | LinkType::Autolink | LinkType::Email
            ) {
                targets.push(dest_url.to_string());
            }
        }
        for link in targets {
            let link = link.as_str();
            if link.starts_with("//")
                || link
                    .split(['#', '?', '/'])
                    .next()
                    .unwrap_or("")
                    .contains(':')
            {
                continue;
            }
            let path = link.split(['#', '?']).next().unwrap();
            if path.is_empty() {
                continue;
            }
            let decoded = percent_decode_str(path).decode_utf8()?;
            let target = document.parent().unwrap().join(decoded.as_ref());
            ensure!(
                target
                    .canonicalize()
                    .is_ok_and(|p| p.starts_with(&root) && p.is_file()),
                "broken bundled document link: {} -> {link}",
                document.strip_prefix(&root)?.display()
            );
            count += 1;
        }
    }
    Ok(count)
}
