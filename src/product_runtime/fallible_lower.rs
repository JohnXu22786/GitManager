//! Unrouted whole-string lowercase for the pinned Rust 1.99.0 / Unicode 17.
//! No semantic limit, checkpoint, locale, normalization or error adapter.
// Final-sigma logic adapted from Rust's alloc/str.rs, copyright (c) The Rust
// Project Contributors. See third_party/rust-unicode-1.99.0 for MIT permission,
// Unicode data permission, exact provenance and modifications.
use crate::product_alloc::{try_reserve_string, AllocationContext,
    AllocationFailure, AllocationSite};
use std::{char::ToLowercase, str::CharIndices};
#[path = "unicode_lower_1_99.rs"]
mod unicode;

fn final_sigma(input: &str, index: usize) -> char {
    // Skip Case_Ignorable BEFORE testing Cased: the properties can overlap.
    // Context is always the original input, never already emitted lowercase.
    let before = input[..index].chars().rev().find(|&c| !unicode::case_ignorable(c));
    if before.is_some_and(unicode::cased) {
        let after = input[index + 'Σ'.len_utf8()..].chars()
            .find(|&c| !unicode::case_ignorable(c));
        if !after.is_some_and(unicode::cased) { return 'ς'; }
    }
    'σ'
}

// Inline cursors only. No caller callback, collection, recursion or allocation.
struct LowerChars<'a> {
    input: &'a str,
    chars: CharIndices<'a>,
    mapping: Option<ToLowercase>,
}
impl<'a> LowerChars<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, chars: input.char_indices(), mapping: None }
    }
}
impl Iterator for LowerChars<'_> {
    type Item = char;
    fn next(&mut self) -> Option<char> {
        loop {
            if let Some(mapping) = &mut self.mapping {
                if let Some(c) = mapping.next() { return Some(c); }
            }
            let (index, c) = self.chars.next()?;
            if c == 'Σ' { return Some(final_sigma(self.input, index)); }
            // Stable inline iterator uses the exact pinned scalar mapping and
            // retains an input NUL; no hand-built sentinel filtering.
            self.mapping = Some(c.to_lowercase());
        }
    }
}

// Precondition: total <= M; all callers maintain it. k is in1..=4, so total+k
// remains representable in usize even when the String extent exceeds M.
fn add_length(total: usize, c: char, cx: &mut AllocationContext)
    -> Result<usize, AllocationFailure> {
    let k = c.len_utf8();
    if total > (isize::MAX as usize) - k {
        // The accepted foundation records CapacityOverflow BEFORE growth.
        // Do not manufacture an unrecorded error or alter a sticky first cause.
        let mut empty = String::new();
        try_reserve_string(&mut empty, total + k, cx, AllocationSite::String)?;
    }
    Ok(total + k)
}

/// Borrow input, size exactly, reserve fallibly once, emit without growth.
/// The returned failure is the foundation's exact allocation-free identity.
pub fn try_lowercase(input: &str, cx: &mut AllocationContext)
    -> Result<String, AllocationFailure> {
    let mut total = 0;
    for c in LowerChars::new(input) { total = add_length(total, c, cx)?; }
    let mut output = String::new();
    try_reserve_string(&mut output, total, cx, AllocationSite::String)?;
    // Immutable input, tables and the SAME deterministic walker imply exactly
    // total emitted bytes. Each prefix+next UTF-8 char <=total<=capacity, hence
    // push cannot grow. Empty reserves no growth; no fallible step follows a
    // successful nonempty reserve. Failure leaves no allocated partial output.
    for c in LowerChars::new(input) { output.push(c); }
    Ok(output)
}

#[cfg(test)]
pub(crate) fn test_add_length(total: usize, c: char, cx: &mut AllocationContext)
    -> Result<usize, AllocationFailure> {
    assert!(total <= isize::MAX as usize);
    add_length(total, c, cx)
}
