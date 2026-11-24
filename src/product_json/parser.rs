// Lexical state/error positions and default-feature numeric conversion adapted
// from serde_json 1.0.150 src/{de,read,error}.rs (Serde project contributors,
// https://github.com/serde-rs/json/tree/v1.0.150). Numeric POW10 table and
// truncation/rounding branches deliberately retain that implementation, NOT
// std::str::parse's different rounding. Scalar safe scanning replaces SIMD;
// generic serde visitors, scratch growth and boxed errors are not used here.
// Upstream is MIT OR Apache-2.0; this adaptation uses the MIT option:
//
// Permission is hereby granted, free of charge, to any
// person obtaining a copy of this software and associated
// documentation files (the "Software"), to deal in the
// Software without restriction, including without
// limitation the rights to use, copy, modify, merge,
// publish, distribute, sublicense, and/or sell copies of
// the Software, and to permit persons to whom the Software
// is furnished to do so, subject to the following
// conditions:
//
// The above copyright notice and this permission notice
// shall be included in all copies or substantial portions
// of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
// ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
// TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
// PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
// SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
// CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
// OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
// IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

use super::{Cause, Error, Number, SyntaxCode as Code, Value, MAX_NESTING_DEPTH};
use crate::product_alloc::{self as alloc, ordered::OrderedMap,
    AllocationContext, AllocationFailure, AllocationSite};

type Result<T> = std::result::Result<T, Error>;
const MAX_PARSER_CONTAINERS: usize = 127;
enum ContainerKind { Array, Object }
enum ReadValue { Scalar(Value), Container(ContainerKind) }
enum ParseFrame {
    Array { values: Vec<Value>, first: bool },
    Object { values: OrderedMap<String, Value>, first: bool, pending_key: Option<String> },
}
enum FrameStep { ReadValue, Close }

