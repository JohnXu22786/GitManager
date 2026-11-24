//! Exact private predicate data for Rust b940084d / Unicode 17.0.0.
// Extracted/adapted, not a newly generated Unicode version.
// Copyright © 1991-2024 Unicode, Inc. (data, Unicode-3.0).
// Copyright (c) The Rust Project Contributors (adapted lookup, MIT).
// Complete notices and source hashes: third_party/rust-unicode-1.99.0/.
// Safe port of unicode_data.rs:67-146,263-343; Lt decoded from718-747.

struct ShortOffsetRunHeader { start: usize, end: u32 }
impl ShortOffsetRunHeader {
    const fn new(start: usize, end: u32) -> Self { Self { start, end } }
}
// Header numeric inputs and all919offset bytes are unchanged from upstream.
static SHORT_OFFSET_RUNS: [ShortOffsetRunHeader; 36] = [
        ShortOffsetRunHeader::new(0, 688), ShortOffsetRunHeader::new(11, 4957),
        ShortOffsetRunHeader::new(263, 5906), ShortOffsetRunHeader::new(265, 8125),
        ShortOffsetRunHeader::new(377, 11388), ShortOffsetRunHeader::new(411, 12293),
        ShortOffsetRunHeader::new(423, 40981), ShortOffsetRunHeader::new(435, 42232),
        ShortOffsetRunHeader::new(437, 42508), ShortOffsetRunHeader::new(439, 64286),
        ShortOffsetRunHeader::new(535, 65024), ShortOffsetRunHeader::new(539, 66045),
        ShortOffsetRunHeader::new(569, 67456), ShortOffsetRunHeader::new(575, 68097),
        ShortOffsetRunHeader::new(581, 68900), ShortOffsetRunHeader::new(593, 69291),
        ShortOffsetRunHeader::new(601, 71727), ShortOffsetRunHeader::new(727, 71995),
        ShortOffsetRunHeader::new(731, 73459), ShortOffsetRunHeader::new(797, 78896),
        ShortOffsetRunHeader::new(809, 90398), ShortOffsetRunHeader::new(813, 92912),
        ShortOffsetRunHeader::new(817, 93504), ShortOffsetRunHeader::new(823, 94031),
        ShortOffsetRunHeader::new(827, 110576), ShortOffsetRunHeader::new(837, 113821),
        ShortOffsetRunHeader::new(843, 118528), ShortOffsetRunHeader::new(847, 119143),
        ShortOffsetRunHeader::new(851, 121344), ShortOffsetRunHeader::new(861, 122880),
        ShortOffsetRunHeader::new(873, 123566), ShortOffsetRunHeader::new(889, 124139),
        ShortOffsetRunHeader::new(893, 125136), ShortOffsetRunHeader::new(907, 127995),
        ShortOffsetRunHeader::new(911, 917505), ShortOffsetRunHeader::new(913, 2032112),
    ];
