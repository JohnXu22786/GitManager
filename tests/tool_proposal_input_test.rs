#[path = "../src/tool_proposal_input.rs"]
mod tool_proposal_input;

use serde_json::{json, Value};
use std::io::{self, Cursor, Read};
use tool_proposal_input::{
    parse_json_bytes, read_json, InputError, MAX_INPUT_BYTES, MAX_NESTING_DEPTH,
};

#[test]
fn preserves_json_values_unicode_and_untrusted_members() {
    let input = br#" {"null":null,"booleans":[true,false],"numbers":[-9223372036854775808,18446744073709551615,1.25,-2e3],"text":"\u4e2d\u6587 \ud83d\ude80","passed":true,"command":"echo untrusted","path":"../../data"} "#;
    let expected: Value = serde_json::from_slice(input).unwrap();
    assert_eq!(parse_json_bytes(input).unwrap(), expected);
    assert_eq!(read_json(Cursor::new(input)).unwrap(), expected);
    assert_eq!(expected["text"], "中文 🚀");
    for input in [
        "null",
        "true",
        "false",
        "0",
        "-0.5",
        "\"中文 🚀\"",
        "[]",
        "{}",
    ] {
        assert_eq!(
            parse_json_bytes(input.as_bytes()).unwrap(),
            serde_json::from_str::<Value>(input).unwrap()
        );
    }
}

#[test]
fn member_names_are_compared_within_each_object_without_unicode_normalization() {
    let input = r#"{"first":{"x":1},"second":{"x":2},"é":3,"e\u0301":4,"X":5,"x":6}"#;
    assert_eq!(
        parse_json_bytes(input.as_bytes()).unwrap(),
        serde_json::from_str::<Value>(input).unwrap()
    );
}

#[test]
fn rejects_duplicate_members_including_decoded_escapes_and_nested_objects() {
    for input in [
        r#"{"x":1,"x":2}"#,
        r#"{"x":1,"\u0078":2}"#,
        r#"{"outer":[{"中文":1,"\u4e2d\u6587":2}]}"#,
        r#"{"🚀":1,"\ud83d\ude80":2}"#,
        r#"{"":1,"":2}"#,
    ] {
        assert!(
            matches!(
                parse_json_bytes(input.as_bytes()),
                Err(InputError::DuplicateObjectMember)
            ),
            "{input}"
        );
        assert!(matches!(
            read_json(Cursor::new(input)),
            Err(InputError::DuplicateObjectMember)
        ));
    }
}

#[test]
fn rejects_empty_malformed_trailing_and_invalid_utf8_input() {
    let inputs: &[&[u8]] = &[
        b"",
        b" \r\n\t",
        b"{",
        b"[1,]",
        b"{\"x\":1,}",
        b"{x:1}",
        b"01",
        b"NaN",
        b"1e9999",
        b"true false",
        b"{}[]",
        b"null!",
        br#""\ud800""#,
        br#""\q""#,
        b"\"\xff\"",
        b"null\xff",
        b"\xef\xbb\xbfnull",
        b"\"raw\nline\"",
        b"// comment\nnull",
    ];
    for input in inputs {
        assert!(
            matches!(parse_json_bytes(input), Err(InputError::InvalidJson(_))),
            "{input:?}"
        );
        assert!(
            matches!(
                read_json(Cursor::new(input)),
                Err(InputError::InvalidJson(_))
            ),
            "{input:?}"
        );
    }
}

#[test]
fn byte_limit_is_inclusive_and_counts_utf8_and_whitespace() {
    let mut input = format!("\"{}\"", "é".repeat((MAX_INPUT_BYTES - 2) / 2)).into_bytes();
    input.resize(MAX_INPUT_BYTES, b' ');
    assert!(parse_json_bytes(&input).is_ok());
    input.push(b' ');
    assert!(matches!(
        parse_json_bytes(&input),
        Err(InputError::InputTooLarge)
    ));
    // The size guard runs before parsing even malformed oversized data.
    input[0] = 0xff;
    assert!(matches!(
        parse_json_bytes(&input),
        Err(InputError::InputTooLarge)
    ));
}

#[test]
fn reader_accepts_exact_byte_limit_and_rejects_one_more_byte() {
    let mut input = b"null".to_vec();
    input.resize(MAX_INPUT_BYTES, b' ');
    let mut reader = Cursor::new(&input);
    assert_eq!(read_json(&mut reader).unwrap(), Value::Null);
    assert_eq!(reader.position() as usize, MAX_INPUT_BYTES);
    input.push(b' ');
    assert!(matches!(
        read_json(Cursor::new(input)),
        Err(InputError::InputTooLarge)
    ));
}

struct CappedProbe {
    bytes_read: usize,
    reads_after_cap: usize,
}

