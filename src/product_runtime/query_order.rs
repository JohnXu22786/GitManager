//! Unrouted, closed query ordering over the existing contract's owner types.
//!
//! Both index reservations precede initialization and all input mutation.
//! Stable-sort equivalence requires a total preorder from the caller; the old
//! `_ => Equal` comparator is intentionally retained even on non-total inputs.
//! This module does not validate product inputs, meter work, or close caller
//! construction/destruction, routing, paid execution, or default-budget fit.
use crate::product_alloc::{try_reserve, AllocationContext, AllocationFailure, AllocationSite};
use crate::product_contract::{DataValue, Id, Record, SortKey};
use std::cmp::Ordering;

pub(crate) type QueryRow = (Id, Vec<DataValue>, DataValue);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QueryOrderError {
    Allocation(AllocationFailure),
    MissingKeySlots { row: usize, required: usize, available: usize },
}

// Fixed counters are test-only observations, never a product price or callback.
macro_rules! count {
    ($field:ident, $n:expr) => {
        #[cfg(test)]
        COUNTS.with(|slot| { let mut c = slot.get(); c.$field += $n; slot.set(c); });
    };
}

/// n<2 deliberately does not inspect the context or reserve any workspace.
pub(crate) fn try_sort_candidate_ids(
    candidates: &mut [&Record], cx: &mut AllocationContext,
) -> Result<(), AllocationFailure> {
    if candidates.len() < 2 { return Ok(()); }
    let (order, mut inverse) = sorted_indices(Input::Candidates(candidates), cx)?;
    permute(candidates, &order, &mut inverse);
    Ok(())
}

/// Short key vectors are an internal shape defect, not a new product input rule.
/// Extra slots are ignored. With zero keys, n>=2 still sorts ascending IDs and
/// prepares both workspaces, including when IDs are already sorted or all equal.
pub(crate) fn try_sort_rows(
    rows: &mut [QueryRow], sort: &[SortKey], cx: &mut AllocationContext,
) -> Result<(), QueryOrderError> {
    if rows.len() < 2 { return Ok(()); }
    for (row, value) in rows.iter().enumerate() {
        if value.1.len() < sort.len() {
            return Err(QueryOrderError::MissingKeySlots {
                row, required: sort.len(), available: value.1.len(),
            });
        }
    }
    let (order, mut inverse) = sorted_indices(Input::Rows(rows, sort), cx)
        .map_err(QueryOrderError::Allocation)?;
    permute(rows, &order, &mut inverse);
    Ok(())
}

// Closed borrowed inputs: no comparator/key/destructor callback can escape the
// finite comparator below. Query Expr, Record.values and List owners are unread.
enum Input<'a> {
    Candidates(&'a [&'a Record]),
    Rows(&'a [QueryRow], &'a [SortKey]),
}
impl Input<'_> {
    fn len(&self) -> usize {
        match self { Self::Candidates(v) => v.len(), Self::Rows(v, _) => v.len() }
    }
    fn compare(&self, a: usize, b: usize) -> Ordering {
        match self {
            Self::Candidates(v) => {
                count!(comparisons, 1);
                bytes(&v[a].id, &v[b].id)
            }
            Self::Rows(v, sort) => compare_rows(&v[a], &v[b], sort),
        }
    }
}

fn bytes(a: &str, b: &str) -> Ordering {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    for i in 0..a.len().min(b.len()) {
        count!(byte_pairs, 1);
        let order = a[i].cmp(&b[i]);
        if order != Ordering::Equal { return order; }
    }
    a.len().cmp(&b.len())
}
fn compare_values(a: &DataValue, b: &DataValue) -> Ordering {
    match (a, b) {
        (DataValue::Boolean { value: a }, DataValue::Boolean { value: b }) => a.cmp(b),
        (DataValue::Integer { value: a }, DataValue::Integer { value: b }) => a.cmp(b),
        (DataValue::Text { value: a }, DataValue::Text { value: b }) => bytes(a, b),
        (DataValue::Date { days: a }, DataValue::Date { days: b }) => a.cmp(b),
        (DataValue::Reference { entity: a, record: x },
         DataValue::Reference { entity: b, record: y }) => {
            let entity = bytes(a, b);
            if entity == Ordering::Equal { bytes(x, y) } else { entity }
        }
        _ => Ordering::Equal,
    }
}
fn compare_rows(a: &QueryRow, b: &QueryRow, sort: &[SortKey]) -> Ordering {
    count!(comparisons, 1);
    for (i, key) in sort.iter().enumerate() {
        count!(keys, 1);
        let order = compare_values(&a.1[i], &b.1[i]);
        if order != Ordering::Equal {
            return if key.descending { order.reverse() } else { order };
        }
    }
    bytes(&a.0, &b.0)
}

