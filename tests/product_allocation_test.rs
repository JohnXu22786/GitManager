#[path = "../src/product_alloc.rs"]
mod product_alloc;

use product_alloc::*;
use product_alloc::{AllocationContext as Context, FallibleStringWriter as Writer};
use product_alloc::AllocationFailureKind as Kind;
use std::cell::Cell;
use std::fmt::{self, Write};
use std::num::NonZeroUsize;

const SITE: AllocationSite = AllocationSite::Vector;
const STRING: AllocationSite = AllocationSite::String;
const FORMAT: AllocationSite = AllocationSite::Formatting;
fn fault(n: usize, sticky: bool) -> Context {
    Context::fail_nth_growth(NonZeroUsize::new(n).unwrap(), sticky)
}

#[test]
fn no_growth_empty_and_zst_do_not_consume_injection() {
    let mut values = Vec::<u8>::with_capacity(4);
    let mut text = String::with_capacity(4);
    let mut units = Vec::<()>::new();
    let mut cx = fault(1, false);
    try_reserve(&mut values, 0, &mut cx, SITE).unwrap();
    try_extend_bytes(&mut values, b"", &mut cx, SITE).unwrap();
    let cap = values.capacity();
    for _ in 0..cap { try_push(&mut values, 7, &mut cx, SITE).unwrap(); }
    for _ in 0..text.capacity() {
        try_append_str(&mut text, "x", &mut cx, STRING).unwrap();
    }
    try_append_str(&mut text, "", &mut cx, STRING).unwrap();
    assert_eq!(try_copy_str("", &mut cx, STRING).unwrap(), "");
    try_reserve(&mut units, usize::MAX, &mut cx, SITE).unwrap();
    try_push(&mut units, (), &mut cx, SITE).unwrap();
    assert_eq!(cx.growth_requests(), 0);
    let error = try_push(&mut values, 9, &mut cx, SITE).unwrap_err();
    assert_eq!(error.kind, Kind::Injected);
    assert_eq!(error.requested_bytes, Some(cap + 1));
    assert_eq!(values.len(), cap);
    assert!(values.iter().all(|x| *x == 7));
    assert_eq!(cx.growth_requests(), 1);
}

fn three_growths(cx: &mut Context) -> Result<(), AllocationFailure> {
    let mut bytes = Vec::new();
    try_extend_bytes(&mut bytes, b"abc", cx, AllocationSite::Bytes)?;
    try_extend_bytes(&mut bytes, b"", cx, AllocationSite::Bytes)?;
    let text = try_copy_str("中", cx, STRING)?;
    let mut words = Vec::new();
    try_push(&mut words, 42_u64, cx, SITE)?;
    assert_eq!(bytes, b"abc");
    assert_eq!(text, "中");
    assert_eq!(words, [42]);
    Ok(())
}

#[test]
fn every_actual_growth_is_deterministically_injectable() {
    let mut normal = Context::new();
    three_growths(&mut normal).unwrap();
    let expected = [(AllocationSite::Bytes, 3), (STRING, 3), (SITE, 8)];
    assert_eq!(normal.growth_requests(), expected.len());
    for (index, (site, bytes)) in expected.into_iter().enumerate() {
        let mut cx = fault(index + 1, false);
        let error = three_growths(&mut cx).unwrap_err();
        assert_eq!(error.site, site);
        assert_eq!(error.requested_bytes, Some(bytes));
        assert_eq!(cx.growth_requests(), index + 1);
        assert_eq!(cx.first_failure(), Some(error));
        try_copy_str("later", &mut cx, STRING).unwrap();
    }
    three_growths(&mut fault(4, false)).unwrap();
}

#[test]
fn failed_append_preserves_original_contents_and_capacity() {
    let mut bytes = b"old".to_vec();
    let mut text = String::from("旧");
    let (bp, bc) = (bytes.as_ptr(), bytes.capacity());
    let (tp, tc) = (text.as_ptr(), text.capacity());
    let extra = vec![b'x'; bc + 1];
    let more = "x".repeat(tc + 1);
    let mut cx = fault(1, true);
    let first = try_extend_bytes(&mut bytes, &extra, &mut cx, AllocationSite::Bytes).unwrap_err();
    let second = try_append_str(&mut text, &more, &mut cx, STRING).unwrap_err();
    assert_eq!(bytes, b"old");
    assert_eq!(text, "旧");
    assert_eq!((bytes.as_ptr(), bytes.capacity()), (bp, bc));
    assert_eq!((text.as_ptr(), text.capacity()), (tp, tc));
    assert_eq!(second.requested_bytes, Some(3 + more.len()));
    assert_eq!(cx.first_failure(), Some(first));
    assert_eq!(cx.growth_requests(), 2);
}

#[test]
fn rejected_owned_value_and_nested_owners_drop_exactly_once() {
    struct Tracked<'a>(&'a Cell<usize>);
    impl Drop for Tracked<'_> {
        fn drop(&mut self) { self.0.set(self.0.get() + 1); }
    }
    let old_drop = Cell::new(0);
    let new_drops = Cell::new(0);
    let mut values = Vec::with_capacity(1);
    for _ in 0..values.capacity() { values.push(vec![Tracked(&old_drop)]); }
    let old_len = values.len();
    let incoming = vec![Tracked(&new_drops), Tracked(&new_drops)];
    let mut cx = fault(1, true);
    let error = try_push(&mut values, incoming, &mut cx, SITE).unwrap_err();
    assert_eq!(error.kind, Kind::Injected);
    assert_eq!(values.len(), old_len);
    assert_eq!(old_drop.get(), 0);
    assert_eq!(new_drops.get(), 2);
    drop(values);
    assert_eq!(old_drop.get(), old_len);
    assert_eq!(new_drops.get(), 2);
}