pub(super) fn parse(bytes: &[u8], cx: &mut AllocationContext) -> Result<Value> {
    parse_with_policy(bytes, cx, Policy::Proposal)
}
pub(super) fn parse_store(bytes: &[u8], cx: &mut AllocationContext) -> Result<Value> {
    parse_with_policy(bytes, cx, Policy::Store)
}
#[derive(Clone, Copy, Eq, PartialEq)]
enum Policy { Proposal, Store }
fn parse_with_policy(bytes: &[u8], cx: &mut AllocationContext, policy: Policy) -> Result<Value> {
    let mut parser = Parser { bytes, index: 0, cx, policy };
    let value = parser.value()?;
    if parser.whitespace().is_some() { return Err(parser.peek_error(Code::TrailingCharacters)); }
    Ok(value)
}
struct Parser<'b, 'c> {
    bytes: &'b [u8], index: usize, cx: &'c mut AllocationContext, policy: Policy,
}
impl Parser<'_, '_> {
    fn peek(&self) -> Option<u8> { self.bytes.get(self.index).copied() }
    fn peek_or_null(&self) -> u8 { self.peek().unwrap_or(0) }
    fn next(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.index += 1;
        Some(byte)
    }
    fn whitespace(&mut self) -> Option<u8> {
        while matches!(self.peek(), Some(b' ' | b'\n' | b'\r' | b'\t')) { self.index += 1; }
        self.peek()
    }
    fn at(&self, cause: Cause, index: usize) -> Error {
        // Byte columns, including CR and UTF-8 bytes, exactly like SliceRead.
        // Only computed on an error; no allocation or diagnostic formatting.
        let (mut line, mut column) = (1, 0);
        for &byte in &self.bytes[..index] {
            if byte == b'\n' { line += 1; column = 0; } else { column += 1; }
        }
        Error { cause, line, column }
    }
    fn syntax(&self, code: Code) -> Cause {
        match self.policy { Policy::Proposal => Cause::Syntax(code), Policy::Store => Cause::StoreSyntax(code) }
    }
    fn error(&self, code: Code) -> Error { self.at(self.syntax(code), self.index) }
    fn peek_error(&self, code: Code) -> Error {
        self.at(self.syntax(code), (self.index + 1).min(self.bytes.len()))
    }
    fn allocation(&self, failure: AllocationFailure) -> Error {
        self.at(Cause::Allocation(failure), self.index)
    }
    fn read_value(&mut self, depth: usize) -> Result<ReadValue> {
        match self.whitespace() {
            None => Err(self.peek_error(Code::EofWhileParsingValue)),
            Some(b'n') => { self.index += 1; self.ident(b"ull")?; Ok(ReadValue::Scalar(Value::Null)) }
            Some(b't') => { self.index += 1; self.ident(b"rue")?; Ok(ReadValue::Scalar(Value::Bool(true))) }
            Some(b'f') => { self.index += 1; self.ident(b"alse")?; Ok(ReadValue::Scalar(Value::Bool(false))) }
            Some(b'-') => { self.index += 1; self.number_value(false).map(ReadValue::Scalar) }
            Some(b'0'..=b'9') => self.number_value(true).map(ReadValue::Scalar),
            Some(b'"') => { self.index += 1; self.string().map(Value::String).map(ReadValue::Scalar) }
            Some(b'[' | b'{') => {
                // Serde decrements its initial 128 before consuming the token.
                if self.policy == Policy::Store && depth >= 127 {
                    return Err(self.peek_error(Code::RecursionLimitExceeded));
                }
                let object = self.next() == Some(b'{');
                if self.policy == Policy::Proposal && depth >= MAX_NESTING_DEPTH {
                    return Err(self.at(Cause::NestingTooDeep, self.index));
                }
                Ok(ReadValue::Container(if object { ContainerKind::Object } else { ContainerKind::Array }))
            }
            Some(_) => Err(self.peek_error(Code::ExpectedSomeValue)),
        }
    }
    fn value(&mut self) -> Result<Value> {
        // JSON depth is explicitly bounded by the proposal/store policies.
        // Keep parser frames in this fixed local array instead of recursively
        // calling value -> array/object -> value: accepted 64-level inputs
        // must work on the default Rust test-thread stack.
        let mut frames: [Option<ParseFrame>; MAX_PARSER_CONTAINERS] =
            std::array::from_fn(|_| None);
        let mut frame_count = 0usize;
        let mut completed = None;
        match self.read_value(0)? {
            ReadValue::Scalar(value) => completed = Some(value),
            ReadValue::Container(kind) => {
                frames[0] = Some(Self::new_frame(kind));
                frame_count = 1;
            }
        }

        loop {
            if let Some(value) = completed.take() {
                if frame_count == 0 { return Ok(value); }
                match frames[frame_count - 1].as_mut().expect("active parser frame") {
                    ParseFrame::Array { values, .. } => {
                        alloc::try_push(values, value, self.cx, AllocationSite::Vector)
                            .map_err(|e| self.allocation(e))?;
                    }
                    ParseFrame::Object { values, pending_key, .. } => {
                        let key = pending_key.take().expect("object value has a parsed key");
                        if self.policy == Policy::Store && values.contains_key(key.as_str()) {
                            // Strict's visitor checks a repeated key only after
                            // fully decoding its value, then consumes `}` if
                            // present before positioning the duplicate error.
                            if self.whitespace() == Some(b'}') { self.index += 1; }
                            return Err(self.at(Cause::StoreDuplicateObjectMember, self.index));
                        }
                        values.try_insert(key, value, self.cx)
                            .map_err(|e| self.allocation(e))?;
                    }
                }
                continue;
            }

            let step = match frames[frame_count - 1].as_mut().expect("active parser frame") {
                ParseFrame::Array { first, .. } => match self.whitespace() {
                    None => return Err(self.peek_error(Code::EofWhileParsingList)),
                    Some(b']') => { self.index += 1; FrameStep::Close }
                    Some(_) if *first => { *first = false; FrameStep::ReadValue }
                    Some(b',') => {
                        self.index += 1;
                        match self.whitespace() {
                            Some(b']') => return Err(self.peek_error(Code::TrailingComma)),
                            None => return Err(self.peek_error(Code::EofWhileParsingValue)),
                            _ => FrameStep::ReadValue,
                        }
                    }
                    _ => return Err(self.peek_error(Code::ExpectedListCommaOrEnd)),
                },
                ParseFrame::Object { values, first, pending_key } => match self.whitespace() {
                    None => return Err(self.peek_error(Code::EofWhileParsingObject)),
                    Some(b'}') => { self.index += 1; FrameStep::Close }
                    Some(_) if *first => {
                        *first = false;
                        if self.peek() != Some(b'"') {
                            return Err(self.peek_error(Code::KeyMustBeAString));
                        }
                        self.object_key(values, pending_key)?;
                        FrameStep::ReadValue
                    }
                    Some(b',') => {
                        self.index += 1;
                        match self.whitespace() {
                            Some(b'"') => (),
                            Some(b'}') => return Err(self.peek_error(Code::TrailingComma)),
                            None => return Err(self.peek_error(Code::EofWhileParsingValue)),
                            _ => return Err(self.peek_error(Code::KeyMustBeAString)),
                        }
                        self.object_key(values, pending_key)?;
                        FrameStep::ReadValue
                    }
                    _ => return Err(self.peek_error(Code::ExpectedObjectCommaOrEnd)),
                },
            };

            match step {
                FrameStep::Close => {
                    let frame = frames[frame_count - 1].take().expect("closing parser frame");
                    frame_count -= 1;
                    completed = Some(match frame {
                        ParseFrame::Array { values, .. } => Value::Array(values),
                        ParseFrame::Object { values, .. } => Value::Object(values),
                    });
                }
                FrameStep::ReadValue => match self.read_value(frame_count)? {
                    ReadValue::Scalar(value) => completed = Some(value),
                    ReadValue::Container(kind) => {
                        // read_value has already enforced the policy's exact
                        // 64/127-container boundary before this push.
                        frames[frame_count] = Some(Self::new_frame(kind));
                        frame_count += 1;
                    }
                },
            }
        }
    }
    fn new_frame(kind: ContainerKind) -> ParseFrame {
        match kind {
            ContainerKind::Array => ParseFrame::Array { values: Vec::new(), first: true },
            ContainerKind::Object => ParseFrame::Object {
                values: OrderedMap::new(), first: true, pending_key: None,
            },
        }
    }
    fn object_key(
        &mut self,
        values: &OrderedMap<String, Value>,
        pending_key: &mut Option<String>,
    ) -> Result<()> {
        self.index += 1; // opening key quote
        let key = self.string()?;
        if self.policy == Policy::Proposal && values.contains_key(key.as_str()) {
            return Err(self.at(Cause::DuplicateObjectMember, self.index));
        }
        match self.whitespace() {
            Some(b':') => self.index += 1,
            None => return Err(self.peek_error(Code::EofWhileParsingObject)),
            _ => return Err(self.peek_error(Code::ExpectedColon)),
        }
        *pending_key = Some(key);
        Ok(())
    }
    fn ident(&mut self, expected: &[u8]) -> Result<()> {
        for &byte in expected {
            match self.next() {
                None => return Err(self.error(Code::EofWhileParsingValue)),
                Some(actual) if actual != byte => return Err(self.error(Code::ExpectedSomeIdent)),
                _ => (),
            }
        }
        Ok(())
    }
    fn append(&mut self, output: &mut Vec<u8>, bytes: &[u8]) -> Result<()> {
        alloc::try_extend_bytes(output, bytes, self.cx, AllocationSite::String)
            .map_err(|e| self.allocation(e))
    }
    fn string(&mut self) -> Result<String> {
        let mut start = self.index;
        let mut output = Vec::new();
        loop {
            while matches!(self.peek(), Some(b) if b >= 0x20 && b != b'"' && b != b'\\') {
                self.index += 1;
            }
            match self.next() {
                None => return Err(self.error(Code::EofWhileParsingString)),
                Some(b'"') => {
                    let raw = &self.bytes[start..self.index - 1];
                    if output.is_empty() {
                        // Validate only at closing quote, as upstream does. In
                        // particular an earlier invalid UTF-8 byte must not
                        // hide a later invalid escape/control/EOF error.
                        let text = std::str::from_utf8(raw)
                            .map_err(|_| self.error(Code::InvalidUnicodeCodePoint))?;
                        return alloc::try_copy_str(text, self.cx, AllocationSite::String)
                            .map_err(|e| self.allocation(e));
                    }
                    self.append(&mut output, raw)?;
                    // Reuses the Vec allocation; no shrink/copy/error boxing.
                    return String::from_utf8(output)
                        .map_err(|_| self.error(Code::InvalidUnicodeCodePoint));
                }
                Some(b'\\') => {
                    self.append(&mut output, &self.bytes[start..self.index - 1])?;
                    self.escape(&mut output)?;
                    start = self.index;
                }
                Some(_) => return Err(self.error(Code::ControlCharacterWhileParsingString)),
            }
        }
    }
    fn escape(&mut self, output: &mut Vec<u8>) -> Result<()> {
        let byte = match self.next() {
            None => return Err(self.error(Code::EofWhileParsingString)),
            Some(b'"') => b'"', Some(b'\\') => b'\\', Some(b'/') => b'/',
            Some(b'b') => 8, Some(b'f') => 12, Some(b'n') => b'\n',
            Some(b'r') => b'\r', Some(b't') => b'\t',
            Some(b'u') => {
                let first = self.hex()?;
                if (0xdc00..=0xdfff).contains(&first) {
                    return Err(self.error(Code::LoneLeadingSurrogateInHexEscape));
                }
                let scalar = if (0xd800..=0xdbff).contains(&first) {
                    for expected in [b'\\', b'u'] {
                        match self.next() {
                            None => return Err(self.error(Code::EofWhileParsingString)),
                            Some(b) if b != expected => return Err(self.error(Code::UnexpectedEndOfHexEscape)),
                            _ => (),
                        }
                    }
                    let second = self.hex()?;
                    if !(0xdc00..=0xdfff).contains(&second) {
                        return Err(self.error(Code::LoneLeadingSurrogateInHexEscape));
                    }
                    (((first as u32 - 0xd800) << 10) | (second as u32 - 0xdc00)) + 0x10000
                } else { first as u32 };
                // Safe conversion; scalar is proved outside surrogate ranges.
                let ch = char::from_u32(scalar)
                    .ok_or_else(|| self.error(Code::InvalidUnicodeCodePoint))?;
                let mut buffer = [0u8; 4];
                return self.append(output, ch.encode_utf8(&mut buffer).as_bytes());
            }
            Some(_) => return Err(self.error(Code::InvalidEscape)),
        };
        self.append(output, &[byte])
    }
    fn hex(&mut self) -> Result<u16> {
        if self.bytes.len() - self.index < 4 {
            self.index = self.bytes.len();
            return Err(self.error(Code::EofWhileParsingString));
        }
        let start = self.index;
        self.index += 4; // SliceRead consumes all four even if one is non-hex.
        let mut value = 0u16;
        for &b in &self.bytes[start..self.index] {
            let digit = match b {
                b'0'..=b'9' => b - b'0', b'a'..=b'f' => b - b'a' + 10,
                b'A'..=b'F' => b - b'A' + 10,
                _ => return Err(self.error(Code::InvalidEscape)),
            };
            value = value * 16 + digit as u16;
        }
        Ok(value)
    }
    fn number_value(&mut self, positive: bool) -> Result<Value> {
        let number = self.integer(positive)?;
        if self.policy == Policy::Store && matches!(number, Number::F64(_)) {
            // visit_f64 errors are positioned immediately after number parsing,
            // unlike a duplicate error produced by the enclosing map visitor.
            return Err(self.at(Cause::StoreFloatNotAllowed, self.index));
        }
        Ok(Value::Number(number))
    }
    fn integer(&mut self, positive: bool) -> Result<Number> {
        match self.next() {
            None => Err(self.error(Code::EofWhileParsingValue)),
            Some(b'0') => match self.peek_or_null() {
                b'0'..=b'9' => Err(self.peek_error(Code::InvalidNumber)),
                _ => self.number(positive, 0),
            },
            Some(c @ b'1'..=b'9') => {
                let mut significand = (c - b'0') as u64;
                while let c @ b'0'..=b'9' = self.peek_or_null() {
                    let digit = (c - b'0') as u64;
                    if significand > (u64::MAX - digit) / 10 {
                        return self.long_integer(positive, significand).map(Number::F64);
                    }
                    self.index += 1;
                    significand = significand * 10 + digit;
                }
                self.number(positive, significand)
            }
            Some(_) => Err(self.error(Code::InvalidNumber)),
        }
    }
    fn number(&mut self, positive: bool, significand: u64) -> Result<Number> {
        Ok(match self.peek_or_null() {
            b'.' => Number::F64(self.decimal(positive, significand, 0)?),
            b'e' | b'E' => Number::F64(self.exponent(positive, significand, 0)?),
            _ if positive => Number::U64(significand),
            _ => {
                let neg = (significand as i64).wrapping_neg();
                if neg >= 0 { Number::F64(-(significand as f64)) }
                else { Number::I64(neg) }
            }
        })
    }
    fn decimal(&mut self, positive: bool, mut significand: u64, before: i32) -> Result<f64> {
        self.index += 1;
        let mut after = 0;
        while let c @ b'0'..=b'9' = self.peek_or_null() {
            let digit = (c - b'0') as u64;
            if significand > (u64::MAX - digit) / 10 {
                return self.decimal_overflow(positive, significand, before + after);
            }
            self.index += 1;
            significand = significand * 10 + digit;
            after -= 1;
        }
        if after == 0 {
            return Err(self.peek_error(if self.peek().is_some() {
                Code::InvalidNumber
            } else { Code::EofWhileParsingValue }));
        }
        let exponent = before + after;
        match self.peek_or_null() {
            b'e' | b'E' => self.exponent(positive, significand, exponent),
            _ => self.f64_from_parts(positive, significand, exponent),
        }
    }
    fn exponent(&mut self, positive: bool, significand: u64, starting: i32) -> Result<f64> {
        self.index += 1;
        let positive_exp = match self.peek_or_null() {
            b'+' => { self.index += 1; true }
            b'-' => { self.index += 1; false }
            _ => true,
        };
        let mut exp = match self.next() {
            Some(c @ b'0'..=b'9') => (c - b'0') as i32,
            None => return Err(self.error(Code::EofWhileParsingValue)),
            Some(_) => return Err(self.error(Code::InvalidNumber)),
        };
        while let c @ b'0'..=b'9' = self.peek_or_null() {
            self.index += 1;
            let digit = (c - b'0') as i32;
            if exp > (i32::MAX - digit) / 10 {
                if significand != 0 && positive_exp {
                    return Err(self.error(Code::NumberOutOfRange));
                }
                while matches!(self.peek_or_null(), b'0'..=b'9') { self.index += 1; }
                return Ok(if positive { 0.0 } else { -0.0 });
            }
            exp = exp * 10 + digit;
        }
        let final_exp = if positive_exp { starting.saturating_add(exp) }
            else { starting.saturating_sub(exp) };
        self.f64_from_parts(positive, significand, final_exp)
    }
    fn f64_from_parts(&self, positive: bool, significand: u64, mut exponent: i32) -> Result<f64> {
        let mut f = significand as f64;
        loop {
            match POW10.get(exponent.wrapping_abs() as usize) {
                Some(&pow) => {
                    if exponent >= 0 {
                        f *= pow;
                        if f.is_infinite() { return Err(self.error(Code::NumberOutOfRange)); }
                    } else { f /= pow; }
                    break;
                }
                None => {
                    if f == 0.0 { break; }
                    if exponent >= 0 { return Err(self.error(Code::NumberOutOfRange)); }
                    f /= 1e308;
                    exponent += 308;
                }
            }
        }
        Ok(if positive { f } else { -f })
    }
    fn long_integer(&mut self, positive: bool, significand: u64) -> Result<f64> {
        let mut exponent = 0;
        // Proposal has its 1 MiB cap; store callers prove their original
        // input bounds separately (see parse_store_json_bytes), below i32::MAX.
        loop {
            match self.peek_or_null() {
                b'0'..=b'9' => { self.index += 1; exponent += 1; }
                b'.' => return self.decimal(positive, significand, exponent),
                b'e' | b'E' => return self.exponent(positive, significand, exponent),
                _ => return self.f64_from_parts(positive, significand, exponent),
            }
        }
    }
    fn decimal_overflow(&mut self, positive: bool, significand: u64, exponent: i32) -> Result<f64> {
        // Keep upstream's truncated significand; do not round discarded digits.
        while matches!(self.peek_or_null(), b'0'..=b'9') { self.index += 1; }
        match self.peek_or_null() {
            b'e' | b'E' => self.exponent(positive, significand, exponent),
            _ => self.f64_from_parts(positive, significand, exponent),
        }
    }
}