static OFFSETS: [u8; 919] = [
        168, 1, 4, 1, 1, 1, 4, 1, 2, 2, 0, 192, 4, 2, 4, 1, 9, 2, 1, 1, 251, 7, 207, 1, 5, 1, 49,
        45, 1, 1, 1, 2, 1, 2, 1, 1, 44, 1, 11, 6, 10, 11, 1, 1, 35, 1, 10, 21, 16, 1, 101, 8, 1, 10,
        1, 4, 33, 1, 1, 1, 30, 27, 91, 11, 58, 11, 4, 1, 2, 1, 24, 24, 43, 3, 44, 1, 7, 2, 5, 9, 41,
        58, 55, 1, 1, 1, 4, 8, 4, 1, 3, 7, 10, 2, 13, 1, 15, 1, 58, 1, 4, 4, 8, 1, 20, 2, 26, 1, 2,
        2, 57, 1, 4, 2, 4, 2, 2, 3, 3, 1, 30, 2, 3, 1, 11, 2, 57, 1, 4, 5, 1, 2, 4, 1, 20, 2, 22, 6,
        1, 1, 58, 1, 2, 1, 1, 4, 8, 1, 7, 2, 11, 2, 30, 1, 61, 1, 12, 1, 50, 1, 3, 1, 55, 1, 1, 3,
        5, 3, 1, 4, 7, 2, 11, 2, 29, 1, 58, 1, 2, 1, 6, 1, 5, 2, 20, 2, 28, 2, 57, 2, 4, 4, 8, 1,
        20, 2, 29, 1, 72, 1, 7, 3, 1, 1, 90, 1, 2, 7, 11, 9, 98, 1, 2, 9, 9, 1, 1, 7, 73, 2, 27, 1,
        1, 1, 1, 1, 55, 14, 1, 5, 1, 2, 5, 11, 1, 36, 9, 1, 102, 4, 1, 6, 1, 2, 2, 2, 25, 2, 4, 3,
        16, 4, 13, 1, 2, 2, 6, 1, 15, 1, 94, 1, 0, 3, 0, 3, 29, 2, 30, 2, 30, 2, 64, 2, 1, 7, 8, 1,
        2, 11, 3, 1, 5, 1, 45, 5, 51, 1, 65, 2, 34, 1, 118, 3, 4, 2, 9, 1, 6, 3, 219, 2, 2, 1, 58,
        1, 1, 7, 1, 1, 1, 1, 2, 8, 6, 10, 2, 1, 39, 1, 8, 46, 2, 12, 20, 4, 48, 1, 1, 5, 1, 1, 5, 1,
        40, 9, 12, 2, 32, 4, 2, 2, 1, 3, 56, 1, 1, 2, 3, 1, 1, 3, 58, 8, 2, 2, 64, 6, 82, 3, 1, 13,
        1, 7, 4, 1, 6, 1, 3, 2, 50, 63, 13, 1, 34, 101, 0, 1, 1, 3, 11, 3, 13, 3, 13, 3, 13, 2, 12,
        5, 8, 2, 10, 1, 2, 1, 2, 5, 49, 5, 1, 10, 1, 1, 13, 1, 16, 13, 51, 33, 0, 2, 113, 3, 125, 1,
        15, 1, 96, 32, 47, 1, 0, 1, 36, 4, 3, 5, 5, 1, 93, 6, 93, 3, 0, 1, 0, 6, 0, 1, 98, 4, 1, 10,
        1, 1, 28, 4, 80, 2, 14, 34, 78, 1, 23, 3, 102, 4, 3, 2, 8, 1, 3, 1, 4, 1, 25, 2, 5, 1, 151,
        2, 26, 18, 13, 1, 38, 8, 25, 11, 46, 3, 48, 1, 2, 4, 2, 2, 17, 1, 21, 2, 66, 6, 2, 2, 2, 2,
        12, 1, 8, 1, 35, 1, 11, 1, 51, 1, 1, 3, 2, 2, 5, 2, 1, 1, 27, 1, 14, 2, 5, 2, 1, 1, 100, 5,
        9, 3, 121, 1, 2, 1, 4, 1, 0, 1, 147, 17, 0, 16, 3, 1, 12, 16, 34, 1, 2, 1, 169, 1, 7, 1, 6,
        1, 11, 1, 35, 1, 1, 1, 47, 1, 45, 2, 67, 1, 21, 3, 0, 1, 226, 1, 149, 5, 0, 6, 1, 42, 1, 9,
        0, 3, 1, 2, 5, 4, 40, 3, 4, 1, 165, 2, 0, 4, 38, 1, 26, 5, 1, 1, 0, 2, 24, 1, 52, 6, 70, 11,
        49, 4, 123, 1, 54, 15, 41, 1, 2, 2, 10, 3, 49, 4, 2, 2, 2, 1, 4, 1, 10, 1, 50, 3, 36, 5, 1,
        8, 62, 1, 12, 2, 52, 9, 10, 4, 2, 1, 95, 3, 2, 1, 1, 2, 6, 1, 2, 1, 157, 1, 3, 8, 21, 2, 57,
        2, 3, 1, 37, 7, 3, 5, 70, 6, 13, 1, 1, 1, 1, 1, 14, 2, 85, 8, 2, 3, 1, 1, 23, 1, 84, 6, 1,
        1, 4, 2, 1, 2, 238, 4, 6, 2, 1, 2, 27, 2, 85, 8, 2, 1, 1, 2, 106, 1, 1, 1, 2, 6, 1, 1, 101,
        1, 1, 1, 2, 4, 1, 5, 0, 9, 1, 2, 0, 2, 1, 1, 4, 1, 144, 4, 2, 2, 4, 1, 32, 10, 40, 6, 2, 4,
        8, 1, 9, 6, 2, 3, 46, 13, 1, 2, 198, 1, 1, 3, 1, 1, 201, 7, 1, 6, 1, 1, 82, 22, 2, 7, 1, 2,
        1, 2, 122, 6, 3, 1, 1, 2, 1, 7, 1, 1, 72, 2, 3, 1, 1, 1, 65, 1, 0, 2, 11, 2, 52, 5, 5, 1, 1,
        1, 23, 1, 0, 17, 6, 15, 0, 12, 3, 3, 0, 5, 59, 7, 9, 4, 0, 3, 40, 2, 0, 1, 63, 17, 64, 2, 1,
        2, 13, 2, 0, 4, 1, 7, 1, 2, 0, 2, 1, 4, 0, 46, 2, 23, 0, 3, 9, 16, 2, 7, 30, 4, 148, 3, 0,
        55, 4, 50, 8, 1, 14, 1, 22, 5, 1, 15, 0, 7, 1, 17, 2, 7, 1, 2, 1, 5, 5, 62, 33, 1, 160, 14,
        0, 1, 61, 4, 0, 5, 254, 2, 243, 1, 2, 1, 7, 2, 5, 1, 9, 1, 0, 7, 109, 8, 0, 5, 0, 1, 30, 96,
        128, 240, 0,
    ];

