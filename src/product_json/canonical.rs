//! Borrowed canonical encoding, not a generic Serialize or whole-request API.
//!
//! Qualified only with pinned serde_json1.0.150 default/std WITHOUT
//! arbitrary_precision: concrete Number constructors/Display then use inline
//! N and itoa/zmij stack buffers. Every build/routing qualification must verify
//! dependency features, source and artifact identities; a local cfg cannot
//! enforce another crate's features. No generic formatter closure is claimed.
//!
//! Two walks cost O(nodes + key/string bytes + encoded bytes), with O(depth)
//! geometric borrowed-frame storage and one output reservation. All prepass,
//! number formatting, reserve checks, initialization, emission and cleanup
//! require predecessor funding. The measured length creates no budget/credit;
//! callers must fund any continuation from their ORIGINAL meter. No source
//! pricing, smaller product limit, digest, App conversion or routing is added.
use super::{Number, Value};
use crate::product_alloc::{ordered::OrderedMap, try_reserve, AllocationContext,
    AllocationFailure, AllocationSite};
use std::{fmt::{self, Write}, mem};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Invariant { NumberFormat, WorkspaceCapacity, OutputBounds, OutputLength, Traversal }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EncodeError {
    Allocation(AllocationFailure),
    EncodedLengthOverflow,
    Invariant(Invariant),
}
impl From<AllocationFailure> for EncodeError {
    fn from(failure: AllocationFailure) -> Self { Self::Allocation(failure) }
}
impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Allocation(error) => fmt::Display::fmt(error,f),
            Self::EncodedLengthOverflow => f.write_str("canonical JSON encoded length overflow"),
            Self::Invariant(cause) => f.write_str(match cause {
                Invariant::NumberFormat => "canonical JSON numeric format invariant",
                Invariant::WorkspaceCapacity => "canonical JSON workspace capacity invariant",
                Invariant::OutputBounds => "canonical JSON output bounds invariant",
                Invariant::OutputLength => "canonical JSON output length invariant",
                Invariant::Traversal => "canonical JSON traversal invariant",
            }),
        }
    }
}
impl std::error::Error for EncodeError {}

