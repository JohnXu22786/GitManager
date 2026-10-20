//! Same-call guarded native source admission. Only the three semantic scalars
//! survive the attempt; neither a receipt nor a cached validation can escape.
use super::*;
use std::cell::Ref;

type UnitResult = Result<()>;
type BoolResult = Result<bool>;
type InputWorkError = bounded_input::InputWorkError<ContractError>;
type LexicalResult = std::result::Result<bounded_input::LexicalEnvelope, InputWorkError>;
type EntityIter = std::slice::Iter<'static, EntityDefinition>;
type FieldIter = std::slice::Iter<'static, FieldDefinition>;
type Context = (&'static CapturedProgram, Audit<'static>);
struct SemanticScalars {
    total_nodes: usize,
    action_prefix_nodes: usize,
    action_count: usize,
}
struct SameCaptureOutputBound {
    bytes: u64,
    nodes: u64,
    pairs: u64,
    native_base: u64,
    escaped: u64,
    digest_copies: u64,
}
type Hit = Option<(SemanticScalars, SameCaptureOutputBound)>;
// Const evaluation rejects every overflowing sum/product. This checked cast is
// also evaluated at compile time, never an input-dependent packet calculation.
const fn units(n: usize) -> u64 {
    assert!(usize::BITS <= u64::BITS || n <= u64::MAX as usize);
    n as u64
}
const RAW_OUTPUT_STACK: u64 = units(OUTPUT_STACK_ROOT);
const COMMON_SLOTS: usize = size_of::<&CapturedProgram>()
    + 2 * size_of::<&AppDefinition>()
    + 4 * A
    + 2 * size_of::<&[u8]>()
    + 4 * size_of::<UnitResult>()
    + 2 * size_of::<BoolResult>()
    + A
    + size_of::<&mut Audit>()
    + size_of::<bounded_input::LexicalEnvelope>()
    + size_of::<LexicalResult>()
    + size_of::<InputWorkError>()
    + size_of::<bounded_input::InputError>()
    + 2 * size_of::<Result<capture_validation::CaptureFacts>>()
    + size_of::<capture_validation::CaptureFacts>()
    + size_of::<SemanticScalars>()
    + size_of::<SameCaptureOutputBound>()
    + 2 * size_of::<Hit>()
    + size_of::<Shape>()
    + size_of::<Result<Shape>>()
    + size_of::<Vec<u8>>()
    + size_of::<serde_json::Result<Vec<u8>>>()
    + size_of::<serde_json::Error>()
    + 2 * size_of::<ContractError>()
    + size_of::<Option<&WorkState>>()
    + size_of::<&WorkState>()
    + size_of::<Ref<'static, Option<WorkFailure>>>()
    + size_of::<bool>()
    + 2 * size_of::<&SameCaptureOutputBound>()
    + size_of::<&SemanticScalars>()
    + 8 * P
    + 8 * size_of::<bool>()
    + 8 * size_of::<Option<u64>>();
pub(in super::super::super) const COMMON: u64 = units(maximum!(4096, 768 + 2 * COMMON_SLOTS));
const HEAD_SLOTS: usize = size_of::<&AppDefinition>()
    + size_of::<&Vec<EntityDefinition>>()
    + size_of::<&[EntityDefinition]>()
    + 2 * A
    + 2 * size_of::<BoolResult>()
    + 2 * size_of::<UnitResult>()
    + size_of::<EntityIter>()
    + size_of::<FieldIter>()
    + size_of::<Option<&EntityDefinition>>()
    + size_of::<Option<&FieldDefinition>>()
    + size_of::<&EntityDefinition>()
    + size_of::<&FieldDefinition>()
    + 2 * size_of::<&Type>()
    + size_of::<&Box<Type>>()
    + 2 * P
    + 4 * size_of::<bool>();
const HEAD: u64 = units(maximum!(2048, 128 + 2 * HEAD_SLOTS));
const ENTITY_SLOTS: usize = 2 * A
    + size_of::<UnitResult>()
    + size_of::<&mut EntityIter>()
    + size_of::<Option<&EntityDefinition>>()
    + size_of::<&EntityDefinition>()
    + size_of::<&Vec<FieldDefinition>>()
    + size_of::<&[FieldDefinition]>()
    + P
    + size_of::<FieldIter>();