fn prepare(n: usize, cx: &mut AllocationContext)
    -> Result<(Vec<usize>, Vec<usize>), AllocationFailure>
{
    let mut order = Vec::new();
    let mut scratch = Vec::new();
    try_reserve(&mut order, n, cx, AllocationSite::Vector)?;
    try_reserve(&mut scratch, n, cx, AllocationSite::Vector)?;
    // On either refusal both lengths are still zero. Only acquired buffers
    // are freed; no input owner was moved, traversed, cloned or destroyed.
    for i in 0..n {
        order.push(i); scratch.push(0);
        count!(init_stores, 2);
    }
    Ok((order, scratch))
}

fn sorted_indices(input: Input<'_>, cx: &mut AllocationContext)
    -> Result<(Vec<usize>, Vec<usize>), AllocationFailure>
{
    let n = input.len();
    let (mut order, mut scratch) = prepare(n, cx)?;
    let mut width = 1;
    while width < n {
        let mut start = 0;
        while start < n {
            // Every addition is <=n, including an unpaired tail. No unchecked
            // doubled width or endpoint arithmetic and no new input-size cap.
            let mid = start + width.min(n - start);
            let end = mid + width.min(n - mid);
            let (mut left, mut right, mut out) = (start, mid, start);
            while left < mid && right < end {
                // Equal picks left, retaining original order for equal items.
                if input.compare(order[left], order[right]) != Ordering::Greater {
                    scratch[out] = order[left]; left += 1;
                } else {
                    scratch[out] = order[right]; right += 1;
                }
                out += 1; count!(merge_stores, 1);
            }
            while left < mid {
                scratch[out] = order[left]; left += 1; out += 1;
                count!(merge_stores, 1);
            }
            while right < end {
                scratch[out] = order[right]; right += 1; out += 1;
                count!(merge_stores, 1);
            }
            start = end;
        }
        std::mem::swap(&mut order, &mut scratch);
        count!(passes, 1);
        width += width.min(n - width);
    }
    // order[destination] = original source. Comparisons only borrowed original
    // inputs; the borrow ends before either caller starts applying permutation.
    Ok((order, scratch))
}

// Private and instantiated only for &Record and QueryRow; no T operation or
// callback, owner extraction, uninitialized scratch, cloning or nested Drop.
fn permute<T>(values: &mut [T], order: &[usize], inverse: &mut [usize]) {
    for (destination, &source) in order.iter().enumerate() {
        inverse[source] = destination;
        count!(inverse_stores, 1);
    }
    for i in 0..values.len() {
        while inverse[i] != i {
            let destination = inverse[i];
            values.swap(i, destination);
            inverse.swap(i, destination);
            count!(swaps, 1);
            // The moved item is fixed at destination. Permutation labels are
            // bijective, so at most n-1 swaps occur, even for non-total keys.
        }
    }
}

// For n>=2 and h=ceil(log2(n)): exactly2n initialized index slots, n*h merge
// stores, n inverse stores, at most n*h comparisons and n-1 whole-slot swaps.
// Shape checks scan n lengths. Each row comparison visits <=k keys and one ID;
// string byte pairs <=D+sum(max(TextLen,EntityLen+RecordLen)) for that comparison.
// These are mathematical envelopes, not unchecked runtime products/fuel prices.
// Whole-slot movement is <=3*(n-1)*size_of::<slot>(); nested buffers are untouched.
// Two workspace deallocations on success; refusal frees only acquired buffers.

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TestCounts {
    pub comparisons: u128, pub byte_pairs: u128, pub keys: u128,
    pub init_stores: u128, pub merge_stores: u128, pub inverse_stores: u128,
    pub swaps: u128, pub passes: u128,
}
#[cfg(test)]
thread_local! {
    static COUNTS: std::cell::Cell<TestCounts> = const { std::cell::Cell::new(TestCounts {
        comparisons:0,byte_pairs:0,keys:0,init_stores:0,merge_stores:0,inverse_stores:0,swaps:0,passes:0,
    }) };
}
#[cfg(test)]
pub(crate) fn test_reset() { COUNTS.with(|c| c.set(TestCounts::default())); }
#[cfg(test)]
pub(crate) fn test_counts() -> TestCounts { COUNTS.with(std::cell::Cell::get) }
#[cfg(test)]
pub(crate) fn test_compare_rows(a: &QueryRow, b: &QueryRow, sort: &[SortKey]) -> Ordering {
    compare_rows(a, b, sort)
}
#[cfg(test)]
pub(crate) fn test_prepare(n: usize, cx: &mut AllocationContext) -> Result<(), AllocationFailure> {
    prepare(n, cx).map(|_| ())
}
