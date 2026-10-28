//! Fallible JSON syntax intake and borrowed canonical encoding foundations.
//! Not yet routed into proposal/App capture or original canonical/digest APIs.
//!
//! This closed tree has no user destructors/comparators/Clone/Deserialize. All
//! growth goes through product_alloc; partial trees drop using only String/Vec
//! destruction, at most 64 proposal or 127 store containers deep for parsed values.
//! Public construction of deeper trees is outside that parser cleanup bound.
//! No reader, typed conversion, paid-work fee, or whole-
//! request recoverable-OOM guarantee is supplied by this standalone module.
use crate::product_alloc::{ordered::OrderedMap, AllocationContext, AllocationFailure};
use std::fmt;
#[path = "product_json/parser.rs"]
mod parser;
#[path = "product_json/canonical.rs"]
pub mod canonical;

pub const MAX_INPUT_BYTES: usize = 1024 * 1024;
pub const MAX_NESTING_DEPTH: usize = 64;

/// Matches serde_json 1.0.150 default/std Number categories (including -0.0).
/// The parser only constructs finite floats. No arbitrary-precision strings.
#[derive(Debug)]
pub enum Number { U64(u64), I64(i64), F64(f64) }
pub enum Value {
    Null,
    Bool(bool),
    Number(Number),
    String(String),
    Array(Vec<Value>),
    Object(OrderedMap<String, Value>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyntaxCode {
    EofWhileParsingList, EofWhileParsingObject, EofWhileParsingString,
    EofWhileParsingValue, ExpectedColon, ExpectedListCommaOrEnd,
    ExpectedObjectCommaOrEnd, ExpectedSomeIdent, ExpectedSomeValue,
    InvalidEscape, InvalidNumber, NumberOutOfRange, InvalidUnicodeCodePoint,
    ControlCharacterWhileParsingString, KeyMustBeAString,
    LoneLeadingSurrogateInHexEscape, TrailingComma, TrailingCharacters,
    UnexpectedEndOfHexEscape, RecursionLimitExceeded,
}
impl SyntaxCode {
    // Text parity with serde_json 1.0.150 error.rs; licensing in parser.rs.
    pub fn message(self) -> &'static str {
        match self {
            Self::EofWhileParsingList => "EOF while parsing a list",
            Self::EofWhileParsingObject => "EOF while parsing an object",
            Self::EofWhileParsingString => "EOF while parsing a string",
            Self::EofWhileParsingValue => "EOF while parsing a value",
            Self::ExpectedColon => "expected `:`",
            Self::ExpectedListCommaOrEnd => "expected `,` or `]`",
            Self::ExpectedObjectCommaOrEnd => "expected `,` or `}`",
            Self::ExpectedSomeIdent => "expected ident",
            Self::ExpectedSomeValue => "expected value",
            Self::InvalidEscape => "invalid escape",
            Self::InvalidNumber => "invalid number",
            Self::NumberOutOfRange => "number out of range",
            Self::InvalidUnicodeCodePoint => "invalid unicode code point",
            Self::ControlCharacterWhileParsingString =>
                "control character (\\u0000-\\u001F) found while parsing a string",
            Self::KeyMustBeAString => "key must be a string",
            Self::LoneLeadingSurrogateInHexEscape => "lone leading surrogate in hex escape",
            Self::TrailingComma => "trailing comma",
            Self::TrailingCharacters => "trailing characters",
            Self::UnexpectedEndOfHexEscape => "unexpected end of hex escape",
            Self::RecursionLimitExceeded => "recursion limit exceeded",
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cause {
    InputTooLarge,
    NestingTooDeep,
    DuplicateObjectMember,
    Syntax(SyntaxCode),
    StoreSyntax(SyntaxCode),
    StoreDuplicateObjectMember,
    StoreFloatNotAllowed,
    Allocation(AllocationFailure),
}
/// Inline, Copy cause plus byte-oriented position; never owns a diagnostic.
/// Keep this value if a later fallible diagnostic renderer fails: rendering
/// failure must not replace the previously reached semantic/resource cause.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Error { pub cause: Cause, pub line: usize, pub column: usize }
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.cause {
            Cause::InputTooLarge => write!(f, "JSON input exceeds {MAX_INPUT_BYTES} bytes"),
            Cause::NestingTooDeep => write!(f, "JSON nesting exceeds {MAX_NESTING_DEPTH} containers"),
            Cause::DuplicateObjectMember => f.write_str("duplicate JSON object member"),
            Cause::Syntax(code) => write!(f, "invalid JSON: {} at line {} column {}",
                code.message(), self.line, self.column),
            Cause::StoreSyntax(code) => write!(f, "{} at line {} column {}",
                code.message(), self.line, self.column),
            Cause::StoreDuplicateObjectMember => write!(f,
                "duplicate JSON member at line {} column {}", self.line, self.column),
            Cause::StoreFloatNotAllowed => write!(f,
                "floating point values are not part of the store format at line {} column {}",
                self.line, self.column),
            Cause::Allocation(failure) => fmt::Display::fmt(&failure, f),
        }
    }
}
impl std::error::Error for Error {}

/// Check the existing 1 MiB encoded cap FIRST, then parse one value and EOF.
/// Duplicate *decoded* keys are rejected before colon/value parsing. A returned
/// tree is only syntax, not an accepted proposal or execution evidence.
/// Allocation refusal returns directly; there is no serde or heap fallback.
pub fn parse_json_bytes(bytes: &[u8], cx: &mut AllocationContext) -> Result<Value, Error> {
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(Error { cause: Cause::InputTooLarge, line: 0, column: 0 });
    }
    parser::parse(bytes, cx)
}

/// Parse the syntax policy of product_store/json.rs's original Strict visitor.
/// Floats (including -0) are rejected after lexical number parsing; duplicate
/// decoded keys are checked only after their value. Serde's default recursion
/// boundary admits 127 containers and rejects the 128th, not proposal depth64.
///
/// This syntax function deliberately has NO byte cap. The later request/work
/// envelope must prove the original caller's input bound before calling it:
/// CURRENT reads use 16 KiB, store objects use 16 MiB. Those bounds also keep
/// the inherited i32 number counters below i32::MAX. A future differently
/// bounded caller must close that obligation separately (not use proposal's
/// 1 MiB cap). This does not read files, decode a typed snapshot, or route any
/// existing store/backup call; those allocation paths remain separate work.
/// Errors keep Strict's unprefixed, line/column-aware text without allocating.
pub fn parse_store_json_bytes(bytes: &[u8], cx: &mut AllocationContext) -> Result<Value, Error> {
    parser::parse_store(bytes, cx)
}
