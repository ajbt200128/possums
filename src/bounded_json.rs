use serde::Serialize;
use std::io::{self, Write};

#[derive(Clone, Copy, Debug, thiserror::Error, PartialEq, Eq)]
pub(crate) enum EncodingFailure {
    #[error("JSON encoding exceeded the transport limit.")]
    LimitExceeded,
    #[error("JSON encoding failed; the underlying cause is unknown.")]
    CauseUnknown,
}

struct BoundedWriter {
    bytes: Vec<u8>,
    limit: usize,
    limit_exceeded: bool,
}

impl Write for BoundedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            self.limit_exceeded = true;
            return Err(io::Error::other("JSON exceeds transport limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(crate) fn to_vec(value: &impl Serialize, limit: usize) -> Result<Vec<u8>, EncodingFailure> {
    let mut writer = BoundedWriter {
        bytes: Vec::new(),
        limit,
        limit_exceeded: false,
    };
    serde_json::to_writer(&mut writer, value).map_err(|_| {
        if writer.limit_exceeded {
            EncodingFailure::LimitExceeded
        } else {
            EncodingFailure::CauseUnknown
        }
    })?;
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
        assert_eq!(to_vec(&value, 25), Err(EncodingFailure::LimitExceeded));
    }

    struct HostileSerializer;
    impl Serialize for HostileSerializer {
        fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
            Err(serde::ser::Error::custom(
                "PRIVATE_CREDENTIAL_URL_PROMPT_ENCODING_FAILURE",
            ))
        }
    }

    #[test]
    fn unexpected_serializer_failure_keeps_only_the_closed_category() {
        let failure = to_vec(&HostileSerializer, 4096).unwrap_err();
        assert_eq!(failure, EncodingFailure::CauseUnknown);
        assert!(!format!("{failure:?} {failure}").contains("PRIVATE_"));
        assert_eq!(to_vec(&"", 0), Err(EncodingFailure::LimitExceeded));
    }
}
