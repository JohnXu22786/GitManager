//! Strong-only ownership. Allocation refusal is inline; payload destruction
//! must separately belong to a closed, nonallocating managed-owner family.
use super::{try_box, AllocationContext, AllocationFailure};
use std::{marker::PhantomData, ops::Deref, ptr::NonNull,
    sync::atomic::{fence, AtomicUsize, Ordering}};

// The trailing value supports built-in composite-tail unsizing. Its metadata
// always travels with the pointer; no private std layout or vtable is assumed.
struct Inner<T: ?Sized> { strong: AtomicUsize, value: T }

/// Immutable strong owner with no Clone, Weak, mutable or raw-pointer API.
///
/// Constructors mint one owner or an exact pair, never an unbounded family.
/// An owning payload must keep its teardown workspace through the last drop.
pub struct Shared<T: ?Sized> {
    ptr: NonNull<Inner<T>>,
    marker: PhantomData<Inner<T>>,
}

impl<T> Shared<T> {
    pub fn try_new(value: T, cx: &mut AllocationContext) -> Result<Self, AllocationFailure> {
        let inner = try_box(Inner { strong: AtomicUsize::new(1), value }, cx)?;
        // SAFETY: Box supplies the exact initialized, aligned allocation.
        // Transfer its single credit without a fallible step or allocation.
        Ok(Self { ptr: unsafe { NonNull::new_unchecked(Box::into_raw(inner)) }, marker: PhantomData })
    }

    pub fn try_pair(value: T, cx: &mut AllocationContext)
        -> Result<(Self, Self), AllocationFailure> {
        let inner = try_box(Inner { strong: AtomicUsize::new(2), value }, cx)?;
        // SAFETY: the initialized header owns exactly two credits. Creating
        // both handles below cannot panic, allocate, or expose a partial pair.
        let ptr = unsafe { NonNull::new_unchecked(Box::into_raw(inner)) };
        Ok((Self { ptr, marker: PhantomData }, Self { ptr, marker: PhantomData }))
    }
}

impl<T: ?Sized> Shared<T> {
    pub fn ptr_eq<U: ?Sized>(a: &Self, b: &Shared<U>) -> bool {
        // Allocation identity ignores metadata after specialized unsizing.
        std::ptr::addr_eq(a.ptr.as_ptr(), b.ptr.as_ptr())
    }

    /// Mint one credit only within a separately audited whole-allocation bound.
    ///
    /// # Safety
    /// The caller must prove that every existing and in-flight credit, across
    /// all threads and all future retain calls, stays below usize::MAX. That
    /// proof includes temporaries, and excludes leaked/forgotten credits.
    /// This private operation is not an unrestricted safe Clone substitute.
    unsafe fn retain_bounded(&self) -> Self {
        // SAFETY: this borrow keeps one live credit throughout the increment;
        // the caller's global bound rules out wrapping. No retry/abort branch.
        unsafe { self.ptr.as_ref() }.strong.fetch_add(1, Ordering::Relaxed);
        Self { ptr: self.ptr, marker: PhantomData }
    }
}

impl<T: ?Sized> Deref for Shared<T> {
    type Target = T;
    fn deref(&self) -> &T {
        // SAFETY: the borrowed credit keeps initialized payload storage live;
        // only immutable access is exposed, including for unsized payloads.
        &unsafe { self.ptr.as_ref() }.value
    }
}

// SAFETY: readers may share T and the final release may destroy T on another
// thread. Both bounds are required; the atomic count protects ownership only.
unsafe impl<T: ?Sized + Send + Sync> Send for Shared<T> {}
unsafe impl<T: ?Sized + Send + Sync> Sync for Shared<T> {}

impl<T: ?Sized> Drop for Shared<T> {
    fn drop(&mut self) {
        // SAFETY: each handle owns exactly one credit. Only the unique 1->0
        // transition may access the allocation for destruction and deallocation.
        if unsafe { self.ptr.as_ref() }.strong.fetch_sub(1, Ordering::Release) == 1 {
            fence(Ordering::Acquire);
            // SAFETY: acquire observes prior releases; no reader remains. The
            // original allocation and its (possibly unsized) layout metadata
            // are preserved. Box drops the payload and releases storage once.
            unsafe { drop(Box::from_raw(self.ptr.as_ptr())); }
        }
    }
}

