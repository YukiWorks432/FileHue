//! JSON5の再帰的な復号へ入る前に、構造の深さを制限する。
use anyhow::{Result, ensure};

pub(super) fn check(text: &str) -> Result<()> {
    let mut chars = text.chars().peekable();
    let mut depth = 0usize;
    while let Some(c) = chars.next() {
        match c {
            '\'' | '"' => {
                while let Some(next) = chars.next() {
                    if next == '\\' {
                        chars.next();
                    } else if next == c {
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'/') => {
                chars.next();
                for next in chars.by_ref() {
                    if matches!(next, '\n' | '\r' | '\u{2028}' | '\u{2029}') {
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                while let Some(next) = chars.next() {
                    if next == '*' && chars.peek() == Some(&'/') {
                        chars.next();
                        break;
                    }
                }
            }
            '{' | '[' => {
                depth += 1;
                ensure!(depth <= 64, "JSON5 nesting limit exceeded");
            }
            '}' | ']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    // 構文の正否はjson5へ委ねる。文字列やコメントの括弧は構造として数えない。
    Ok(())
}
