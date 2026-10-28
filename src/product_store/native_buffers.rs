//! Pure Unix byte preparation, deliberately not registered by store callers yet.
//!
//! These owners confer no filesystem, no-follow, cleanup or publication authority.
//! In particular, the existing component predicate accepts raw `foo/` and `foo/.`;
//! preserving that predicate does not prove the original bytes contain no slash.
#![cfg(any(target_os = "linux", target_os = "macos"))]

use crate::product_alloc::{try_extend_bytes, try_reserve, AllocationContext,
    AllocationFailure, AllocationFailureKind, AllocationSite};
use std::{ffi::{CStr, OsStr, OsString}, os::unix::ffi::{OsStrExt, OsStringExt},
    path::{Component, Path, PathBuf}};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NativeBufferFailure {
    Allocation(AllocationFailure),
    UnsafeName,
}
impl From<AllocationFailure> for NativeBufferFailure {
    fn from(value: AllocationFailure) -> Self { Self::Allocation(value) }
}

fn overflow() -> NativeBufferFailure {
    NativeBufferFailure::Allocation(AllocationFailure {
        kind: AllocationFailureKind::CapacityOverflow,
        site: AllocationSite::Bytes,
        requested_bytes: None,
    })
}

// These pure arithmetic seams also accept synthetic usize lengths for tests.
// Real borrowed byte slices have extent <=isize::MAX: name+1 fits usize and
// parent+name+one separator <=usize::MAX. Their checked-add failure is therefore
// unreachable from the public slice-based preparation paths. All reachable
// capacity refusal (including a representable total >isize::MAX) goes through
// the actual context's try_reserve, retaining its first-failure/sticky policy.
pub(crate) fn name_capacity(bytes: usize) -> Result<usize, NativeBufferFailure> {
    bytes.checked_add(1).ok_or_else(overflow)
}
pub(crate) fn child_capacity(parent: usize, separator: bool, name: usize)
    -> Result<usize, NativeBufferFailure> {
    parent.checked_add(usize::from(separator)).and_then(|n| n.checked_add(name))
        .ok_or_else(overflow)
}

/// One private byte owner with exactly one final NUL and no interior NUL.
/// The original OsStr satisfies the existing normalized-components predicate;
/// it is NOT necessarily a single raw pathname component.
pub(crate) struct UnixName { bytes: Vec<u8> }
impl UnixName {
    pub(crate) fn as_c_str(&self) -> &CStr {
        // SAFETY: only try_unix_name constructs this private, immutable owner.
        // It copies bytes, rejects every existing NUL and appends exactly one
        // terminator. No mutable view or owning C conversion is exposed.
        unsafe { CStr::from_bytes_with_nul_unchecked(&self.bytes) }
    }
    pub(crate) fn as_os_str(&self) -> &OsStr {
        OsStr::from_bytes(&self.bytes[..self.bytes.len() - 1])
    }
}

/// Preserve the old predicate, raw bytes, and reserve/copy-before-NUL ordering.
/// Exactly one actual growth request on a predicate-accepted input.
pub(crate) fn try_unix_name(value: &OsStr, cx: &mut AllocationContext)
    -> Result<UnixName, NativeBufferFailure> {
    let path = Path::new(value);
    if path.components().count() != 1
        || !matches!(path.components().next(), Some(Component::Normal(_))) {
        return Err(NativeBufferFailure::UnsafeName);
    }
    let raw = value.as_bytes();
    let mut bytes = Vec::new();
    try_reserve(&mut bytes, name_capacity(raw.len())?, cx, AllocationSite::Bytes)?;
    // Reserved raw.len()+1 above; neither this copy nor the final push grows.
    bytes.extend_from_slice(raw);
    if bytes.contains(&0) { return Err(NativeBufferFailure::UnsafeName); }
    bytes.push(0);
    Ok(UnixName { bytes })
}

/// Raw path copy only: it neither validates nor opens a path. Unix ownership
/// transfers from Vec -> OsString -> PathBuf do not copy, shrink or allocate.
pub(crate) fn try_unix_path_copy(value: &Path, cx: &mut AllocationContext)
    -> Result<PathBuf, NativeBufferFailure> {
    let mut bytes = Vec::new();
    try_extend_bytes(&mut bytes, value.as_os_str().as_bytes(), cx, AllocationSite::Bytes)?;
    Ok(PathBuf::from(OsString::from_vec(bytes)))
}

/// Match Unix Path::join for these nonabsolute predicate-accepted names,
/// retaining repeated/trailing separators and non-UTF8 bytes on both sides.
/// This joins memory only; the name does not attest pathname safety.
pub(crate) fn try_unix_child_path(parent: &Path, name: &UnixName,
    cx: &mut AllocationContext) -> Result<PathBuf, NativeBufferFailure> {
    let parent = parent.as_os_str().as_bytes();
    let name = name.as_os_str().as_bytes();
    let separator = !parent.is_empty() && !parent.ends_with(b"/");
    let required = child_capacity(parent.len(), separator, name.len())?;
    let mut bytes = Vec::new();
    try_reserve(&mut bytes, required, cx, AllocationSite::Bytes)?;
    bytes.extend_from_slice(parent);
    if separator { bytes.push(b'/'); }
    bytes.extend_from_slice(name);
    Ok(PathBuf::from(OsString::from_vec(bytes)))
}

/// Prepared arguments own memory, never a File or an acquired temp pathname.
/// Dropping them may only release their two Vec owners, never unlink or close.
pub(crate) struct PreparedUnixNames { target: UnixName, temporary: UnixName }
impl PreparedUnixNames {
    pub(crate) fn target(&self) -> &UnixName { &self.target }
    pub(crate) fn temporary(&self) -> &UnixName { &self.temporary }
}

fn decimal(mut value: u64, output: &mut [u8; 40], position: &mut usize) {
    let mut digits = [0_u8; 20];
    let mut start = digits.len();
    loop {
        start -= 1;
        digits[start] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 { break; }
    }
    let end = *position + digits.len() - start;
    // Call sites below give at most 9+10+1+20=40 bytes in total.
    output[*position..end].copy_from_slice(&digits[start..]);
    *position = end;
}

/// Caller supplies the process ID and the counter captured once at its original
/// site. No process query, global counter, randomness or collision retry here.
/// At most two growth requests; second failure drops the first private owner.
pub(crate) fn try_prepared_unix_names(target: &OsStr, pid: u32, id: u64,
    cx: &mut AllocationContext) -> Result<PreparedUnixNames, NativeBufferFailure> {
    let target = try_unix_name(target, cx)?;
    // This bound follows scalar widths, not a newly imposed input/name limit.
    let mut text = [0_u8; 40];
    text[..9].copy_from_slice(b".pending-");
    let mut position = 9;
    decimal(u64::from(pid), &mut text, &mut position);
    text[position] = b'-';
    position += 1;
    decimal(id, &mut text, &mut position);
    let temporary = try_unix_name(OsStr::from_bytes(&text[..position]), cx)?;
    Ok(PreparedUnixNames { target, temporary })
}
