//! Private, versioned, length-prefixed protocol. stdout is reserved for frames.
use bulkparse::{Conversion, Options};
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Seek, Write};
use std::path::PathBuf;

pub const VERSION: u32 = 1;
pub const MAX_REQUEST: usize = 16 * 1024;
pub const MAX_RESPONSE: usize = 1024 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub id: u64,
    pub path: PathBuf,
    pub options: Options,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Reply {
    Ready {
        version: u32,
    },
    Converted {
        id: u64,
        conversion: Box<Conversion>,
    },
    Failed {
        id: u64,
        code: String,
        message: String,
    },
}

impl<'de> Deserialize<'de> for Reply {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // An internally tagged derived enum buffers the entire conversion in a
        // generic tree first. Decode the typed payload as it arrives instead.
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            #[serde(rename = "type")]
            kind: String,
            #[serde(default, deserialize_with = "present")]
            version: Option<u32>,
            #[serde(default, deserialize_with = "present")]
            id: Option<u64>,
            #[serde(default, deserialize_with = "present")]
            conversion: Option<Box<Conversion>>,
            #[serde(default, deserialize_with = "present")]
            code: Option<String>,
            #[serde(default, deserialize_with = "present")]
            message: Option<String>,
        }
        fn required<T, E: serde::de::Error>(v: Option<T>, name: &'static str) -> Result<T, E> {
            v.ok_or_else(|| E::missing_field(name))
        }
        fn present<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
            d: D,
        ) -> Result<Option<T>, D::Error> {
            T::deserialize(d).map(Some)
        }
        let w = Wire::deserialize(d)?;
        match w.kind.as_str() {
            "ready"
                if w.id.is_none()
                    && w.conversion.is_none()
                    && w.code.is_none()
                    && w.message.is_none() =>
            {
                Ok(Self::Ready {
                    version: required(w.version, "version")?,
                })
            }
            "converted" if w.version.is_none() && w.code.is_none() && w.message.is_none() => {
                Ok(Self::Converted {
                    id: required(w.id, "id")?,
                    conversion: required(w.conversion, "conversion")?,
                })
            }
            "failed" if w.version.is_none() && w.conversion.is_none() => Ok(Self::Failed {
                id: required(w.id, "id")?,
                code: required(w.code, "code")?,
                message: required(w.message, "message")?,
            }),
            _ => Err(serde::de::Error::custom("invalid reply type or fields")),
        }
    }
}

pub fn read_frame(reader: &mut impl Read, limit: usize) -> io::Result<Vec<u8>> {
    let mut header = [0; 4];
    reader.read_exact(&mut header)?;
    let len = u32::from_le_bytes(header) as usize;
    if len == 0 || len > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid frame size",
        ));
    }
    let mut bytes = vec![0; len];
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}

pub fn write_frame(writer: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
    let len = u32::try_from(bytes.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "frame too large"))?;
    writer.write_all(&len.to_le_bytes())?;
    writer.write_all(bytes)?;
    writer.flush()
}

/// Decode within the frame boundary without retaining a second response-sized
/// byte buffer. A truncated body or trailing non-JSON data remains an error.
#[allow(dead_code)] // Shared module: the child uses the write helpers only.
pub fn read_reply(reader: &mut impl Read, limit: usize) -> io::Result<Reply> {
    let mut header = [0; 4];
    reader.read_exact(&mut header)?;
    let len = u32::from_le_bytes(header) as usize;
    if len == 0 || len > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid frame size",
        ));
    }
    let mut body = reader.take(len as u64);
    let result = serde_json::from_reader(io::BufReader::with_capacity(64 * 1024, &mut body))
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    if body.limit() != 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "truncated frame",
        ));
    }
    Ok(result)
}

/// Stage a bounded reply, spilling after 64 KiB. Taking ownership drops the
/// source model before transmission/parent decoding, rather than keeping it
/// alive alongside an entire serialized byte buffer and the parent's model.
pub fn stage_reply(value: Reply, limit: usize) -> io::Result<tempfile::SpooledTempFile> {
    struct Capped<W> {
        writer: W,
        remaining: usize,
    }
    impl<W: Write> Write for Capped<W> {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if bytes.len() > self.remaining {
                return Err(io::Error::new(
                    io::ErrorKind::FileTooLarge,
                    "frame limit exceeded",
                ));
            }
            let n = self.writer.write(bytes)?;
            self.remaining -= n;
            Ok(n)
        }
        fn flush(&mut self) -> io::Result<()> {
            self.writer.flush()
        }
    }
    let mut spool = tempfile::spooled_tempfile(64 * 1024);
    {
        let bounded = Capped {
            writer: &mut spool,
            remaining: limit,
        };
        let mut writer = io::BufWriter::with_capacity(64 * 1024, bounded);
        serde_json::to_writer(&mut writer, &value).map_err(|e| {
            io::Error::new(e.io_error_kind().unwrap_or(io::ErrorKind::InvalidData), e)
        })?;
        writer.flush()?;
    }
    // `value` is dropped on return, before sending the staged frame.
    Ok(spool)
}