// Prove lookup totality at compile time, not by approximate runtime fallback.
const _: () = {
    assert!(SHORT_OFFSET_RUNS[0].start == 0);
    assert!(SHORT_OFFSET_RUNS[35].end > char::MAX as u32);
    assert!(OFFSETS[918] == 0);
    let mut run = 0;
    let mut previous = 0;
    while run < SHORT_OFFSET_RUNS.len() {
        let start = SHORT_OFFSET_RUNS[run].start;
        let end = if run + 1 < SHORT_OFFSET_RUNS.len() {
            SHORT_OFFSET_RUNS[run + 1].start
        } else { OFFSETS.len() };
        assert!(start < end && end <= OFFSETS.len());
        assert!(end - start - 1 <= 251);
        let limit = SHORT_OFFSET_RUNS[run].end;
        assert!(previous < limit && limit < (1 << 21));
        let mut i = start;
        let mut sum = 0u32;
        while i + 1 < end {
            assert!(sum <= u32::MAX - OFFSETS[i] as u32);
            sum += OFFSETS[i] as u32;
            i += 1;
        }
        assert!(sum <= limit - previous);
        previous = limit;
        run += 1;
    }
};

pub(super) fn case_ignorable(c: char) -> bool {
    if c.is_ascii() { return matches!(c, '\'' | '.' | ':' | '^' | '`'); }
    let needle = c as u32;
    if needle < 0xa8 { return false; }
    // Select first END prefix strictly greater than needle. The final prefix
    // exceeds every char; sorted headers make previous <=needle. At most36
    // tests. Safe indexing is total by the const invariants above.
    let mut run = 0;
    while needle >= SHORT_OFFSET_RUNS[run].end { run += 1; }
    let mut offset = SHORT_OFFSET_RUNS[run].start;
    let end = if run + 1 < SHORT_OFFSET_RUNS.len() {
        SHORT_OFFSET_RUNS[run + 1].start
    } else { OFFSETS.len() };
    let previous = if run == 0 { 0 } else { SHORT_OFFSET_RUNS[run - 1].end };
    let total = needle - previous;
    let mut prefix = 0;
    // The last offset in EACH run is a sentinel, not a prefix increment.
    // Keep zero offsets and GLOBAL offset-index parity exactly as upstream.
    while offset + 1 < end {
        prefix += OFFSETS[offset] as u32;
        if prefix > total { break; }
        offset += 1;
    }
    offset % 2 == 1
}

fn titlecase(c: char) -> bool {
    matches!(c, '\u{1c5}' | '\u{1c8}' | '\u{1cb}' | '\u{1f2}'
        | '\u{1f88}'..='\u{1f8f}' | '\u{1f98}'..='\u{1f9f}'
        | '\u{1fa8}'..='\u{1faf}' | '\u{1fbc}' | '\u{1fcc}' | '\u{1ffc}')
}

pub(super) fn cased(c: char) -> bool {
    // Stable pinned predicates plus the exact31Lt scalars; Alphabetic and
    // whether case mapping changes are NOT equivalent to Cased.
    c.is_lowercase() || c.is_uppercase() || titlecase(c)
}
