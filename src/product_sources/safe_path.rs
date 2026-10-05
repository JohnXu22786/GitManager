//! Bounded regular-file intake. Directory handles prevent a symlink/rename
//! from redirecting a read outside its anchored path while it is in progress.
use super::{invalid, validate_relative_path};
use crate::product_contract::AdapterError;
use std::{
    fs::{self, File},
    io::Read,
    path::{Component, Path},
};

pub(super) fn relative(path: &str) -> Result<(), AdapterError> {
    validate_relative_path(path)?;
    if path.split('/').any(|p| p.eq_ignore_ascii_case(".git")) {
        return Err(invalid(
            "Git administrative paths are not program artifacts",
        ));
    }
    Ok(())
}
fn failure(e: std::io::Error) -> AdapterError {
    AdapterError::Failed(e.to_string())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn open(path: &Path, directory: bool) -> Result<File, AdapterError> {
    use std::{
        ffi::CString,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::ffi::OsStrExt,
        },
    };
    unsafe extern "C" {
        fn openat(
            fd: std::ffi::c_int,
            path: *const std::ffi::c_char,
            flags: std::ffi::c_int,
            ...
        ) -> std::ffi::c_int;
    }
    #[cfg(target_os = "linux")]
    const FLAGS: (i32, i32) = (0x20000 | 0x800 | 0x80000, 0x10000);
    #[cfg(target_os = "macos")]
    const FLAGS: (i32, i32) = (0x100 | 0x4 | 0x1000000, 0x100000);
    if !path.is_absolute() {
        return Err(invalid("Source path must be absolute"));
    }
    let mut current = File::open("/").map_err(failure)?;
    let parts: Vec<_> = path
        .components()
        .filter(|c| !matches!(c, Component::RootDir))
        .collect();
    for (i, part) in parts.iter().enumerate() {
        let Component::Normal(name) = part else {
            return Err(invalid("Unsafe path component"));
        };
        let name = CString::new(name.as_bytes()).map_err(|_| invalid("Invalid path bytes"))?;
        let is_dir = directory || i + 1 < parts.len();
        // SAFETY: fd is a live owned descriptor; name is a NUL-terminated C
        // string; no creation flag is used. A successful new fd is owned once.
        let fd = unsafe {
            openat(
                current.as_raw_fd(),
                name.as_ptr(),
                FLAGS.0 | if is_dir { FLAGS.1 } else { 0 },
            )
        };
        if fd < 0 {
            return Err(failure(std::io::Error::last_os_error()));
        }
        current = unsafe { File::from_raw_fd(fd) };
    }
    Ok(current)
}

#[cfg(windows)]
fn open(path: &Path, directory: bool) -> Result<File, AdapterError> {
    use std::{
        fs::OpenOptions,
        os::windows::fs::{MetadataExt, OpenOptionsExt},
    };
    if !path.is_absolute() {
        return Err(invalid("Source path must be absolute"));
    }
    let mut parents = Vec::new();
    let mut cursor = std::path::PathBuf::new();
    let parts: Vec<_> = path.components().collect();
    for (i, part) in parts.iter().enumerate() {
        if matches!(part, Component::ParentDir | Component::CurDir) {
            return Err(invalid("Unsafe path component"));
        }
        cursor.push(part);
        if matches!(part, Component::Prefix(_)) {
            continue;
        }
        let is_dir = directory || i + 1 < parts.len();
        // Excluding FILE_SHARE_DELETE holds each parent against rename while
        // descendants are opened. OPEN_REPARSE_POINT refuses final indirection.
        let file = OpenOptions::new()
            .read(true)
            .share_mode(0x1 | 0x2)
            .custom_flags(0x00200000 | if is_dir { 0x02000000 } else { 0 })
            .open(&cursor)
            .map_err(failure)?;
        let metadata = file.metadata().map_err(failure)?;
        if metadata.file_attributes() & 0x400 != 0 || is_dir && !metadata.is_dir() {
            return Err(invalid("Reparse points are not source paths"));
        }
        if i + 1 == parts.len() {
            return Ok(file);
        }
        parents.push(file);
    }
    Err(invalid("Missing source path"))
}
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn open(_path: &Path, _directory: bool) -> Result<File, AdapterError> {
    Err(AdapterError::Unsupported(
        "Safe source file intake is unavailable on this platform".into(),
    ))
}

pub(super) fn directory(path: &Path) -> Result<(), AdapterError> {
    let file = open(path, true)?;
    if !file.metadata().map_err(failure)?.is_dir() {
        return Err(invalid("Expected a directory"));
    }
    Ok(())
}
pub(super) fn read(root: &Path, path: &str, limit: usize) -> Result<Vec<u8>, AdapterError> {
    relative(path)?;
    let file = open(&root.join(path), false)?;
    let metadata = file.metadata().map_err(failure)?;
    if !metadata.is_file() || metadata.len() > limit as u64 {
        return Err(invalid("Source must be a bounded regular file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            return Err(invalid("Hard-linked source files are unsupported"));
        }
    }
    #[cfg(windows)]
    {
        use std::{mem::MaybeUninit, os::windows::io::AsRawHandle};
        use windows_sys::Win32::Storage::FileSystem::{
            GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
        };
        let mut info = MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
        // SAFETY: file owns a live Windows handle; the API writes the full
        // structure on success. Do not inspect uninitialized data on failure.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), info.as_mut_ptr()) } == 0 {
            return Err(failure(std::io::Error::last_os_error()));
        }
        if unsafe { info.assume_init() }.nNumberOfLinks != 1 {
            return Err(invalid("Hard-linked source files are unsupported"));
        }
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(failure)?;
    if bytes.len() > limit {
        return Err(invalid("Source exceeds byte limit"));
    }
    Ok(bytes)
}
/// New private job bundles only; existing project and registry files are never
/// rewritten. A same-ID directory is never reused or overwritten.
pub(super) fn create_job(root: &Path, id: &str) -> Result<std::path::PathBuf, AdapterError> {
    directory(root)?;
    let path = root.join(id);
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&path).map_err(failure)?;
    directory(&path)?;
    Ok(path)
}