impl Read for CappedProbe {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.bytes_read == MAX_INPUT_BYTES + 1 {
            self.reads_after_cap += 1;
            return Err(io::Error::other("read beyond the intake cap"));
        }
        let count = buf.len().min(MAX_INPUT_BYTES + 1 - self.bytes_read);
        buf[..count].fill(b' ');
        self.bytes_read += count;
        Ok(count)
    }
}

#[test]
fn reader_stops_after_one_excess_byte_without_reading_to_eof() {
    let mut reader = CappedProbe {
        bytes_read: 0,
        reads_after_cap: 0,
    };
    assert!(matches!(
        read_json(&mut reader),
        Err(InputError::InputTooLarge)
    ));
    assert_eq!(reader.bytes_read, MAX_INPUT_BYTES + 1);
    assert_eq!(reader.reads_after_cap, 0);
}

struct FailingAfter<'a> {
    remaining: &'a [u8],
}

impl Read for FailingAfter<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.remaining.is_empty() {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "input unavailable",
            ))
        } else {
            self.remaining.read(buf)
        }
    }
}

#[test]
fn reader_preserves_io_errors_even_after_a_valid_prefix_or_exact_limit() {
    let mut full = b"null".to_vec();
    full.resize(MAX_INPUT_BYTES, b' ');
    for input in [b"".as_slice(), b"null", full.as_slice()] {
        let error = read_json(FailingAfter { remaining: input }).unwrap_err();
        match &error {
            InputError::Io(source) => {
                assert_eq!(source.kind(), io::ErrorKind::PermissionDenied);
                assert_eq!(source.to_string(), "input unavailable");
            }
            other => panic!("expected original I/O error, got {other:?}"),
        }
        assert!(std::error::Error::source(&error).is_some());
    }
}

struct ShortReads<'a> {
    remaining: &'a [u8],
    interrupted: bool,
}

impl Read for ShortReads<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if !self.interrupted {
            self.interrupted = true;
            return Err(io::ErrorKind::Interrupted.into());
        }
        let length = buf.len().min(1);
        self.remaining.read(&mut buf[..length])
    }
}

#[test]
fn reader_handles_interruptions_and_unicode_split_across_short_reads() {
    let input = "{\"中文\":\"🚀\"}";
    assert_eq!(
        read_json(ShortReads {
            remaining: input.as_bytes(),
            interrupted: false
        })
        .unwrap(),
        json!({"中文": "🚀"})
    );
}

fn nested(depth: usize, object: bool) -> String {
    let (open, close) = if object { ("{\"x\":", "}") } else { ("[", "]") };
    format!("{}null{}", open.repeat(depth), close.repeat(depth))
}

#[test]
fn nesting_limit_counts_containers_and_accepts_the_exact_limit() {
    for object in [false, true] {
        for depth in [MAX_NESTING_DEPTH - 1, MAX_NESTING_DEPTH] {
            let input = nested(depth, object);
            assert!(
                parse_json_bytes(input.as_bytes()).is_ok(),
                "depth {depth}, object {object}"
            );
        }
        let input = nested(MAX_NESTING_DEPTH + 1, object);
        assert!(matches!(
            parse_json_bytes(input.as_bytes()),
            Err(InputError::NestingTooDeep)
        ));
    }
    let input = format!(
        "{}{}",
        "[".repeat(MAX_NESTING_DEPTH),
        "]".repeat(MAX_NESTING_DEPTH)
    );
    assert!(parse_json_bytes(input.as_bytes()).is_ok());
    // Array and object depths must share one budget.
    let input = format!(
        "{}{{}}{}",
        "[".repeat(MAX_NESTING_DEPTH),
        "]".repeat(MAX_NESTING_DEPTH)
    );
    assert!(matches!(
        parse_json_bytes(input.as_bytes()),
        Err(InputError::NestingTooDeep)
    ));
}

#[test]
fn deeply_nested_hostile_input_returns_an_error_without_exhausting_the_stack() {
    for object in [false, true] {
        let input = nested(20_000, object);
        assert!(input.len() < MAX_INPUT_BYTES);
        assert!(matches!(
            parse_json_bytes(input.as_bytes()),
            Err(InputError::NestingTooDeep)
        ));
        assert!(matches!(
            read_json(Cursor::new(input)),
            Err(InputError::NestingTooDeep)
        ));
    }
}

#[test]
fn sibling_containers_and_brackets_inside_strings_do_not_consume_depth() {
    let input = format!(
        "[{}]",
        vec![r#"{"text":"[ { \\\" } ]"}"#; MAX_NESTING_DEPTH + 1].join(",")
    );
    assert_eq!(
        parse_json_bytes(input.as_bytes()).unwrap(),
        serde_json::from_str::<Value>(&input).unwrap()
    );
}
