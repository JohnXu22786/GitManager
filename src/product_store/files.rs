//! Pinned directory operations. Unix mutations are descriptor-relative; Windows
//! ancestor handles deny rename/delete. No generated language path reaches here.
use super::*;
use std::ffi::{OsStr, OsString};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
static TEMP_ID: AtomicU64 = AtomicU64::new(1);

#[cfg(test)]
pub(crate) mod file_sync_testing {
    use std::collections::BTreeMap;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, OnceLock};

    fn faults() -> &'static Mutex<BTreeMap<PathBuf, (Arc<AtomicUsize>, usize)>> {
        static FAULTS: OnceLock<Mutex<BTreeMap<PathBuf, (Arc<AtomicUsize>, usize)>>> =
            OnceLock::new();
        FAULTS.get_or_init(|| Mutex::new(BTreeMap::new()))
    }
    pub(crate) struct FileSyncFault {
        path: PathBuf,
        attempts: Arc<AtomicUsize>,
    }
    impl FileSyncFault {
        pub(crate) fn attempts(&self) -> usize {
            self.attempts.load(Ordering::Acquire)
        }
    }
    impl Drop for FileSyncFault {
        fn drop(&mut self) {
            faults()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&self.path);
        }
    }
    pub(crate) fn fail(path: PathBuf) -> FileSyncFault {
        fail_after(path, 0)
    }
    pub(crate) fn fail_after(path: PathBuf, successful: usize) -> FileSyncFault {
        let attempts = Arc::new(AtomicUsize::new(0));
        assert!(faults()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(path.clone(), (attempts.clone(), successful))
            .is_none());
        FileSyncFault { path, attempts }
    }
    pub(super) fn before_sync(path: &Path) -> io::Result<()> {
        if let Some((attempts, successful)) =
            faults().lock().unwrap_or_else(|e| e.into_inner()).get(path)
        {
            if attempts.fetch_add(1, Ordering::AcqRel) >= *successful {
                return Err(io::Error::other("injected durability barrier failure"));
            }
        }
        Ok(())
    }
    fn directory() -> (tempfile::TempDir, super::Directory, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(temp.path()).unwrap();
        let directory = super::Directory::open(&root).unwrap();
        (temp, directory, root)
    }
    #[test]
    fn synced_read_is_bounded_and_ordinary_read_never_syncs() {
        let (_temp, directory, root) = directory();
        let bytes = b"exact retained bytes";
        let path = root.join("backup");
        std::fs::write(&path, bytes).unwrap();
        let fault = fail(path.clone());
        assert!(directory.read_and_sync("backup", bytes.len() - 1).is_err());
        assert_eq!(fault.attempts(), 0);
        assert_eq!(directory.read("backup", bytes.len()).unwrap(), bytes);
        assert_eq!(fault.attempts(), 0);
        assert!(directory.read_and_sync("backup", bytes.len()).is_err());
        assert_eq!(fault.attempts(), 1);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        drop(fault);
        assert_eq!(
            directory.read_and_sync("backup", bytes.len()).unwrap(),
            bytes
        );
    }
    #[test]
    fn synced_read_rejects_missing_traversing_and_special_paths() {
        let (_temp, directory, root) = directory();
        assert!(directory.read_and_sync("missing", 32).is_err());
        assert!(!root.join("missing").exists());
        assert!(directory.read_and_sync("../outside", 32).is_err());
        std::fs::create_dir(root.join("not-a-file")).unwrap();
        assert!(directory.read_and_sync("not-a-file", 32).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn synced_read_rejects_symbolic_and_hard_links() {
        let (_temp, directory, root) = directory();
        let bytes = b"unchanged linked bytes";
        let source = root.join("source");
        std::fs::write(&source, bytes).unwrap();
        std::os::unix::fs::symlink(&source, root.join("symbolic")).unwrap();
        assert!(directory.read_and_sync("symbolic", 32).is_err());
        std::fs::hard_link(&source, root.join("hard")).unwrap();
        assert!(directory.read_and_sync("hard", 32).is_err());
        assert!(directory.read_and_sync("source", 32).is_err());
        assert_eq!(std::fs::read(source).unwrap(), bytes);
    }
}

pub(crate) struct WriteLock {
    file: File,
}
impl Drop for WriteLock {
    fn drop(&mut self) {
        // Unix duplicates share the lock, so closing only this descriptor can
        // leave it held past the transaction. Release it before File closes.
        // Drop cannot return errors; closing the file remains the fallback.
        let _ = self.file.unlock();
    }
}

pub(super) struct Directory {
    pub file: File,
    path: PathBuf,
    _guards: Vec<File>,
}
fn unsafe_path() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "unsafe or replaced project path",
    )
}
fn valid_name(name: &OsStr) -> io::Result<()> {
    let p = Path::new(name);
    if p.components().count() != 1 || !matches!(p.components().next(), Some(Component::Normal(_))) {
        Err(unsafe_path())
    } else {
        Ok(())
    }
}
impl Directory {
    pub fn open(path: &Path) -> io::Result<Self> {
        if !path.is_absolute()
            || path
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
        {
            return Err(unsafe_path());
        }
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            let mut file = OpenOptions::new().read(true).open("/")?;
            for component in path.components() {
                if let Component::Normal(name) = component {
                    file = unix::open(&file, name, unix::DIRECTORY, 0)?;
                }
            }
            Ok(Self {
                file,
                path: path.into(),
                _guards: vec![],
            })
        }
        #[cfg(windows)]
        {
            let mut current = PathBuf::new();
            let mut guards = Vec::new();
            for component in path.components() {
                current.push(component.as_os_str());
                if !matches!(component, Component::Prefix(_)) {
                    guards.push(windows::directory(&current)?);
                }
            }
            let file = guards.last().ok_or_else(unsafe_path)?.try_clone()?;
            Ok(Self {
                file,
                path: path.into(),
                _guards: guards,
            })
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        {
            let _ = path;
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "safe store unavailable on this platform",
            ))
        }
    }
    pub fn child(&self, name: &OsStr) -> io::Result<Self> {
        valid_name(name)?;
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            Ok(Self {
                file: unix::open(&self.file, name, unix::DIRECTORY, 0)?,
                path: self.path.join(name),
                _guards: vec![],
            })
        }
        #[cfg(windows)]
        {
            let mut guards = self
                ._guards
                .iter()
                .map(File::try_clone)
                .collect::<io::Result<Vec<_>>>()?;
            guards.push(self.file.try_clone()?);
            Ok(Self {
                file: windows::directory(&self.path.join(name))?,
                path: self.path.join(name),
                _guards: guards,
            })
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        {
            let _ = name;
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "safe store unavailable",
            ))
        }
    }
    pub fn create_child(&self, name: &OsStr) -> io::Result<Self> {
        valid_name(name)?;
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        unix::mkdir(&self.file, name)?;
        #[cfg(windows)]
        std::fs::create_dir(self.path.join(name))?;
        self.sync()?;
        self.child(name)
    }
    pub fn same(&self, other: &Self) -> io::Result<bool> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let a = self.file.metadata()?;
            let b = other.file.metadata()?;
            Ok(a.dev() == b.dev() && a.ino() == b.ino())
        }
        #[cfg(windows)]
        {
            windows::same(&self.file, &other.file)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = other;
            Ok(false)
        }
    }
    pub fn read(&self, name: &str, limit: usize) -> io::Result<Vec<u8>> {
        self.read_file(name, limit, false)
    }
    pub fn read_and_sync(&self, name: &str, limit: usize) -> io::Result<Vec<u8>> {
        self.read_file(name, limit, true)
    }
    fn read_file(&self, name: &str, limit: usize, sync: bool) -> io::Result<Vec<u8>> {
        // FlushFileBuffers requires write access on Windows. Use the same
        // checked opener and share flags, without creating or changing a file.
        let mut file = self.open_file(OsStr::new(name), sync && cfg!(windows), false)?;
        let meta = file.metadata()?;
        if !meta.is_file() || meta.len() > limit as u64 {
            return Err(unsafe_path());
        }
        let mut bytes = Vec::new();
        (&mut file).take(limit as u64 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > limit {
            return Err(unsafe_path());
        }
        if sync {
            #[cfg(test)]
            file_sync_testing::before_sync(&self.path.join(name))?;
            // Sync the descriptor that supplied these bounded bytes, never a
            // second path-based handle which could name a different object.
            file.sync_all()?;
        }
        Ok(bytes)
    }
    fn open_file(&self, name: &OsStr, write: bool, create: bool) -> io::Result<File> {
        valid_name(name)?;
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            let flags = if write { unix::RDWR } else { 0 } | if create { unix::CREATE } else { 0 };
            let file = unix::open(&self.file, name, flags, 0o600)?;
            use std::os::unix::fs::MetadataExt;
            let metadata = file.metadata()?;
            // An opened old CURRENT can have zero links after atomic
            // replacement. Its pinned bytes remain safe to finish reading.
            if !metadata.is_file() || metadata.nlink() > 1 {
                return Err(unsafe_path());
            }
            Ok(file)
        }
        #[cfg(windows)]
        {
            windows::file(&self.path.join(name), write, create)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        {
            let _ = (name, write, create);
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "safe store unavailable",
            ))
        }
    }
    pub fn lock(&self) -> io::Result<WriteLock> {
        let file = self.open_file(OsStr::new(".write.lock"), true, true)?;
        file.try_lock()
            .map_err(|e| io::Error::new(io::ErrorKind::WouldBlock, e))?;
        Ok(WriteLock { file })
    }
    pub fn publish(&self, name: &str, bytes: &[u8], replace: bool) -> io::Result<()> {
        self.publish_with_checkpoint(name, bytes, replace, || Ok(()))
    }
    pub(super) fn publish_with_checkpoint<E: From<io::Error>>(
        &self, name: &str, bytes: &[u8], replace: bool,
        checkpoint: impl FnOnce() -> std::result::Result<(), E>,
    ) -> std::result::Result<(), E> {
        valid_name(OsStr::new(name))?;
        // Existing objects/pointers must never be symlinks or special files.
        match self.open_file(OsStr::new(name), false, false) {
            Ok(_existing) =>
            {
                #[cfg(windows)]
                if replace && _existing.metadata()?.permissions().readonly() {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "the existing project file is read-only",
                    ).into());
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            let temp = OsString::from(format!(
                ".pending-{}-{}",
                std::process::id(),
                TEMP_ID.fetch_add(1, Ordering::Relaxed)
            ));
            let mut file = unix::open(
                &self.file,
                &temp,
                unix::RDWR | unix::CREATE | unix::EXCLUSIVE,
                0o600,
            )?;
            let result = (|| {
                file.write_all(bytes)?;
                file.sync_all()?;
                checkpoint()?;
                if replace {
                    unix::rename(&self.file, &temp, OsStr::new(name))?;
                } else {
                    unix::rename_new(&self.file, &temp, OsStr::new(name))?;
                }
                self.sync().map_err(E::from)
            })();
            let _ = unix::unlink(&self.file, &temp);
            result
        }
        #[cfg(windows)]
        {
            windows::publish_with_checkpoint(&self.path, name, bytes, replace, checkpoint)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        {
            let _ = (bytes, replace, checkpoint);
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "safe store unavailable",
            ).into())
        }
    }
    pub fn sync(&self) -> io::Result<()> {
        #[cfg(unix)]
        {
            #[cfg(test)]
            file_sync_testing::before_sync(&self.path)?;
            self.file.sync_all()
        }
        #[cfg(not(unix))]
        {
            Ok(())
        }
    }
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix {
    use super::*;
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::raw::{c_char, c_int, c_uint};
    use std::os::unix::ffi::OsStrExt;
    pub const RDWR: c_int = 2;
    #[cfg(target_os = "linux")]
    pub const DIRECTORY: c_int = 0o200000;
    #[cfg(target_os = "macos")]
    pub const DIRECTORY: c_int = 0x100000;
    #[cfg(target_os = "linux")]
    pub const CREATE: c_int = 0o100;
    #[cfg(target_os = "macos")]
    pub const CREATE: c_int = 0x200;
    #[cfg(target_os = "linux")]
    pub const EXCLUSIVE: c_int = 0o200;
    #[cfg(target_os = "macos")]
    pub const EXCLUSIVE: c_int = 0x800;
    #[cfg(target_os = "linux")]
    const SAFE: c_int = 0o400000 | 0o2000000 | 0o4000;
    #[cfg(target_os = "macos")]
    const SAFE: c_int = 0x100 | 0x1000000 | 0x4;
    extern "C" {
        // The mode is variadic: Apple ARM64 passes it on the stack, unlike
        // fixed arguments. Keep the libc ABI and its C integer promotion.
        fn openat(fd: c_int, path: *const c_char, flags: c_int, ...) -> c_int;
        fn mkdirat(fd: c_int, path: *const c_char, mode: c_uint) -> c_int;
        fn renameat(old: c_int, a: *const c_char, new: c_int, b: *const c_char) -> c_int;
        fn unlinkat(fd: c_int, path: *const c_char, flags: c_int) -> c_int;
    }
    fn name(value: &OsStr) -> io::Result<CString> {
        valid_name(value)?;
        CString::new(value.as_bytes()).map_err(|_| unsafe_path())
    }
    fn status(value: c_int) -> io::Result<()> {
        if value < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
    pub fn open(parent: &File, path: &OsStr, flags: c_int, mode: c_uint) -> io::Result<File> {
        let path = name(path)?;
        let fd = unsafe {
            openat(
                parent.as_raw_fd(),
                path.as_ptr(),
                flags | SAFE,
                mode as c_int,
            )
        };
        status(fd)?;
        Ok(unsafe { File::from_raw_fd(fd) })
    }
    pub fn mkdir(parent: &File, path: &OsStr) -> io::Result<()> {
        let path = name(path)?;
        status(unsafe { mkdirat(parent.as_raw_fd(), path.as_ptr(), 0o700) })
    }
    pub fn rename(parent: &File, a: &OsStr, b: &OsStr) -> io::Result<()> {
        let a = name(a)?;
        let b = name(b)?;
        status(unsafe {
            renameat(
                parent.as_raw_fd(),
                a.as_ptr(),
                parent.as_raw_fd(),
                b.as_ptr(),
            )
        })
    }
    // Publish without replacing an existing immutable name and without a
    // two-hard-link crash window. Unsupported filesystems fail explicitly.
    pub fn rename_new(parent: &File, a: &OsStr, b: &OsStr) -> io::Result<()> {
        let a = name(a)?;
        let b = name(b)?;
        #[cfg(target_os = "linux")]
        {
            extern "C" {
                fn renameat2(
                    old: c_int,
                    a: *const c_char,
                    new: c_int,
                    b: *const c_char,
                    flags: c_uint,
                ) -> c_int;
            }
            const RENAME_NOREPLACE: c_uint = 1;
            status(unsafe {
                renameat2(
                    parent.as_raw_fd(),
                    a.as_ptr(),
                    parent.as_raw_fd(),
                    b.as_ptr(),
                    RENAME_NOREPLACE,
                )
            })
        }
        #[cfg(target_os = "macos")]
        {
            extern "C" {
                fn renameatx_np(
                    old: c_int,
                    a: *const c_char,
                    new: c_int,
                    b: *const c_char,
                    flags: c_uint,
                ) -> c_int;
            }
            const RENAME_EXCL: c_uint = 4;
            status(unsafe {
                renameatx_np(
                    parent.as_raw_fd(),
                    a.as_ptr(),
                    parent.as_raw_fd(),
                    b.as_ptr(),
                    RENAME_EXCL,
                )
            })
        }
    }
    pub fn unlink(parent: &File, path: &OsStr) -> io::Result<()> {
        let path = name(path)?;
        status(unsafe { unlinkat(parent.as_raw_fd(), path.as_ptr(), 0) })
    }
}
#[cfg(windows)]
mod windows {
    use super::*;
    use std::os::windows::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    };
    use windows_sys::Win32::Storage::FileSystem::*;
    fn checked(file: File, directory: bool) -> io::Result<File> {
        let meta = file.metadata()?;
        if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || meta.is_dir() != directory
            || (!directory && !meta.is_file())
        {
            Err(unsafe_path())
        } else {
            if !directory {
                let mut information = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
                if unsafe {
                    GetFileInformationByHandle(file.as_raw_handle() as _, information.as_mut_ptr())
                } == 0
                {
                    return Err(io::Error::last_os_error());
                }
                if unsafe { information.assume_init() }.nNumberOfLinks > 1 {
                    return Err(unsafe_path());
                }
            }
            Ok(file)
        }
    }
    pub fn directory(path: &Path) -> io::Result<File> {
        checked(
            OpenOptions::new()
                .read(true)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(path)?,
            true,
        )
    }
    pub fn file(path: &Path, write: bool, create: bool) -> io::Result<File> {
        checked(
            OpenOptions::new()
                .read(true)
                .write(write)
                .create(create)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
                .open(path)?,
            false,
        )
    }
    pub fn same(a: &File, b: &File) -> io::Result<bool> {
        fn identity(file: &File) -> io::Result<(u32, u64)> {
            let mut value = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
            if unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, value.as_mut_ptr()) }
                == 0
            {
                return Err(io::Error::last_os_error());
            }
            let value = unsafe { value.assume_init() };
            Ok((
                value.dwVolumeSerialNumber,
                (u64::from(value.nFileIndexHigh) << 32) | u64::from(value.nFileIndexLow),
            ))
        }
        Ok(identity(a)? == identity(b)?)
    }
    pub fn publish_with_checkpoint<E: From<io::Error>>(
        path: &Path, name: &str, bytes: &[u8], replace: bool,
        checkpoint: impl FnOnce() -> std::result::Result<(), E>,
    ) -> std::result::Result<(), E> {
        let mut temporary = tempfile::NamedTempFile::new_in(path)?;
        temporary.write_all(bytes)?;
        temporary.as_file().sync_all()?;
        if replace {
            // Rust's Windows rename uses FileRenameInfoEx when necessary so
            // existing readers retain their old handle while new opens see
            // the new file. MoveFileExW alone can reject an open destination.
            // Keep the no-clobber path below separate for immutable objects.
            let target = path.join(name);
            checkpoint()?;
            fs::rename(temporary.path(), target)?;
            return temporary.as_file().sync_all().map_err(E::from);
        }
        let source: Vec<u16> = temporary
            .path()
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        let target: Vec<u16> = path
            .join(name)
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        checkpoint()?;
        if unsafe { MoveFileExW(source.as_ptr(), target.as_ptr(), MOVEFILE_WRITE_THROUGH) } == 0 {
            return Err(io::Error::last_os_error().into());
        }
        Ok(())
    }
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos", windows)))]
mod tests {
    use super::*;