// Tests were written before the production implementation. Their private access
// exercises bounded retain and genuine stable composite-tail trait unsizing.
#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, mem::ManuallyDrop, sync::atomic::{AtomicUsize, Ordering}};

    #[test]
    fn bounded_retain_has_one_credit_and_survivor_in_each_drop_order() {
        struct Payload<'a>(&'a Cell<usize>);
        impl Drop for Payload<'_> { fn drop(&mut self) { self.0.set(self.0.get() + 1); } }
        for first_original in [false, true] {
            let drops = Cell::new(0);
            let a = Shared::try_new(Payload(&drops), &mut AllocationContext::new()).unwrap();
            // SAFETY: this allocation mints exactly one additional credit;
            // neither credit is leaked, forgotten, or retained again.
            let b = unsafe { a.retain_bounded() };
            assert!(Shared::ptr_eq(&a, &b));
            assert_eq!(unsafe { a.ptr.as_ref() }.strong.load(Ordering::Relaxed), 2);
            let last = if first_original { drop(a); b } else { drop(b); a };
            assert_eq!(drops.get(), 0);
            assert_eq!(unsafe { last.ptr.as_ref() }.strong.load(Ordering::Relaxed), 1);
            drop(last); assert_eq!(drops.get(), 1);
        }
    }

    #[test]
    fn two_threads_retain_with_fixed_whole_allocation_bound() {
        struct Payload<'a>(&'a AtomicUsize);
        impl Drop for Payload<'_> { fn drop(&mut self) { self.0.fetch_add(1, Ordering::Relaxed); } }
        let drops = AtomicUsize::new(0);
        let (a, b) = Shared::try_pair(Payload(&drops), &mut AllocationContext::new()).unwrap();
        std::thread::scope(|s| {
            let x = s.spawn(move || {
                // SAFETY: across BOTH threads, only these two retain calls
                // can mint. Pair2 + in-flight2 <=4, with no leaks/recursion.
                let c = unsafe { a.retain_bounded() }; drop(a); drop(c);
            });
            let y = s.spawn(move || {
                // SAFETY: same whole-allocation bound4, not a per-thread bound.
                let d = unsafe { b.retain_bounded() }; drop(d); drop(b);
            });
            x.join().unwrap(); y.join().unwrap();
        });
        assert_eq!(drops.load(Ordering::Relaxed), 1);
    }

    trait Witness: Send + Sync { fn value(&self) -> u64; }
    #[repr(align(256))]
    struct Concrete<'a> { drops: &'a AtomicUsize, value: u64 }
    impl Witness for Concrete<'_> { fn value(&self) -> u64 { self.value } }
    impl Drop for Concrete<'_> {
        fn drop(&mut self) { self.drops.fetch_add(1, Ordering::Relaxed); }
    }
    // Test-only, concrete trait specialization. This is NOT a general erased
    // constructor and is NOT production ReplayAdmission migration.
    fn into_witness<'a>(owner: Shared<Concrete<'a>>) -> Shared<dyn Witness + 'a> {
        let owner = ManuallyDrop::new(owner);
        // Built-in unsizing of the composite tail; no invented metadata,
        // transmute, unstable CoerceUnsized implementation, or allocation.
        let raw: *mut Inner<dyn Witness + 'a> = owner.ptr.as_ptr();
        // SAFETY: unsizing preserves the initialized allocation and its sole
        // transferred credit, including alignment, provenance and lifetime.
        Shared { ptr: unsafe { NonNull::new_unchecked(raw) }, marker: PhantomData }
    }

    #[test]
    fn specialized_dyn_transfer_preserves_identity_count_and_exact_drop() {
        #[cfg(shared_observer)]
        let _lock = crate::LOCK.lock().unwrap();
        for dyn_last in [false, true] {
            #[cfg(shared_observer)]
            let watch = crate::Watch::begin();
            let drops = AtomicUsize::new(0); let mut cx = AllocationContext::new();
            let (concrete, other) = Shared::try_pair(Concrete { drops: &drops, value: 91 }, &mut cx).unwrap();
            #[cfg(shared_observer)]
            watch.no_growth();
            let erased = into_witness(other);
            assert!(Shared::ptr_eq(&concrete, &erased));
            assert_eq!(erased.value(), 91); assert_eq!(cx.growth_requests(), 1);
            assert_eq!(unsafe { concrete.ptr.as_ref() }.strong.load(Ordering::Relaxed), 2);
            if dyn_last { drop(concrete); assert_eq!(drops.load(Ordering::Relaxed), 0);
                assert_eq!(erased.value(), 91); drop(erased);
            } else { drop(erased); assert_eq!(drops.load(Ordering::Relaxed), 0);
                assert_eq!(concrete.value, 91); drop(concrete); }
            assert_eq!(drops.load(Ordering::Relaxed), 1);
            #[cfg(shared_observer)]
            assert_eq!(watch.empty(), 1);
        }
    }
}
