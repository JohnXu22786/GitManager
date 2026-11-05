//! Fallible storage, not an end-to-end OOM guarantee or a second work budget.
//! Generic destructors/formatting callbacks need separate closure review.
use std::{fmt, mem, num::NonZeroUsize};
#[path = "product_alloc/ordered.rs"]
pub mod ordered;
#[path = "product_alloc/owned.rs"]
mod owned;
pub use owned::try_box;
#[path = "product_alloc/shared.rs"]
mod shared;
pub use shared::Shared;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AllocationSite { Vector, Bytes, String, Formatting, OrderedMap, OrderedSet, Box }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AllocationFailureKind { CapacityOverflow, ReservationFailed, RawAllocationFailed, Injected }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AllocationFailure {
    pub kind: AllocationFailureKind,
    pub site: AllocationSite,
    /// Minimum total bytes, not OS usage. None if length/bytes overflow usize.
    pub requested_bytes: Option<usize>,
}
impl fmt::Display for AllocationFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self.kind {
            AllocationFailureKind::CapacityOverflow => "capacity overflow",
            AllocationFailureKind::ReservationFailed => "reservation failed",
            AllocationFailureKind::RawAllocationFailed => "raw allocation failed",
            AllocationFailureKind::Injected => "injected",
        };
        let site = match self.site {
            AllocationSite::Vector => "vector", AllocationSite::Bytes => "bytes",
            AllocationSite::String => "string", AllocationSite::Formatting => "formatting",
            AllocationSite::OrderedMap => "ordered map", AllocationSite::OrderedSet => "ordered set",
            AllocationSite::Box => "box",
        };
        write!(f, "allocation failure ({kind}) at {site}: ")?;
        match self.requested_bytes {
            Some(bytes) => write!(f, "{bytes} bytes"),
            None => f.write_str("unrepresentable size"),
        }
    }
}
impl std::error::Error for AllocationFailure {}
#[derive(Debug)]
pub struct AllocationContext {
    until_failure: Option<NonZeroUsize>,
    sticky: bool,
    requests: usize,
    first_failure: Option<AllocationFailure>,
}
impl AllocationContext {
    pub const fn new() -> Self {
        Self { until_failure: None, sticky: false, requests: 0, first_failure: None }
    }
    /// Sticky refuses growth after ANY failure, even overflow; not no-growth.
    pub const fn fail_nth_growth(n: NonZeroUsize, sticky: bool) -> Self {
        Self { until_failure: Some(n), sticky, requests: 0, first_failure: None }
    }
    pub fn growth_requests(&self) -> usize { self.requests }
    pub fn first_failure(&self) -> Option<AllocationFailure> { self.first_failure }
    fn record(&mut self, kind: AllocationFailureKind, site: AllocationSite,
        requested_bytes: Option<usize>) -> AllocationFailure {
        let failure = AllocationFailure { kind, site, requested_bytes };
        if self.first_failure.is_none() { self.first_failure = Some(failure); }
        failure
    }
    fn growth(&mut self, site: AllocationSite, bytes: usize) -> Result<(), AllocationFailure> {
        self.requests = self.requests.saturating_add(1);
        let inject = self.until_failure.map(|n| n.get() == 1).unwrap_or(false);
        if let Some(n) = self.until_failure {
            self.until_failure = NonZeroUsize::new(n.get() - 1);
        }
        if inject || (self.sticky && self.first_failure.is_some()) {
            return Err(self.record(AllocationFailureKind::Injected, site, Some(bytes)));
        }
        Ok(())
    }
    pub(crate) fn extent(&mut self, len: usize, additional: usize, size: usize,
        site: AllocationSite) -> Result<(usize, usize), AllocationFailure> {
        let length = len.checked_add(additional);
        let bytes = length.and_then(|n| n.checked_mul(size));
        match (length, bytes) {
            (Some(n), Some(b)) if size == 0 || b <= isize::MAX as usize => Ok((n, b)),
            _ => Err(self.record(AllocationFailureKind::CapacityOverflow, site, bytes)),
        }
    }
}
pub fn try_reserve<T>(values: &mut Vec<T>, additional: usize,
    cx: &mut AllocationContext, site: AllocationSite) -> Result<(), AllocationFailure> {
    let (required, bytes) = cx.extent(values.len(), additional, mem::size_of::<T>(), site)?;
    if required <= values.capacity() { return Ok(()); }
    cx.growth(site, bytes)?;
    values.try_reserve_exact(additional).map_err(|_| {
        cx.record(AllocationFailureKind::ReservationFailed, site, Some(bytes))
    })
}
/// Refusal drops the incoming value once; arbitrary T::drop is not closed.
pub fn try_push<T>(values: &mut Vec<T>, value: T,
    cx: &mut AllocationContext, site: AllocationSite) -> Result<(), AllocationFailure> {
    try_reserve(values, 1, cx, site)?;
    values.push(value);
    Ok(())
}
pub fn try_extend_bytes(values: &mut Vec<u8>, bytes: &[u8],
    cx: &mut AllocationContext, site: AllocationSite) -> Result<(), AllocationFailure> {
    try_reserve(values, bytes.len(), cx, site)?;
    values.extend_from_slice(bytes);
    Ok(())
}
pub fn try_reserve_string(value: &mut String, additional: usize,
    cx: &mut AllocationContext, site: AllocationSite) -> Result<(), AllocationFailure> {
    let (required, bytes) = cx.extent(value.len(), additional, 1, site)?;
    if required <= value.capacity() { return Ok(()); }
    cx.growth(site, bytes)?;
    value.try_reserve_exact(additional).map_err(|_| {
        cx.record(AllocationFailureKind::ReservationFailed, site, Some(bytes))
    })
}
pub fn try_append_str(value: &mut String, suffix: &str,
    cx: &mut AllocationContext, site: AllocationSite) -> Result<(), AllocationFailure> {
    try_reserve_string(value, suffix.len(), cx, site)?;
    value.push_str(suffix);
    Ok(())
}
pub fn try_copy_str(value: &str, cx: &mut AllocationContext,
    site: AllocationSite) -> Result<String, AllocationFailure> {
    let mut result = String::new();
    try_append_str(&mut result, value, cx, site)?;
    Ok(result)
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FormatFailure { Allocation(AllocationFailure), Formatter }
/// Finish successfully to commit. Otherwise Drop restores content (not
/// capacity). First allocation failure survives fmt::Error and later writes.
/// Generic Display/Debug callbacks are NOT proved allocation-free.
pub struct FallibleStringWriter<'a> {
    value: &'a mut String,
    cx: &'a mut AllocationContext,
    site: AllocationSite,
    start: usize,
    failure: Option<AllocationFailure>,
    committed: bool,
}
impl<'a> FallibleStringWriter<'a> {
    pub fn new(value: &'a mut String, cx: &'a mut AllocationContext,
        site: AllocationSite) -> Self {
        let start = value.len();
        Self { value, cx, site, start, failure: None, committed: false }
    }
    pub fn failure(&self) -> Option<AllocationFailure> { self.failure }
    pub fn finish(mut self, result: fmt::Result) -> Result<(), FormatFailure> {
        if let Some(f) = self.failure { return Err(FormatFailure::Allocation(f)); }
        result.map_err(|_| FormatFailure::Formatter)?;
        self.committed = true;
        Ok(())
    }
}
impl fmt::Write for FallibleStringWriter<'_> {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        if self.failure.is_some() { return Err(fmt::Error); }
        try_append_str(self.value, value, self.cx, self.site).map_err(|failure| {
            self.failure = Some(failure);
            fmt::Error
        })
    }
}
impl Drop for FallibleStringWriter<'_> {
    fn drop(&mut self) {
        if !self.committed { self.value.truncate(self.start); }
    }
}
pub fn try_append_fmt(value: &mut String, arguments: fmt::Arguments<'_>,
    cx: &mut AllocationContext, site: AllocationSite) -> Result<(), FormatFailure> {
    let mut sink = FallibleStringWriter::new(value, cx, site);
    let result = fmt::write(&mut sink, arguments);
    sink.finish(result)
}
