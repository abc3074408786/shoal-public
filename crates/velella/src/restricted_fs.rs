//! Single-request filesystem operations, entered only after confinement.
//!
//! No request path may be inspected by the trusted controller. Landlock limits
//! file content access and writes in this child; it does not hide metadata,
//! stat results or symlink targets.

use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Map, Value};
use std::{
    fs::{self, DirBuilder, OpenOptions},
    io::{self, Read, Write},
    os::unix::{
        ffi::OsStrExt,
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
};

const MAX_FRAME: usize = 2 * 1024 * 1024;
const MAX_FILE: usize = 1024 * 1024;
const MAX_ENTRIES: usize = 4096;
const MAX_URI: usize = 16384;

fn invalid(message: impl Into<String>) -> anyhow::Error {
    anyhow::Error::from(io::Error::from_raw_os_error(libc::EINVAL)).context(message.into())
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn file_path(value: &Value) -> Result<PathBuf> {
    let uri = value.as_str().ok_or_else(|| invalid("invalid file URI"))?;
    if uri.len() > MAX_URI || uri.contains(['\0', '?', '#']) {
        return Err(invalid("invalid file URI"));
    }
    let encoded = uri
        .strip_prefix("file://")
        .filter(|path| path.starts_with('/'))
        .ok_or_else(|| invalid("only absolute file:/// URIs are supported"))?;
    let mut bytes = Vec::with_capacity(encoded.len());
    let input = encoded.as_bytes();
    let mut index = 0;
    while index < input.len() {
        if input[index] == b'%' {
            let high = input.get(index + 1).and_then(|byte| hex(*byte));
            let low = input.get(index + 2).and_then(|byte| hex(*byte));
            match (high, low) {
                (Some(high), Some(low)) => bytes.push(high * 16 + low),
                _ => return Err(invalid("invalid percent escape in file URI")),
            }
            index += 3;
        } else {
            bytes.push(input[index]);
            index += 1;
        }
    }
    if bytes.contains(&0) {
        return Err(invalid("file URI contains NUL"));
    }
    let decoded =
        String::from_utf8(bytes).map_err(|_| invalid("file URI must decode to valid UTF-8"))?;
    Ok(PathBuf::from(decoded))
}

fn file_uri(path: &Path) -> Result<String> {
    if !path.is_absolute() {
        return Err(invalid("canonical path is not absolute"));
    }
    let mut uri = String::from("file://");
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for byte in path.as_os_str().as_bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~".contains(byte) {
            uri.push(*byte as char);
        } else {
            uri.push('%');
            uri.push(HEX[(byte >> 4) as usize] as char);
            uri.push(HEX[(byte & 0x0f) as usize] as char);
        }
    }
    Ok(uri)
}

fn field<'a>(params: &'a Map<String, Value>, name: &str) -> Result<&'a Value> {
    params
        .get(name)
        .ok_or_else(|| invalid(format!("missing {name}")))
}

fn optional_bool(params: &Map<String, Value>, name: &str, default: bool) -> Result<bool> {
    match params.get(name) {
        None => Ok(default),
        Some(Value::Bool(value)) => Ok(*value),
        _ => Err(invalid(format!("invalid {name}"))),
    }
}

fn nullable_bool(params: &Map<String, Value>, name: &str) -> Result<bool> {
    match params.get(name) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(value)) => Ok(*value),
        _ => Err(invalid(format!("invalid {name}"))),
    }
}

fn decode_data(value: &Value) -> Result<Vec<u8>> {
    let encoded = value
        .as_str()
        .ok_or_else(|| invalid("invalid base64 payload"))?;
    if encoded.len() > MAX_FILE.div_ceil(3) * 4 {
        return Err(invalid("base64 payload exceeds limit"));
    }
    let data = STANDARD
        .decode(encoded)
        .map_err(|_| invalid("invalid base64 payload"))?;
    if data.len() > MAX_FILE {
        return Err(invalid("decoded payload exceeds limit"));
    }
    Ok(data)
}