const ENTITY: u64 = units(maximum!(256, 40 + 2 * ENTITY_SLOTS));
const FIELD_SLOTS: usize = 2 * A
    + size_of::<UnitResult>()
    + size_of::<&mut FieldIter>()
    + size_of::<Option<&FieldDefinition>>()
    + size_of::<&FieldDefinition>()
    + size_of::<&Type>()
    + P;
const FIELD: u64 = units(maximum!(256, 40 + 2 * FIELD_SLOTS));
const TYPE_SLOTS: usize =
    2 * A + size_of::<UnitResult>() + 2 * size_of::<&Type>() + size_of::<&Box<Type>>() + P;
const TYPE_STEP: u64 = units(maximum!(128, 40 + 2 * TYPE_SLOTS));
const SOURCE_DELTA_SLOTS: usize = 3 * size_of::<Context>()
    + size_of::<&mut Context>()
    + 2 * size_of::<Result<Hit>>()
    + 2 * size_of::<Hit>()
    + 2 * size_of::<Validator>()
    + 4 * P
    + 4 * size_of::<UnitResult>()
    + 2 * size_of::<ContractError>()
    + 2 * size_of::<&AppDefinition>()
    + size_of::<&Vec<u8>>()
    + size_of::<&[u8]>()
    + 4 * A
    + 2 * size_of::<bool>()
    + 2 * size_of::<u64>()
    + size_of::<Result<u64>>()
    + 2 * P
    + size_of::<&Shape>()
    + size_of::<&[u8]>()
    + 2 * size_of::<bool>();
pub(in super::super::super) const SOURCE_DELTA_ROOT: u64 =
    units(maximum!(2048, 128 + 2 * SOURCE_DELTA_SLOTS));
type NativeValueResult = serde_json::Result<JsonValue>;
type NativeMap = <serde_json::value::Serializer as serde::Serializer>::SerializeMap;
type NativeSeq = <serde_json::value::Serializer as serde::Serializer>::SerializeSeq;
const VALUE_ITER: usize = maximum!(
    RMAP,
    SET,
    size_of::<std::slice::Iter<'static, Expr>>(),
    size_of::<std::slice::Iter<'static, Id>>()
);
const VALUE_FRAME: usize = maximum!(
    1024,
    128 + 24 * P
        + 2 * J
        + 2 * size_of::<NativeValueResult>()
        + 2 * size_of::<NativeMap>()
        + 2 * size_of::<NativeSeq>()
        + VALUE_ITER
        + 4 * size_of::<UnitResult>()
);
const MATERIAL_STACK: u64 = units(2 * 74 * OUTPUT_FRAME + 2 * 74 * VALUE_FRAME);
const MATERIAL_ROOT_SLOTS: usize = 2 * size_of::<&CapturedProgram>()
    + 4 * A
    + 2 * size_of::<SemanticScalars>()
    + 2 * size_of::<Shape>()
    + 2 * size_of::<Result<Shape>>()
    + 3 * J
    + 2 * size_of::<NativeValueResult>()
    + 2 * size_of::<Result<JsonValue>>()
    + 4 * size_of::<UnitResult>()
    + 2 * size_of::<serde_json::Error>()
    + 4 * size_of::<&mut JsonValue>()
    + 4 * P
    + 4 * size_of::<bool>();
const MATERIAL_ROOT: u64 = units(maximum!(4096, 768 + 2 * MATERIAL_ROOT_SLOTS));

fn preflight(app: &AppDefinition, a: Audit<'_>) -> Result<bool> {
    if app.entities.len() > MAX_ITEMS {
        return Ok(false);
    }
    let mut entities = app.entities.iter();
    loop {
        #[cfg(test)]
        controls::event("entity-next");
        a.pay(ENTITY)?;
        let Some(entity) = entities.next() else {
            return Ok(true);
        };
        let field_slice = entity.fields.as_slice();
        if field_slice.len() > MAX_ITEMS {
            return Ok(false);
        }
        let mut fields = field_slice.iter();
        loop {
            #[cfg(test)]
            controls::event("field-next");
            a.pay(FIELD)?;
            let Some(field) = fields.next() else {
                break;
            };
            let mut current = &field.value_type;
            let mut depth = 0usize;
            loop {
                #[cfg(test)]
                controls::event("type-next");
                a.pay(TYPE_STEP)?;
                match current {
                    Type::List { item } | Type::Optional { item } => {
                        if depth == MAX_TYPE_DEPTH {
                            return Ok(false);
                        }
                        current = item.as_ref();
                        depth += 1;
                    }
                    Type::Boolean
                    | Type::Integer
                    | Type::Text
                    | Type::Date
                    | Type::Reference { .. } => break,
                }
            }
        }
    }
}