static POW10: [f64; 309] = [
    1e000, 1e001, 1e002, 1e003, 1e004, 1e005, 1e006, 1e007, 1e008, 1e009, //
    1e010, 1e011, 1e012, 1e013, 1e014, 1e015, 1e016, 1e017, 1e018, 1e019, //
    1e020, 1e021, 1e022, 1e023, 1e024, 1e025, 1e026, 1e027, 1e028, 1e029, //
    1e030, 1e031, 1e032, 1e033, 1e034, 1e035, 1e036, 1e037, 1e038, 1e039, //
    1e040, 1e041, 1e042, 1e043, 1e044, 1e045, 1e046, 1e047, 1e048, 1e049, //
    1e050, 1e051, 1e052, 1e053, 1e054, 1e055, 1e056, 1e057, 1e058, 1e059, //
    1e060, 1e061, 1e062, 1e063, 1e064, 1e065, 1e066, 1e067, 1e068, 1e069, //
    1e070, 1e071, 1e072, 1e073, 1e074, 1e075, 1e076, 1e077, 1e078, 1e079, //
    1e080, 1e081, 1e082, 1e083, 1e084, 1e085, 1e086, 1e087, 1e088, 1e089, //
    1e090, 1e091, 1e092, 1e093, 1e094, 1e095, 1e096, 1e097, 1e098, 1e099, //
    1e100, 1e101, 1e102, 1e103, 1e104, 1e105, 1e106, 1e107, 1e108, 1e109, //
    1e110, 1e111, 1e112, 1e113, 1e114, 1e115, 1e116, 1e117, 1e118, 1e119, //
    1e120, 1e121, 1e122, 1e123, 1e124, 1e125, 1e126, 1e127, 1e128, 1e129, //
    1e130, 1e131, 1e132, 1e133, 1e134, 1e135, 1e136, 1e137, 1e138, 1e139, //
    1e140, 1e141, 1e142, 1e143, 1e144, 1e145, 1e146, 1e147, 1e148, 1e149, //
    1e150, 1e151, 1e152, 1e153, 1e154, 1e155, 1e156, 1e157, 1e158, 1e159, //
    1e160, 1e161, 1e162, 1e163, 1e164, 1e165, 1e166, 1e167, 1e168, 1e169, //
    1e170, 1e171, 1e172, 1e173, 1e174, 1e175, 1e176, 1e177, 1e178, 1e179, //
    1e180, 1e181, 1e182, 1e183, 1e184, 1e185, 1e186, 1e187, 1e188, 1e189, //
    1e190, 1e191, 1e192, 1e193, 1e194, 1e195, 1e196, 1e197, 1e198, 1e199, //
    1e200, 1e201, 1e202, 1e203, 1e204, 1e205, 1e206, 1e207, 1e208, 1e209, //
    1e210, 1e211, 1e212, 1e213, 1e214, 1e215, 1e216, 1e217, 1e218, 1e219, //
    1e220, 1e221, 1e222, 1e223, 1e224, 1e225, 1e226, 1e227, 1e228, 1e229, //
    1e230, 1e231, 1e232, 1e233, 1e234, 1e235, 1e236, 1e237, 1e238, 1e239, //
    1e240, 1e241, 1e242, 1e243, 1e244, 1e245, 1e246, 1e247, 1e248, 1e249, //
    1e250, 1e251, 1e252, 1e253, 1e254, 1e255, 1e256, 1e257, 1e258, 1e259, //
    1e260, 1e261, 1e262, 1e263, 1e264, 1e265, 1e266, 1e267, 1e268, 1e269, //
    1e270, 1e271, 1e272, 1e273, 1e274, 1e275, 1e276, 1e277, 1e278, 1e279, //
    1e280, 1e281, 1e282, 1e283, 1e284, 1e285, 1e286, 1e287, 1e288, 1e289, //
    1e290, 1e291, 1e292, 1e293, 1e294, 1e295, 1e296, 1e297, 1e298, 1e299, //
    1e300, 1e301, 1e302, 1e303, 1e304, 1e305, 1e306, 1e307, 1e308,
];
