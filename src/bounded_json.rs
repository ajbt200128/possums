use serde::Serialize;
use std::io::{self, Write};

struct BoundedWriter {
    bytes: Vec<u8>,
    limit: usize,
}

impl Write for BoundedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other("JSON exceeds transport limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(crate) fn to_vec(value: &impl Serialize, limit: usize) -> Result<Vec<u8>, serde_json::Error> {
    let mut writer = BoundedWriter {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer(&mut writer, value)?;
    Ok(writer.bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_expansion_before_exceeding_limit() {
        let value = "\0".repeat(4);
        assert_eq!(
            to_vec(&value, 26).unwrap(),
            b"\"\\u0000\\u0000\\u0000\\u0000\""
        );
        assert!(to_vec(&value, 25).is_err());
    }
}