    struct TestDirectory(PathBuf);
    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "gitmanager-store-lock-{}-{}-{}",
                module_path!().replace("::", "-"),
                std::process::id(),
                TEMP_ID.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(fs::canonicalize(path).unwrap())
        }
        fn open(&self) -> Directory {
            Directory::open(&self.0).unwrap()
        }
    }
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn assert_busy<T>(result: io::Result<T>) {
        let error = result
            .err()
            .expect("a live owner must exclude other writers");
        assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
        assert!(matches!(
            error
                .get_ref()
                .and_then(|inner| inner.downcast_ref::<fs::TryLockError>()),
            Some(fs::TryLockError::WouldBlock)
        ));
    }

    #[test]
    fn lock_excludes_other_owners_until_scope_exit() {
        let temp = TestDirectory::new();
        let directory = temp.open();
        let other = temp.open();
        {
            let _owner = directory.lock().unwrap();
            assert_busy(other.lock());
            assert_busy(other.lock());
        }
        let owner = other.lock().unwrap();
        assert_busy(directory.lock());
        drop(owner);
        directory.lock().unwrap();
    }

    #[test]
    fn lock_releases_after_failed_scope() {
        let temp = TestDirectory::new();
        let directory = temp.open();
        let other = temp.open();
        let result = (|| -> io::Result<()> {
            let _owner = directory.lock()?;
            directory.publish("../invalid", b"not committed", false)?;
            Ok(())
        })();
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidInput);
        {
            let _owner = other.lock().unwrap();
            assert_busy(directory.lock());
        }
        let result = std::panic::catch_unwind(|| {
            let _owner = directory.lock().unwrap();
            panic!("transaction interrupted");
        });
        assert!(result.is_err());
        other.lock().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn owned_lock_releases_while_duplicate_survives() {
        let temp = TestDirectory::new();
        let directory = temp.open();
        let other = temp.open();
        let owner = directory.lock().unwrap();
        let duplicate = owner.file.try_clone().unwrap();
        assert_busy(other.lock());
        drop(owner);
        let next_owner = other
            .lock()
            .expect("the transaction owner has released its lock");
        assert_busy(directory.lock());
        drop(duplicate);
        assert_busy(directory.lock());
        drop(next_owner);
        directory.lock().unwrap();
    }
}