#[test]
fn overflow_precedes_growth_and_preserves_representable_byte_request() {
    let mut bytes = vec![1_u8];
    let mut words = Vec::<u16>::new();
    let mut text = String::from("x");
    let mut units = vec![()];
    let mut cx = fault(1, true);
    let len = try_reserve(&mut bytes, usize::MAX, &mut cx, SITE).unwrap_err();
    assert_eq!(len.kind, Kind::CapacityOverflow);
    assert_eq!(len.requested_bytes, None);
    let mul = try_reserve(&mut words, usize::MAX / 2 + 1, &mut cx, SITE).unwrap_err();
    assert_eq!(mul.requested_bytes, None);
    let layout = try_reserve(&mut Vec::<u8>::new(), isize::MAX as usize + 1, &mut cx, SITE).unwrap_err();
    assert_eq!(layout.requested_bytes, Some(isize::MAX as usize + 1));
    let string = try_reserve_string(&mut text, usize::MAX, &mut cx, STRING).unwrap_err();
    assert_eq!(string.kind, Kind::CapacityOverflow);
    let zst = try_reserve(&mut units, usize::MAX, &mut cx, SITE).unwrap_err();
    assert_eq!(zst.kind, Kind::CapacityOverflow);
    assert_eq!(cx.growth_requests(), 0);
    assert_eq!(bytes, [1]);
    assert_eq!(text, "x");
    assert_eq!(units.len(), 1);
    assert_eq!(cx.first_failure(), Some(len));
    assert_eq!(try_copy_str("later", &mut cx, STRING).unwrap_err().kind,
        Kind::Injected);
}

#[test]
fn unicode_copy_append_and_contexts_are_independent() {
    let mut cx = Context::new();
    let mut refusing = fault(1, false);
    let mut text = try_copy_str("中é", &mut cx, STRING).unwrap();
    try_append_str(&mut text, "🚀e\u{301}", &mut cx, STRING).unwrap();
    assert_eq!(text, "中é🚀e\u{301}");
    let error = try_copy_str("🚀", &mut refusing, STRING).unwrap_err();
    assert_eq!(error.requested_bytes, Some(4));
    assert_eq!(cx.first_failure(), None);
}

#[test]
fn closed_builtin_formatting_succeeds_without_altering_unicode() {
    let mut text = String::from("prefix:");
    let mut cx = Context::new();
    try_append_fmt(&mut text, format_args!("{}:{}:{:x}:{}", "中", -42_i64, 255_u32, '🚀'),
        &mut cx, FORMAT).unwrap();
    assert_eq!(text, "prefix:中:-42:ff:🚀");
}

#[test]
fn formatting_retains_first_concrete_failure_and_rolls_back() {
    let mut text = String::with_capacity(8);
    text.push_str("old");
    let fill = "x".repeat(text.capacity() - text.len());
    let capacity = text.capacity();
    let mut cx = fault(1, true);
    let first;
    {
        let mut sink = Writer::new(&mut text, &mut cx, FORMAT);
        sink.write_str(&fill).unwrap();
        assert!(sink.write_char('🚀').is_err());
        first = sink.failure().unwrap();
        assert!(sink.write_str("").is_err());
        assert!(write!(&mut sink, "{}", 7_u32).is_err());
        assert_eq!(sink.failure(), Some(first));
        assert_eq!(sink.finish(Err(fmt::Error)), Err(FormatFailure::Allocation(first)));
    }
    assert_eq!(text, "old");
    assert_eq!(first.requested_bytes, Some(capacity + 4));
    assert_eq!(cx.growth_requests(), 1);
    assert_eq!(cx.first_failure(), Some(first));
    assert!(try_copy_str("later", &mut cx, STRING).is_err());
    assert_eq!(cx.first_failure(), Some(first));
}

#[test]
fn formatting_error_or_unfinished_sink_does_not_commit_partial_text() {
    let mut text = String::with_capacity(32);
    text.push_str("old");
    let mut cx = fault(1, true);
    {
        let mut sink = Writer::new(&mut text, &mut cx, FORMAT);
        sink.write_str(" partial").unwrap();
        assert_eq!(sink.finish(Err(fmt::Error)), Err(FormatFailure::Formatter));
    }
    assert_eq!(text, "old");
    {
        let mut sink = Writer::new(&mut text, &mut cx, FORMAT);
        sink.write_str(" abandoned").unwrap();
    }
    assert_eq!(text, "old");
    assert_eq!(cx.first_failure(), None);
    assert_eq!(cx.growth_requests(), 0);
}

#[test]
fn error_display_and_propagation_use_fixed_storage_after_failure() {
    struct Fixed { bytes: [u8; 256], len: usize }
    impl Write for Fixed {
        fn write_str(&mut self, value: &str) -> fmt::Result {
            let end = self.len.checked_add(value.len()).ok_or(fmt::Error)?;
            let slot = self.bytes.get_mut(self.len..end).ok_or(fmt::Error)?;
            slot.copy_from_slice(value.as_bytes());
            self.len = end;
            Ok(())
        }
    }
    let mut cx = fault(1, true);
    let failure = try_copy_str("中", &mut cx, STRING).unwrap_err();
    let copied = failure;
    let mut fixed = Fixed { bytes: [0; 256], len: 0 };
    write!(&mut fixed, "{}", copied).unwrap();
    assert_eq!(&fixed.bytes[..fixed.len], b"allocation failure (injected) at string: 3 bytes");
    assert_eq!(cx.first_failure(), Some(failure));
    assert_eq!(cx.growth_requests(), 1);
    assert!(try_copy_str("later", &mut cx, STRING).is_err());
    assert_eq!(cx.first_failure(), Some(failure));
}