enum Frame<'a> {
    Array(&'a [Value], usize),
    Object(&'a OrderedMap<String, Value>, usize),
}
enum Mode<'a> { Measure, Emit(&'a mut [u8]) }
pub(crate) struct Sink<'a> { mode: Mode<'a>, cursor: usize }
impl<'a> Sink<'a> {
    pub(crate) fn measure() -> Self { Self{mode:Mode::Measure,cursor:0} }
    pub(crate) fn emit(output: &'a mut [u8]) -> Self { Self{mode:Mode::Emit(output),cursor:0} }
    pub(crate) fn position(&self) -> usize { self.cursor }
    pub(crate) fn measuring(&self) -> bool { matches!(&self.mode,Mode::Measure) }
    pub(crate) fn append(&mut self, bytes: &[u8]) -> Result<(), EncodeError> {
        let end=self.cursor.checked_add(bytes.len()).ok_or(if self.measuring() {
            EncodeError::EncodedLengthOverflow
        } else { EncodeError::Invariant(Invariant::OutputBounds) })?;
        if let Mode::Emit(output)=&mut self.mode {
            output.get_mut(self.cursor..end)
                .ok_or(EncodeError::Invariant(Invariant::OutputBounds))?.copy_from_slice(bytes);
        }
        self.cursor=end;
        Ok(())
    }
    pub(crate) fn finish(&self, expected: usize) -> Result<(), EncodeError> {
        if self.cursor!=expected { return Err(EncodeError::Invariant(Invariant::OutputLength)); }
        Ok(())
    }
}
struct NumericScratch { bytes: [u8;24], len: usize }
impl Write for NumericScratch {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let end=self.len.checked_add(text.len()).ok_or(fmt::Error)?;
        self.bytes.get_mut(self.len..end).ok_or(fmt::Error)?.copy_from_slice(text.as_bytes());
        self.len=end;
        Ok(())
    }
}
pub(crate) fn number(value: &Number, sink: &mut Sink<'_>) -> Result<(), EncodeError> {
    let number=match value {
        Number::U64(n)=>serde_json::Number::from(*n),
        Number::I64(n)=>serde_json::Number::from(*n),
        Number::F64(n) if !n.is_finite()=>return sink.append(b"null"),
        Number::F64(n)=>serde_json::Number::from_f64(*n)
            .ok_or(EncodeError::Invariant(Invariant::NumberFormat))?,
    };
    let mut scratch=NumericScratch{bytes:[0;24],len:0};
    // Only this inspected concrete Number Display, no user callback/heap sink.
    write!(&mut scratch,"{number}").map_err(|_|EncodeError::Invariant(Invariant::NumberFormat))?;
    sink.append(&scratch.bytes[..scratch.len])
}
pub(crate) fn string(value: &str, sink: &mut Sink<'_>) -> Result<(), EncodeError> {
    const HEX:&[u8;16]=b"0123456789abcdef";
    sink.append(b"\"")?;
    let bytes=value.as_bytes(); let mut start=0;
    for (index,&byte) in bytes.iter().enumerate() {
        let escape:&[u8]=match byte {
            b'"'=>b"\\\"",b'\\'=>b"\\\\",b'\x08'=>b"\\b",b'\t'=>b"\\t",
            b'\n'=>b"\\n",b'\x0c'=>b"\\f",b'\r'=>b"\\r",
            0..=31=>b"",_=>continue,
        };
        sink.append(&bytes[start..index])?;
        if escape.is_empty() { sink.append(&[b'\\',b'u',b'0',b'0',HEX[(byte>>4) as usize],HEX[(byte&15) as usize]])?; }
        else { sink.append(escape)?; }
        // index is strictly smaller than this real slice's length.
        start=index+1;
    }
    sink.append(&bytes[start..])?;
    sink.append(b"\"")
}
// Clamp doubling, not the required next slot. At the actual layout maximum,
// let try_reserve report its ordinary recorded CapacityOverflow, not a new cap.
fn growth_target(capacity: usize, required: usize, layout_max: usize) -> usize {
    required.max(capacity.saturating_mul(2).min(layout_max))
}
fn push_frame<'a>(stack: &mut Vec<Frame<'a>>, frame: Frame<'a>, measure: bool,
    cx: &mut AllocationContext) -> Result<(), EncodeError> {
    if stack.len()==stack.capacity() && measure {
        let required=stack.len().checked_add(1).ok_or(EncodeError::Invariant(Invariant::Traversal))?;
        let target=growth_target(stack.capacity(),required,isize::MAX as usize/mem::size_of::<Frame<'a>>());
        try_reserve(stack,target-stack.len(),cx,AllocationSite::Vector)?;
    }
    if stack.len()>=stack.capacity() { return Err(EncodeError::Invariant(Invariant::WorkspaceCapacity)); }
    stack.push(frame); // Checked above: never silently grow, even during emission.
    Ok(())
}
fn walk<'a>(root: &'a Value, stack: &mut Vec<Frame<'a>>, sink: &mut Sink<'_>,
    cx: &mut AllocationContext) -> Result<(), EncodeError> {
    if !stack.is_empty() { return Err(EncodeError::Invariant(Invariant::Traversal)); }
    let mut current=Some(root);
    loop {
        if let Some(value)=current.take() {
            match value {
                Value::Null=>sink.append(b"null")?,
                Value::Bool(true)=>sink.append(b"true")?,Value::Bool(false)=>sink.append(b"false")?,
                Value::Number(n)=>number(n,sink)?,Value::String(s)=>string(s,sink)?,
                Value::Array(a) if a.is_empty()=>sink.append(b"[]")?,
                Value::Object(m) if m.is_empty()=>sink.append(b"{}")?,
                Value::Array(a)=>{sink.append(b"[")?;push_frame(stack,Frame::Array(a,0),sink.measuring(),cx)?;}
                Value::Object(m)=>{sink.append(b"{")?;push_frame(stack,Frame::Object(m,0),sink.measuring(),cx)?;}
            }
        }
        loop {
            let Some(frame)=stack.last_mut() else { return Ok(()); };
            let close=match frame {
                Frame::Array(values,next)=>{
                    let values=*values;
                    if let Some(value)=values.get(*next) {
                        if *next>0 {sink.append(b",")?;}
                        *next=next.checked_add(1).ok_or(EncodeError::Invariant(Invariant::Traversal))?;
                        current=Some(value);break;
                    }
                    b"]"
                }
                Frame::Object(values,next)=>{
                    let values=*values;
                    if let Some((key,value))=values.get_index(*next) {
                        if *next>0 {sink.append(b",")?;}
                        string(key,sink)?;sink.append(b":")?;
                        *next=next.checked_add(1).ok_or(EncodeError::Invariant(Invariant::Traversal))?;
                        current=Some(value);break;
                    }
                    b"}"
                }
            };
            sink.append(close)?;stack.pop();
        }
    }
}
pub(crate) fn reserve_output(length: usize, cx: &mut AllocationContext) -> Result<Vec<u8>, EncodeError> {
    let mut output=Vec::new();
    try_reserve(&mut output,length,cx,AllocationSite::Bytes)?;
    if output.capacity()<length { return Err(EncodeError::Invariant(Invariant::OutputBounds)); }
    output.resize(length,0); // Within reserved capacity; O(length) initialization is explicit.
    Ok(output)
}
/// Exact compact, lexically keyed bytes of the closed Value, not RFC8785.
/// Manually constructed NaN/infinite F64 encodes null, like original generic
/// Serde f64. Nonrecursive traversal borrows input throughout; source ownership
/// and eventual Drop remain the caller's obligation (deep owned Drop is NOT
/// fixed). Failure drops only local borrowed frames and byte storage. Nothing
/// is published, retried, reformatted into heap storage or routed to old Serde.
pub fn try_canonical_bytes(value: &Value, cx: &mut AllocationContext) -> Result<Vec<u8>, EncodeError> {
    let mut stack=Vec::new();
    let mut measure=Sink{mode:Mode::Measure,cursor:0};
    walk(value,&mut stack,&mut measure,cx)?;
    let length=measure.cursor;
    let mut output=reserve_output(length,cx)?;
    let mut emit=Sink{mode:Mode::Emit(&mut output),cursor:0};
    walk(value,&mut stack,&mut emit,cx)?;
    emit.finish(length)?;
    if !stack.is_empty() { return Err(EncodeError::Invariant(Invariant::Traversal)); }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::product_alloc::AllocationFailureKind;
    use std::num::NonZeroUsize;
    #[test]
    fn checked_length_layout_and_geometric_boundaries() {
        let mut sink=Sink{mode:Mode::Measure,cursor:usize::MAX};
        assert_eq!(sink.append(b"x"),Err(EncodeError::EncodedLengthOverflow));
        assert_eq!(sink.cursor,usize::MAX);
        for (old,required,max,expected) in [(0,1,7,1),(1,2,7,2),(4,5,7,7),(7,8,7,8),
            (usize::MAX/2+1,usize::MAX/2+2,usize::MAX,usize::MAX)] {
            assert_eq!(growth_target(old,required,max),expected);
        }
        let mut cx=AllocationContext::new();
        let err=reserve_output(isize::MAX as usize+1,&mut cx).unwrap_err();
        assert_eq!(err,EncodeError::Allocation(AllocationFailure{
            kind:AllocationFailureKind::CapacityOverflow,site:AllocationSite::Bytes,
            requested_bytes:Some(isize::MAX as usize+1)}));
        assert_eq!(cx.growth_requests(),0);
        assert_eq!(cx.first_failure(),match err {EncodeError::Allocation(f)=>Some(f),_=>None});
    }
    #[test]
    fn scratch_cursor_and_workspace_invariants_fail_without_growth() {
        let mut fixed=NumericScratch{bytes:[0;24],len:23};
        assert!(fmt::Write::write_str(&mut fixed,"xx").is_err()); assert_eq!(fixed.len,23);
        fixed.len=usize::MAX; assert!(fmt::Write::write_str(&mut fixed,"x").is_err());
        let mut bytes=[9;1]; let mut sink=Sink{mode:Mode::Emit(&mut bytes),cursor:0};
        assert_eq!(sink.append(b"xx"),Err(EncodeError::Invariant(Invariant::OutputBounds)));
        assert_eq!(sink.finish(1),Err(EncodeError::Invariant(Invariant::OutputLength)));
        sink.cursor=usize::MAX;
        assert_eq!(sink.append(b"x"),Err(EncodeError::Invariant(Invariant::OutputBounds)));
        let mut stack=Vec::new(); let mut cx=AllocationContext::new();
        assert_eq!(push_frame(&mut stack,Frame::Array(&[],0),false,&mut cx),
            Err(EncodeError::Invariant(Invariant::WorkspaceCapacity)));
        assert_eq!(cx.growth_requests(),0); assert!(stack.is_empty());
        assert_eq!(bytes,[9]);
    }
    #[test]
    fn measured_storage_reused_with_zero_emission_requests() {
        let value=Value::Array(vec![Value::Array(vec![Value::Null])]);
        let mut stack=Vec::new(); let mut cx=AllocationContext::new();
        let mut measure=Sink{mode:Mode::Measure,cursor:0};
        walk(&value,&mut stack,&mut measure,&mut cx).unwrap();
        assert_eq!(measure.cursor,8); assert!(stack.is_empty());
        let capacity=stack.capacity(); let before=cx.growth_requests();
        let mut output=reserve_output(measure.cursor,&mut cx).unwrap();
        assert_eq!(cx.growth_requests(),before+1);
        let before=cx.growth_requests();
        let mut emit=Sink{mode:Mode::Emit(&mut output),cursor:0};
        walk(&value,&mut stack,&mut emit,&mut cx).unwrap(); emit.finish(8).unwrap();
        assert!(stack.is_empty()); assert_eq!(stack.capacity(),capacity);
        assert_eq!(cx.growth_requests(),before); assert_eq!(output,b"[[null]]");
    }
    #[test]
    fn closed_workspace_and_output_request_bytes_are_exact() {
        let value=Value::Array(vec![Value::Null]);
        for (nth,site,bytes) in [(1,AllocationSite::Vector,std::mem::size_of::<Frame<'_>>()),
            (2,AllocationSite::Bytes,6)] {
            let mut cx=AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(),true);
            assert_eq!(try_canonical_bytes(&value,&mut cx),Err(EncodeError::Allocation(
                AllocationFailure{kind:AllocationFailureKind::Injected,site,requested_bytes:Some(bytes)})));
            assert_eq!(cx.growth_requests(),nth);
            assert_eq!(try_canonical_bytes(&value,&mut AllocationContext::new()).unwrap(),b"[null]");
        }
    }
}
