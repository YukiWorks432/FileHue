//! JSONの出力上限を、巨大な一時文字列を作る前に適用する。
use anyhow::Result;
use serde::Serialize;
use std::io::{self, Write};

pub fn encode(value: &impl Serialize, limit: usize) -> Result<Vec<u8>> {
    struct Bounded {
        bytes: Vec<u8>,
        limit: usize,
    }
    impl Write for Bounded {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
                return Err(io::Error::other("JSON output exceeds byte limit"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut output = Bounded {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer_pretty(&mut output, value)?;
    Ok(output.bytes)
}

#[cfg(test)]
mod tests {
    #[test]
    fn escaping_is_counted_in_the_output_budget() {
        let text = "\u{0001}".repeat(100);
        assert!(super::encode(&text, 128).is_err());
        let encoded = super::encode(&"日本語", 32).unwrap();
        assert_eq!(
            serde_json::from_slice::<String>(&encoded).unwrap(),
            "日本語"
        );
    }
}
