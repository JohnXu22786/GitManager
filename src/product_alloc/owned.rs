//! Narrow stable ownership bridge; no private Box layout or allocator policy.
use super::{AllocationContext, AllocationFailure, AllocationFailureKind, AllocationSite};
use std::alloc::{alloc, Layout};
use std::{mem, ptr};

/// Moves `value` into one Box, or drops it exactly once on refusal.
///
/// Only nonzero storage requests consume the context's injection ordinal.
/// This closes allocation and ownership, NOT arbitrary `T::drop`: callers
/// must separately review destructors in their covered product graph.
/// Allocator aborts or OS overcommit faults are not converted to errors;
/// only a returned null is a `RawAllocationFailed`.
pub fn try_box<T>(value: T, cx: &mut AllocationContext)
    -> Result<Box<T>, AllocationFailure> {
    // The checked constructor establishes the alloc Layout requirements.
    // Sized T has this exact layout; no array multiplication or private ABI.
    let layout = Layout::from_size_align(mem::size_of::<T>(), mem::align_of::<T>())
        .map_err(|_| cx.record(AllocationFailureKind::CapacityOverflow,
            AllocationSite::Box, Some(mem::size_of::<T>())))?;
    let raw = if layout.size() == 0 {
        // Documented Box ZST route: aligned/non-null, no global allocation,
        // no injected refusal and no request count (even after sticky failure).
        ptr::NonNull::<T>::dangling().as_ptr()
    } else {
        let (raw, refusal) = match cx.growth(AllocationSite::Box, layout.size()) {
            Ok(()) => {
                // SAFETY: validated layout and strictly positive size. This
                // is the global allocation family Box uses, not System or a
                // direct call on a GlobalAlloc implementation.
                (unsafe { alloc(layout) }.cast::<T>(), None)
            }
            Err(failure) => (ptr::null_mut(), Some(failure)),
        };
        // Injected null and real alloc-null share the same pre-init return.
        // `value` is still solely owned here and its ordinary drop cleans up.
        // Injected is never mislabeled as an observed raw allocator failure.
        if raw.is_null() {
            return Err(refusal.unwrap_or_else(|| cx.record(
                AllocationFailureKind::RawAllocationFailed,
                AllocationSite::Box, Some(layout.size()))));
        }
        raw
    };
    // SAFETY: raw is either a unique, aligned allocation for exactly T from
    // alloc, or the documented aligned/non-null ZST pointer. Preserve its
    // provenance; write moves value without reading/dropping uninitialized
    // storage. There is no fallible step/callback between initialization and
    // adoption. from_raw creates the sole owner, whose drop uses the matching
    // global deallocation/layout (and does not deallocate for ZST).
    // Contracts: https://doc.rust-lang.org/std/boxed/index.html#memory-layout
    // and https://doc.rust-lang.org/std/alloc/fn.alloc.html
    unsafe {
        raw.write(value);
        Ok(Box::from_raw(raw))
    }
}
