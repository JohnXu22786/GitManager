use super::{input, protocol::*};
use serde::{de::DeserializeOwned, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path},
};

pub(crate) fn safe_directory(path: &Path, create: bool) -> Result<(), String> {
    if !path.is_absolute() {
        return Err("job and credential directories must be absolute".into());
    }
    let mut cursor = std::path::PathBuf::new();
    for component in path.components() {
        if matches!(component, Component::ParentDir | Component::CurDir) {
            return Err("relative path components are forbidden".into());
        }
        cursor.push(component);
        if !cursor.exists() && create {
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder.create(&cursor).map_err(|e| e.to_string())?;
            if let Some(parent) = cursor.parent() {
                sync_directory(parent)?;
            }
        }
        let metadata = fs::symlink_metadata(&cursor).map_err(|e| e.to_string())?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err("directory symlinks are not allowed".into());
        }
    }
    Ok(())
}
pub(crate) fn file(path: &Path, write: bool, create_new: bool) -> Result<File, String> {
    safe_directory(path.parent().ok_or("missing parent")?, false)?;
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err("expected regular non-symlink job file".into());
        }
    }
    let mut options = OpenOptions::new();
    options.read(true).write(write).create_new(create_new);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        #[cfg(target_os = "linux")]
        options.custom_flags(0x20000 | 0x800); // O_NOFOLLOW | O_NONBLOCK
        #[cfg(target_os = "macos")]
        options.custom_flags(0x100 | 0x4);
        options.mode(0o600);
    }
    let f = options.open(path).map_err(|e| e.to_string())?;
    if !f.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("expected regular job file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if f.metadata().map_err(|e| e.to_string())?.nlink() != 1 {
            return Err("hard-linked job files are forbidden".into());
        }
    }
    Ok(f)
}
pub(crate) fn read(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let f = file(path, false, false)?;
    if f.metadata().map_err(|e| e.to_string())?.len() > limit as u64 {
        return Err("file exceeds byte limit".into());
    }
    let mut bytes = Vec::new();
    f.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > limit {
        return Err("file exceeds byte limit".into());
    }
    Ok(bytes)
}
/// Only the saved typed request uses this larger, derived local envelope cap.
/// Duplicate members, nesting, trailing input and the typed byte bounds remain
/// independently enforced before a subprocess or retained result can be used.
pub(crate) fn request(path: &Path) -> Result<ProviderRequest, String> {
    let bytes = read(path, MAX_SERIALIZED_REQUEST_BYTES)?;
    let value = input::parse_json_bytes_with_limit(&bytes, MAX_SERIALIZED_REQUEST_BYTES)
        .map_err(|e| e.to_string())?;
    let request: ProviderRequest = serde_json::from_value(value).map_err(|e| e.to_string())?;
    request.validate()?;
    Ok(request)
}

pub(crate) fn json<T: DeserializeOwned>(path: &Path) -> Result<T, String> {
    let bytes = read(path, MAX_RESULT_BYTES)?;
    let value = input::parse_json_bytes(&bytes).map_err(|e| e.to_string())?;
    serde_json::from_value(value).map_err(|e| e.to_string())
}
pub(crate) fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("missing parent")?;
    safe_directory(parent, false)?;
    if fs::symlink_metadata(path).is_ok() {
        let _ = file(path, false, false)?;
    }
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    temporary.write_all(bytes).map_err(|e| e.to_string())?;
    temporary.as_file().sync_all().map_err(|e| e.to_string())?;
    temporary.persist(path).map_err(|e| e.error.to_string())?;
    #[cfg(unix)]
    File::open(parent)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    Ok(())
}
pub(crate) fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    write(path, &serde_json::to_vec(value).map_err(|e| e.to_string())?)
}
pub(crate) fn sync_directory(directory: &Path) -> Result<(), String> {
    #[cfg(unix)]
    File::open(directory)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    Ok(())
}
pub(crate) struct JobLock(File);
impl Drop for JobLock {
    fn drop(&mut self) {
        // Closing alone can leave flock inherited by another thread's fork
        // until exec. Explicit unlock releases the shared open-file lock now.
        let _ = self.0.unlock();
    }
}
pub(crate) fn lock(directory: &Path) -> Result<JobLock, String> {
    let f = file(&directory.join("lock"), true, false)?;
    f.try_lock()
        .map_err(|_| "job is active in another worker".to_string())?;
    Ok(JobLock(f))
}
pub(crate) fn receipt(directory: &Path) -> Result<JobReceipt, String> {
    let receipt: JobReceipt = json(&directory.join("receipt.json"))?;
    if receipt.version != RECEIPT_VERSION
        || !valid_id(&receipt.request_id)
        || directory.file_name().and_then(|s| s.to_str()) != Some(receipt.request_id.as_str())
    {
        return Err("unsupported or mismatched job receipt".into());
    }
    if !valid_digest(&receipt.request_digest)
        || !valid_digest(&receipt.source_digest)
        || receipt.disclosure.nonce.len() != 32
        || !receipt
            .disclosure
            .nonce
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || receipt.disclosure.request_id != receipt.request_id
        || receipt.provider != receipt.disclosure.provider
        || receipt.request_digest != receipt.disclosure.request_digest
        || receipt.source_digest != receipt.disclosure.source_digest
        || receipt.capabilities.digest() != receipt.disclosure.capability_digest
        || receipt.provenance != receipt.capabilities.provenance
    {
        return Err("job receipt identity mismatch".into());
    }
    Ok(receipt)
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests {
    use super::*;
    use std::os::fd::AsRawFd;
    #[test]
    fn release_is_not_delayed_by_an_inherited_descriptor() {
        // Deterministically emulate the shared open-file description inherited
        // during another thread's fork, without forking a multi-threaded test.
        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        drop(file(&root.join("lock"), true, true).unwrap());
        let owner = lock(&root).unwrap();
        let inherited = owner.0.try_clone().unwrap();
        assert_ne!(owner.0.as_raw_fd(), inherited.as_raw_fd());
        assert!(lock(&root).is_err());
        drop(owner);
        let next = lock(&root).expect(
            "ownership release must explicitly unlock even while an inherited descriptor lives",
        );
        drop(inherited);
        drop(next);
    }
}