fn timestamp_millis(seconds: i64, nanos: i64) -> Result<i64> {
    let millis = (i128::from(seconds) * 1_000_000_000 + i128::from(nanos)) / 1_000_000;
    i64::try_from(millis).map_err(|_| invalid("file timestamp exceeds range"))
}

fn operate(method: &str, raw: &[u8]) -> Result<Value> {
    if raw.len() > MAX_FRAME {
        return Err(invalid("filesystem request exceeds limit"));
    }
    let value: Value =
        serde_json::from_slice(raw).map_err(|_| invalid("invalid filesystem request JSON"))?;
    let params = value
        .as_object()
        .ok_or_else(|| invalid("filesystem request must be an object"))?;
    let path = file_path(field(params, "path")?)?;
    let follow = optional_bool(params, "followSymlinks", true)?;
    let flags = libc::O_NONBLOCK | if follow { 0 } else { libc::O_NOFOLLOW };

    match method {
        "fs/readFile" => {
            let file = OpenOptions::new()
                .read(true)
                .custom_flags(flags)
                .open(&path)
                .context("open file for reading")?;
            let metadata = file.metadata().context("inspect file for reading")?;
            if !metadata.is_file() {
                return Err(invalid("only regular files may be read"));
            }
            if metadata.len() > MAX_FILE as u64 {
                return Err(invalid("file exceeds limit"));
            }
            let mut data = Vec::with_capacity(metadata.len() as usize);
            file.take(MAX_FILE as u64 + 1)
                .read_to_end(&mut data)
                .context("read file")?;
            if data.len() > MAX_FILE {
                return Err(invalid("file exceeds limit"));
            }
            Ok(json!({ "dataBase64": STANDARD.encode(data) }))
        }
        "fs/writeFile" => {
            let data = decode_data(field(params, "dataBase64")?)?;
            // Do not truncate a FIFO, device or directory before confirming the
            // opened object is a regular file. O_NONBLOCK prevents FIFO hangs.
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .custom_flags(flags)
                .mode(0o600)
                .open(&path)
                .context("open file for writing")?;
            if !file
                .metadata()
                .context("inspect file for writing")?
                .is_file()
            {
                return Err(invalid("only regular files may be written"));
            }
            file.set_len(0).context("truncate file")?;
            file.write_all(&data).context("write file")?;
            Ok(json!({}))
        }
        "fs/getMetadata" => {
            let metadata = if follow {
                fs::metadata(&path)
            } else {
                fs::symlink_metadata(&path)
            }
            .context("read file metadata")?;
            Ok(json!({
                "isDirectory": metadata.is_dir(),
                "isFile": metadata.is_file(),
                "isSymlink": metadata.file_type().is_symlink(),
                "size": metadata.len(),
                // Preserve the experimental protocol's ctime convention. This
                // is the metadata change time on Linux, not a birth timestamp.
                "createdAtMs": timestamp_millis(metadata.ctime(), metadata.ctime_nsec())?,
                "modifiedAtMs": timestamp_millis(metadata.mtime(), metadata.mtime_nsec())?,
            }))
        }
        "fs/canonicalize" => {
            let canonical = fs::canonicalize(&path).context("canonicalize file path")?;
            Ok(json!({ "path": file_uri(&canonical)? }))
        }
        "fs/readDirectory" => {
            let mut entries = Vec::new();
            for entry in fs::read_dir(&path).context("read directory")? {
                if entries.len() >= MAX_ENTRIES {
                    return Err(invalid("directory entry limit exceeded"));
                }
                let entry = entry.context("read directory entry")?;
                let file_type = entry.file_type().context("inspect directory entry")?;
                let name = entry.file_name();
                let name = name
                    .to_str()
                    .ok_or_else(|| invalid("directory entry name is not valid UTF-8"))?;
                entries.push(json!({
                    "fileName": name,
                    "isDirectory": file_type.is_dir(),
                    "isFile": file_type.is_file(),
                }));
            }
            Ok(json!({ "entries": entries }))
        }
        "fs/createDirectory" => {
            DirBuilder::new()
                .mode(0o700)
                .recursive(nullable_bool(params, "recursive")?)
                .create(&path)
                .context("create directory")?;
            Ok(json!({}))
        }
        "fs/remove" => {
            if nullable_bool(params, "recursive")? {
                return Err(invalid("recursive removal is unsupported"));
            }
            let force = nullable_bool(params, "force")?;
            let result = match fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.is_dir() => fs::remove_dir(&path),
                Ok(_) => fs::remove_file(&path),
                Err(error) => Err(error),
            };
            match result {
                Ok(()) => Ok(json!({})),
                Err(error) if force && error.kind() == io::ErrorKind::NotFound => Ok(json!({})),
                Err(error) => Err(error).context("remove file or directory"),
            }
        }
        _ => Err(invalid("unsupported filesystem operation")),
    }
}

