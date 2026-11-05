//! Fixed-storage product digest and explicitly fallible owned hex rendering.
//! Unrouted foundation: native/work closure and caller funding are separate gates.
use crate::product_alloc::{self, AllocationContext, AllocationFailure, AllocationSite};
use crate::product_contract::IdentityDomain;
use ring::digest::{Context, SHA256};

const PREFIX: &[u8] = b"gitmanager.product.v1\0";
const MAX_FRAMED_BYTES: u64 = u64::MAX / 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DigestValue([u8;32]);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DigestError { InputTooLong, UnexpectedOutputLength }

fn quoted_label(domain: IdentityDomain) -> &'static [u8] {
    match domain {
        IdentityDomain::Source => b"\"source\"",
        IdentityDomain::Program => b"\"program\"",
        IdentityDomain::SemanticProgram => b"\"semantic_program\"",
        IdentityDomain::Data => b"\"data\"",
        IdentityDomain::Schema => b"\"schema\"",
        IdentityDomain::Input => b"\"input\"",
        IdentityDomain::Session => b"\"session\"",
        IdentityDomain::Observation => b"\"observation\"",
        IdentityDomain::Output => b"\"output\"",
        IdentityDomain::OutputBytes => b"\"output_bytes\"",
        IdentityDomain::Request => b"\"request\"",
        IdentityDomain::Scenario => b"\"scenario\"",
        IdentityDomain::Decision => b"\"decision\"",
        IdentityDomain::Evolution => b"\"evolution\"",
        IdentityDomain::Adoption => b"\"adoption\"",
        IdentityDomain::Evidence => b"\"evidence\"",
    }
}
/// The SHA-256 bit counter boundary, not an additional product/input cap.
pub fn checked_framed_len(domain: IdentityDomain, payload_len: u64) -> Result<u64,DigestError> {
    let overhead = PREFIX.len() as u64 + quoted_label(domain).len() as u64 + 1;
    let length = payload_len.checked_add(overhead).ok_or(DigestError::InputTooLong)?;
    if length > MAX_FRAMED_BYTES { return Err(DigestError::InputTooLong); }
    Ok(length)
}
fn copy_output(bytes: &[u8]) -> Result<DigestValue,DigestError> {
    if bytes.len() != 32 { return Err(DigestError::UnexpectedOutputLength); }
    let mut result = [0;32];
    result.copy_from_slice(bytes);
    Ok(DigestValue(result))
}
/// Checks before ring construction/CPU initialization; never retains the payload.
pub fn try_bytes_digest(domain: IdentityDomain, payload: &[u8]) -> Result<DigestValue,DigestError> {
    let length = u64::try_from(payload.len()).map_err(|_| DigestError::InputTooLong)?;
    checked_framed_len(domain,length)?;
    let mut context = Context::new(&SHA256);
    context.update(PREFIX);
    context.update(quoted_label(domain));
    context.update(&[0]);
    context.update(payload);
    copy_output(context.finish().as_ref())
}
impl DigestValue {
    pub fn as_bytes(&self) -> &[u8;32] { &self.0 }
    pub fn lower_hex_bytes(&self) -> [u8;64] {
        const HEX: &[u8;16] = b"0123456789abcdef";
        let mut output = [0;64];
        for (i,byte) in self.0.iter().copied().enumerate() {
            output[2*i] = HEX[(byte >> 4) as usize];
            output[2*i+1] = HEX[(byte & 15) as usize];
        }
        output
    }
    pub fn try_hex_string(&self, cx: &mut AllocationContext) -> Result<String,AllocationFailure> {
        let mut output = String::new();
        product_alloc::try_reserve_string(&mut output,64,cx,AllocationSite::String)?;
        for byte in self.lower_hex_bytes() { output.push(char::from(byte)); }
        Ok(output)
    }
    pub fn try_hex_vec(&self, cx: &mut AllocationContext) -> Result<Vec<u8>,AllocationFailure> {
        let mut output = Vec::new();
        product_alloc::try_reserve(&mut output,64,cx,AllocationSite::Bytes)?;
        output.extend_from_slice(&self.lower_hex_bytes());
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn static_domain_labels_are_exact_quoted_literals() {
        use IdentityDomain::*;
        for (domain,label) in [(Source,b"\"source\"".as_slice()),(Program,b"\"program\""),
            (SemanticProgram,b"\"semantic_program\""),(Data,b"\"data\""),(Schema,b"\"schema\""),
            (Input,b"\"input\""),(Session,b"\"session\""),(Observation,b"\"observation\""),
            (Output,b"\"output\""),(OutputBytes,b"\"output_bytes\""),(Request,b"\"request\""),
            (Scenario,b"\"scenario\""),(Decision,b"\"decision\""),(Evolution,b"\"evolution\""),
            (Adoption,b"\"adoption\""),(Evidence,b"\"evidence\"")] {
            assert_eq!(quoted_label(domain),label);
        }
    }
    #[test]
    fn fixed_hex_256_patterns_cover_all_bytes_and_mixed_nibbles() {
        for n in 0..256 {
            let raw=match n {
                253=>[255;32],
                254=>std::array::from_fn(|i|[253,254,15,240][i%4]),
                255=>std::array::from_fn(|i|i as u8),
                _=>[n as u8;32],
            };
            let value=DigestValue(raw);let hex=value.lower_hex_bytes();
            let expected:String=raw.iter().map(|b|format!("{b:02x}")).collect();
            assert_eq!(hex.as_slice(),expected.as_bytes());assert_eq!(hex.len(),64);
            assert!(hex.iter().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(b)));
            assert_eq!(value.as_bytes(),&raw);assert_eq!(value.clone(),value);
            let mut cx=AllocationContext::new();
            assert_eq!(value.try_hex_string(&mut cx).unwrap(),expected);
            assert_eq!(value.try_hex_vec(&mut cx).unwrap(),hex);assert_eq!(cx.growth_requests(),2);
        }
    }
    #[test]
    fn fixed_output_length_guard_rejects_without_partial_value() {
        let bytes=[0x5a;33];
        for n in [0,1,31,33] { assert_eq!(copy_output(&bytes[..n]),Err(DigestError::UnexpectedOutputLength)); }
        assert_eq!(copy_output(&bytes[..32]).unwrap().as_bytes(),&[0x5a;32]);
    }
}
