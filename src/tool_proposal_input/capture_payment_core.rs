//! Checked arithmetic only. Callers prepay descriptor arithmetic separately.
use std::mem::{align_of, size_of};

pub(crate) const LEVEL_DESCRIPTOR: u64 = 8 + 4 * usize::BITS as u64;
pub(crate) const SEARCH_DESCRIPTOR: u64 = LEVEL_DESCRIPTOR + 12;
pub(crate) const INSERT_DESCRIPTOR: u64 = LEVEL_DESCRIPTOR + 96;
pub(crate) const CLONE_DESCRIPTOR: u64 = 64;
pub(crate) const BULK_DESCRIPTOR: u64 = LEVEL_DESCRIPTOR + 96;

pub(crate) fn string_copy_bytes(bytes: usize) -> Option<u64> {
    u64::try_from(bytes).ok()?.checked_mul(2)?.checked_add(4)
}
pub(crate) fn btree_levels(len: usize) -> Option<usize> {
    if len == 0 {
        return Some(1);
    }
    let mut quotient = len.checked_add(1)? / 2;
    let mut levels = 1usize;
    while quotient >= 6 {
        quotient /= 6;
        levels = levels.checked_add(1)?;
    }
    Some(levels)
}
fn node_bytes<K, V>() -> Option<u64> {
    let pair = size_of::<K>().checked_add(size_of::<V>())?;
    let alignment = align_of::<K>().max(align_of::<V>()).max(size_of::<usize>());
    let n = pair
        .checked_mul(11)?
        .checked_add(size_of::<usize>().checked_mul(13)?)?
        .checked_add(4)?
        .checked_add(alignment.checked_mul(7)?)?;
    u64::try_from(n).ok()
}
fn pair_bytes<K, V>() -> Option<u64> {
    u64::try_from(size_of::<K>().checked_add(size_of::<V>())?).ok()
}
pub(crate) fn btree_search(len: usize, comparator: u64) -> Option<u64> {
    let h = u64::try_from(btree_levels(len)?).ok()?;
    let per_node = comparator.checked_add(3)?.checked_mul(11)?.checked_add(6)?;
    h.checked_mul(per_node)?.checked_add(4)
}
pub(crate) fn btree_insert<K, V>(len: usize, comparator: u64) -> Option<u64> {
    let pair = pair_bytes::<K, V>()?;
    if len == 0 {
        return node_bytes::<K, V>()?.checked_add(pair)?.checked_add(39);
    }
    let search = btree_search(len, comparator)?;
    if len <= 10 {
        return u64::try_from(len.checked_add(1)?)
            .ok()?
            .checked_mul(pair)?
            .checked_add(search)?
            .checked_add(39);
    }
    let h = u64::try_from(btree_levels(len)?.checked_add(1)?).ok()?;
    let moves = pair
        .checked_mul(22)?
        .checked_add(u64::try_from(size_of::<usize>()).ok()?.checked_mul(24)?)?
        .checked_add(node_bytes::<K, V>()?)?
        .checked_add(203)?;
    moves.checked_mul(h)?.checked_add(search)?.checked_add(35)
}
pub(crate) fn btree_clone<K, V>(len: usize, payload: u64) -> Option<u64> {
    if len == 0 {
        return Some(8);
    }
    let n = u64::try_from(len).ok()?;
    node_bytes::<K, V>()?
        .checked_add(pair_bytes::<K, V>()?)?
        .checked_add(54)?
        .checked_mul(n)?
        .checked_add(payload)?
        .checked_add(16)
}
pub(crate) fn btree_bulk<K, V>(len: usize, comparator: u64) -> Option<u64> {
    if len == 0 {
        return Some(8);
    }
    let n = u64::try_from(len).ok()?;
    let h = u64::try_from(btree_levels(len)?).ok()?;
    let pair = pair_bytes::<K, V>()?;
    let border = pair
        .checked_mul(22)?
        .checked_add(u64::try_from(size_of::<usize>()).ok()?.checked_mul(24)?)?
        .checked_add(202)?
        .checked_mul(h)?;
    let entries = pair
        .checked_add(node_bytes::<K, V>()?)?
        .checked_add(54)?
        .checked_add(h.checked_mul(8)?)?
        .checked_add(comparator)?
        .checked_mul(n)?;
    entries.checked_add(border)?.checked_add(16)
}
pub(crate) fn btree_iter(len: usize) -> Option<u64> {
    u64::try_from(len).ok()?.checked_mul(27)?.checked_add(8)
}
/// ceil(log2(ceil(max(n,8)/8))) + 1, without rounding the quotient down.
pub(crate) fn growth_count(n: usize) -> Option<u64> {
    let mut q = n.max(8).checked_add(7)? / 8;
    let mut count = 1u64;
    while q > 1 {
        q = q.checked_add(1)? / 2;
        count = count.checked_add(1)?;
    }
    Some(count)
}
pub(crate) fn format_bytes(bytes: usize) -> Option<u64> {
    let n = u64::try_from(bytes).ok()?;
    let capacity = n.checked_mul(2)?.max(8);
    n.checked_mul(8)?
        .checked_add(capacity.checked_mul(3)?)?
        .checked_add(growth_count(bytes)?.checked_mul(9)?)?
        .checked_add(18)
}
pub(crate) fn custom_error_bytes(bytes: usize) -> Option<u64> {
    format_bytes(bytes)?
        .checked_add(u64::try_from(bytes).ok()?.checked_mul(26)?)?
        .checked_add(32)
}