pub fn write_staged_frame(
    writer: &mut impl Write,
    spool: &mut tempfile::SpooledTempFile,
) -> io::Result<()> {
    let len = spool.seek(io::SeekFrom::End(0))?;
    let len = u32::try_from(len).map_err(|_| io::Error::other("frame too large"))?;
    spool.rewind()?;
    writer.write_all(&len.to_le_bytes())?;
    io::copy(spool, writer)?;
    writer.flush()
}

/// Bound the serialization buffer as well as the eventual wire frame.
pub fn encode(value: &impl Serialize, limit: usize) -> Result<Vec<u8>, serde_json::Error> {
    struct Capped {
        bytes: Vec<u8>,
        limit: usize,
    }
    impl Write for Capped {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
                return Err(io::Error::other("frame limit exceeded"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut writer = Capped {
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
    fn spilled_reply_round_trip_preserves_following_frame() {
        let reply = Reply::Failed {
            id: 7,
            code: "test".into(),
            message: "large".repeat(30_000),
        };
        let expected = serde_json::to_value(&reply).unwrap();
        let mut spool = stage_reply(reply, 200_000).unwrap();
        assert!(spool.is_rolled());
        let mut wire = Vec::new();
        write_staged_frame(&mut wire, &mut spool).unwrap();
        write_frame(&mut wire, br#"{"type":"ready","version":1}"#).unwrap();
        let mut reader = io::Cursor::new(wire);
        assert_eq!(
            serde_json::to_value(read_reply(&mut reader, 200_000).unwrap()).unwrap(),
            expected
        );
        assert!(matches!(
            read_reply(&mut reader, 200_000).unwrap(),
            Reply::Ready { version: 1 }
        ));
    }
    #[test]
    fn typed_reply_accepts_reordered_fields_and_rejects_foreign_null_fields() {
        let conversion =
            bulkparse::parse(b"a,b\n1,2\n", Some("a.csv"), bulkparse::Options::default()).unwrap();
        let original = Reply::Converted {
            id: 3,
            conversion: Box::new(conversion),
        };
        let expected = serde_json::to_value(&original).unwrap();
        // Value's sorted map keys place type tags after payload fields.
        let mut wire = Vec::new();
        write_frame(&mut wire, &serde_json::to_vec(&expected).unwrap()).unwrap();
        let decoded = read_reply(&mut wire.as_slice(), 100_000).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), expected);
        for bad in [
            r#"{"type":"ready","version":1,"id":null}"#,
            r#"{"type":"ready","version":1,"extra":null}"#,
        ] {
            assert!(serde_json::from_str::<Reply>(bad).is_err());
        }
    }
    #[test]
    fn bounded_reply_rejects_limit_truncation_and_trailing_junk() {
        let reply = || Reply::Ready { version: 1 };
        let len = serde_json::to_vec(&reply()).unwrap().len();
        assert!(stage_reply(reply(), len).is_ok());
        assert_eq!(
            stage_reply(reply(), len - 1).unwrap_err().kind(),
            io::ErrorKind::FileTooLarge
        );
        let payload = br#"{"type":"ready","version":1}"#;
        for suffix in [b"x".as_slice(), b"{}".as_slice()] {
            let mut wire = Vec::new();
            write_frame(&mut wire, &[payload.as_slice(), suffix].concat()).unwrap();
            assert!(read_reply(&mut wire.as_slice(), 1000).is_err());
        }
        let mut wire = Vec::new();
        wire.extend_from_slice(&((payload.len() + 1) as u32).to_le_bytes());
        wire.extend_from_slice(payload);
        assert!(read_reply(&mut wire.as_slice(), 1000).is_err());
        let mut wire = Vec::new();
        write_frame(&mut wire, payload).unwrap();
        assert!(read_reply(&mut wire.as_slice(), payload.len() - 1).is_err());
    }
}