fn response(method: &str) -> Result<Vec<u8>> {
    let mut raw = Vec::new();
    io::stdin()
        .lock()
        .take(MAX_FRAME as u64 + 1)
        .read_to_end(&mut raw)
        .context("read filesystem request")?;
    let result = operate(method, &raw)?;
    let output = serde_json::to_vec(&result).context("encode filesystem response")?;
    if output.len() > MAX_FRAME {
        return Err(invalid("filesystem response exceeds limit"));
    }
    Ok(output)
}

/// Must be called only in a fresh, already-confined runtime child.
pub(super) fn run(method: &str) -> Result<()> {
    let (output, error) = match response(method) {
        Ok(output) => (output, None),
        Err(error) => {
            let errno = error
                .chain()
                .find_map(|cause| cause.downcast_ref::<io::Error>())
                .and_then(io::Error::raw_os_error)
                .unwrap_or(libc::EINVAL);
            let message: String = format!("{error:#}").chars().take(8192).collect();
            let output = serde_json::to_vec(&json!({
                "error": { "errno": errno, "message": message }
            }))?;
            (output, Some(error))
        }
    };
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(&output)
        .context("write filesystem response")?;
    stdout.flush().context("flush filesystem response")?;
    match error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uris_roundtrip_utf8_and_reserved_characters() {
        let path = Path::new("/tmp/中文 file?#%/a");
        let uri = file_uri(path).unwrap();
        assert_eq!(uri, "file:///tmp/%E4%B8%AD%E6%96%87%20file%3F%23%25/a");
        assert_eq!(file_path(&json!(uri)).unwrap(), path);
    }

    #[test]
    fn ambiguous_or_invalid_file_uris_are_rejected() {
        for value in [
            "https:///tmp/a",
            "file://localhost/tmp/a",
            "file:/tmp/a",
            "file://relative",
            "file:///tmp/a?x=1",
            "file:///tmp/a#fragment",
            "file:///tmp/%00",
            "file:///tmp/%FF",
            "file:///tmp/%",
            "file:///tmp/%0X",
        ] {
            let error = file_path(&json!(value)).unwrap_err();
            assert_eq!(
                error.downcast_ref::<io::Error>().unwrap().raw_os_error(),
                Some(libc::EINVAL),
                "{value}"
            );
        }
    }

    #[test]
    fn request_and_payload_limits_reject_before_filesystem_work() {
        assert!(operate("fs/readFile", &vec![b' '; MAX_FRAME + 1]).is_err());
        assert!(decode_data(&json!(STANDARD.encode(vec![0; MAX_FILE + 1]))).is_err());
        assert_eq!(
            decode_data(&json!(STANDARD.encode(vec![0; MAX_FILE])))
                .unwrap()
                .len(),
            MAX_FILE
        );
        assert!(decode_data(&json!("YWJj\n")).is_err());
        assert!(operate("fs/readFile", br#"{"path":5}"#).is_err());
    }
}