pub(in super::super::super) fn validate_guarded_native_capture(
    capture: &CapturedProgram,
    a: Audit<'_>,
) -> Result<capture_validation::CaptureFacts> {
    // COMMON and SOURCE_DELTA_ROOT were paid inline by the original entry.
    let context = (capture, a);
    let attempt = move || -> Result<Hit> {
        let (capture, a) = context;
        let raw = capture.source_bytes.as_slice();
        let mut lexical_callback = move |n: u64| a.pay(n);
        #[cfg(test)]
        assert_eq!(std::mem::size_of_val(&lexical_callback), size_of::<Audit>());
        let lexical = match bounded_input::lexical_envelope_with_work(raw, &mut lexical_callback) {
            Ok(value) => value,
            Err(InputWorkError::Resource(error)) => return Err(error),
            Err(InputWorkError::ArithmeticOverflow) => return Err(a.overflow()),
            Err(InputWorkError::Input(error)) => {
                drop(error);
                return Ok(None);
            }
        };
        if lexical.container_depth > bounded_input::MAX_NESTING_DEPTH {
            return Ok(None);
        }
        drop(lexical);
        drop(lexical_callback);
        #[cfg(test)]
        controls::event("preflight-head");
        a.pay(HEAD)?;
        if !preflight(&capture.program, a)? {
            return Ok(None);
        }
        let app = &capture.program;
        #[cfg(test)]
        controls::event("semantic");
        validate_app_semantics!(app, a, validator, action_prefix_nodes);
        a.pay(0)?; // Removed count entry opportunity, not a count traversal.
        #[cfg(test)]
        controls::event("raw-shape");
        a.pay(0)?;
        a.pay(96)?;
        a.pay(RAW_OUTPUT_STACK)?;
        let mut shape = Shape::new(a)?;
        a.pay(1)?;
        shape.canonical = false;
        a.pay(1)?;
        shape.simple_app = true;
        app.serialize(&mut shape)?;
        #[cfg(test)]
        controls::event("match-length");
        a.pay(128)?;
        let raw_length = a.usize(raw.len())?;
        #[cfg(test)]
        if controls::fault("raw-length-overflow") {
            return Err(a.overflow());
        }
        if shape.bytes != raw_length {
            #[cfg(test)]
            controls::event("length-miss");
            return Ok(None);
        }
        let bound = SameCaptureOutputBound {
            bytes: shape.bytes,
            nodes: shape.nodes,
            pairs: shape.pairs,
            native_base: shape.native,
            escaped: shape.escaped,
            digest_copies: shape.digest_copies,
        };
        #[cfg(test)]
        controls::event("raw-reserve");
        #[cfg(test)]
        if controls::fault("raw-native-overflow") {
            shape.native = u64::MAX;
        }
        shape.reserve_output()?;
        #[cfg(test)]
        controls::event("raw-native");
        let native = serde_json::to_vec(app);
        #[cfg(test)]
        let native = controls::native_result(native, "raw-error");
        let bytes = match native {
            Ok(bytes) => bytes,
            Err(error) => {
                #[cfg(test)]
                controls::event("raw-native-error");
                drop(error);
                return Ok(None);
            }
        };
        #[cfg(test)]
        controls::event("raw-native-ok");
        a.pay(0)?;
        #[cfg(test)]
        controls::event("wire-tail");
        a.pay(128)?;
        let wire_len = bytes.len();
        a.pay(0)?;
        a.pay(3)?;
        a.require(wire_len <= MAX_WIRE_BYTES, "program exceeds byte budget")?;
        let scalars = SemanticScalars {
            total_nodes: validator.nodes,
            action_prefix_nodes,
            action_count: app.actions.len(),
        };
        #[cfg(test)]
        controls::event("equal-bytes");
        a.pay(3)?;
        let equal = if bytes.len() != raw.len() {
            false
        } else {
            a.scaled(raw.len(), 2, 0)?;
            bytes.as_slice() == raw
        };
        drop(bytes);
        drop(shape);
        drop(validator);
        #[cfg(test)]
        controls::event("raw-drop");
        if !equal {
            return Ok(None);
        }
        Ok(Some((scalars, bound)))
    };
    #[cfg(test)]
    assert!(std::mem::size_of_val(&attempt) <= size_of::<Context>());
    let attempted = attempt();
    let hit = match attempted {
        Ok(hit) => hit,
        Err(error) => {
            if a.resource_failed_prepaid() {
                return Err(error);
            }
            drop(error);
            None
        }
    };
    if let Some((scalars, bound)) = hit {
        drop(bound); // No source-size or earlier payment authority reaches output.
        #[cfg(test)]
        controls::event("material-root");
        a.pay(MATERIAL_ROOT)?;
        a.pay(1)?;
        let raw = capture_validation::bytes_digest_audited(
            IdentityDomain::Source,
            &capture.source_bytes,
            a,
        )?;
        a.require(
            a.eq_str(capture.artifact.raw_digest.as_str(), raw.as_str())?
                && a.eq_str(
                    capture.binding.source_digest.as_str(),
                    capture.artifact.raw_digest.as_str(),
                )?,
            "raw source digest mismatch",
        )?;
        a.pay(8 + 2 * size_of::<Audit>() as u64 + 4 * size_of::<bool>() as u64)?;
        #[cfg(test)]
        controls::event("material-shape");
        a.pay(0)?;
        a.pay(96)?;
        a.pay(MATERIAL_STACK)?;
        let mut shape = Shape::new(a)?;
        capture.program.serialize(&mut shape)?;
        #[cfg(test)]
        controls::event("material-errors");
        a.format(128)?;
        a.scaled(ERROR_WIDTH, 2, 5)?;
        #[cfg(test)]
        controls::event("material-native-reserve");
        #[cfg(test)]
        if controls::fault("material-native-overflow") {
            return Err(a.overflow());
        }
        a.pay(shape.native)?;
        #[cfg(test)]
        controls::event("material-native");
        let native = serde_json::to_value(&capture.program);
        #[cfg(test)]
        let native = controls::native_result(native, "value-error");
        let mut value = match native {
            Ok(value) => value,
            Err(error) => {
                #[cfg(test)]
                controls::event("material-native-error");
                return Err(ContractError(error.to_string()));
            }
        };
        #[cfg(test)]
        controls::materialized(&value, &capture.source_bytes);
        #[cfg(test)]
        controls::event("material-native-ok");
        a.pay(0)?;
        drop(shape);
        value_output_types!(PendingLength, BOUND_ROOT);
        const PENDING_ROOT: u64 = units(BOUND_ROOT);
        #[cfg(test)]
        controls::event("pending");
        a.pay(128)?;
        a.pay(PENDING_ROOT)?;
        let pending = PendingLength {
            capture,
            value: &mut value,
            a,
            total_nodes: scalars.total_nodes,
            action_prefix_nodes: scalars.action_prefix_nodes,
            action_count: scalars.action_count,
        };
        let (bytes, program_output, facts) = pending.first_value_output()?;
        let program = capture_validation::bytes_digest_audited(IdentityDomain::Program, &bytes, a)?;
        let equal = a.eq_str(capture.artifact.program_digest.as_str(), program.as_str())?;
        let equal = if equal {
            a.pay(8 + 2 * size_of::<Audit>() as u64 + 4 * size_of::<bool>() as u64)?;
            let semantic_output = program_output.clear_labels()?;
            let semantic_bytes = semantic_output.write()?;
            let digest = capture_validation::bytes_digest_audited(
                IdentityDomain::SemanticProgram,
                &semantic_bytes,
                a,
            )?;
            a.eq_str(capture.artifact.semantic_digest.as_str(), digest.as_str())?
        } else {
            false
        };
        a.require(equal, "program digest mismatch")?;
        return Ok(facts);
    }
    #[cfg(test)]
    controls::event("fallback");
    let actual = capture_typed::parse_app(&capture.source_bytes, a)?;
    let facts = actual.validate_audited(a)?;
    a.require(
        a.equal_owned(&actual, &capture.program)?,
        "source bytes do not represent captured program",
    )?;
    let raw =
        capture_validation::bytes_digest_audited(IdentityDomain::Source, &capture.source_bytes, a)?;
    a.require(
        a.eq_str(capture.artifact.raw_digest.as_str(), raw.as_str())?
            && a.eq_str(
                capture.binding.source_digest.as_str(),
                capture.artifact.raw_digest.as_str(),
            )?,
        "raw source digest mismatch",
    )?;
    a.pay(8 + 2 * size_of::<Audit<'_>>() as u64 + 4 * size_of::<bool>() as u64)?;
    if !a.enabled() {
        actual.validate_audited(a)?;
    }
    let bytes = canonical_bytes_audited(&actual, a)?;
    let program = capture_validation::bytes_digest_audited(IdentityDomain::Program, &bytes, a)?;
    let equal = a.eq_str(capture.artifact.program_digest.as_str(), program.as_str())?;
    let equal = if equal {
        a.pay(8 + 2 * size_of::<Audit<'_>>() as u64 + 4 * size_of::<bool>() as u64)?;
        if !a.enabled() {
            actual.validate_audited(a)?;
        }
        let mut semantic = a.clone_owned(&actual)?;
        semantic.clear_display_labels_audited(a)?;
        let bytes = canonical_bytes_audited(&semantic, a)?;
        let digest =
            capture_validation::bytes_digest_audited(IdentityDomain::SemanticProgram, &bytes, a)?;
        a.eq_str(capture.artifact.semantic_digest.as_str(), digest.as_str())?
    } else {
        false
    };
    a.require(equal, "program digest mismatch")?;
    Ok(facts)
}

// Tests observe only actual reached stages and inject impossible native Err at
// the Result seam. No fake serializer or test visibility exists in production.
#[cfg(test)]
pub(in super::super) mod controls {
    use super::*;
    thread_local! {
        static RECORD: std::cell::RefCell<Option<(usize,Vec<(&'static str,usize)>,Option<&'static str>)>> = const {std::cell::RefCell::new(None)};
    }
    pub(in super::super::super) fn begin(fault: Option<&'static str>) {
        RECORD.with(|r| assert!(r.borrow_mut().replace((0, vec![], fault)).is_none()));
    }
    pub(in super::super::super) fn tick() {
        RECORD.with(|r| {
            if let Some((n, _, _)) = r.borrow_mut().as_mut() {
                *n += 1;
            }
        });
    }
    pub(super) fn event(name: &'static str) {
        RECORD.with(|r| {
            if let Some((n, v, _)) = r.borrow_mut().as_mut() {
                v.push((name, *n));
            }
        });
    }
    // Differential oracle only: it is not part of the paid production graph.
    pub(super) fn materialized(value: &JsonValue, raw: &[u8]) {
        RECORD.with(|r| {
            if r.borrow().is_some() {
                assert_eq!(*value, serde_json::from_slice::<JsonValue>(raw).unwrap());
            }
        });
    }
    pub(super) fn fault(name: &'static str) -> bool {
        RECORD.with(|r| {
            if let Some((_, _, f)) = r.borrow_mut().as_mut() {
                if *f == Some(name) {
                    *f = None;
                    true
                } else {
                    false
                }
            } else {
                false
            }
        })
    }
    pub(super) fn native_result<T>(
        result: serde_json::Result<T>,
        name: &'static str,
    ) -> serde_json::Result<T> {
        if fault(name) {
            drop(result);
            Err(<serde_json::Error as serde::ser::Error>::custom(
                "injected native error",
            ))
        } else {
            result
        }
    }
    pub(in super::super::super) fn take() -> Vec<(&'static str, usize)> {
        RECORD.with(|r| r.borrow_mut().take().unwrap().1)
    }
}
#[cfg(test)]
#[path = "capture_guarded_preflight_tests.rs"]
mod preflight_tests;
