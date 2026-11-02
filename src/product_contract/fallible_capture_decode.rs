//! Fallible decoding for the persisted CapturedProgram ownership graph.
//!
//! This private DTO composes the bounded AppDefinition decoder with the small
//! source/artifact metadata graph and validates both graphs without converting
//! either one into an infallibly allocated legacy owner.
use crate::product_alloc::{self, AllocationContext, AllocationSite};
use crate::product_alloc::ordered::OrderedMap;
use crate::product_json::{Number, Value};
use serde::Serialize;

use super::fallible_app_decode::{decode_app_definition, DecodedAppDefinition};
use super::fallible_value_decode::{JsonKind, NumericValue, ValueDecodeError};
use super::{AppValidationError, ArtifactKind, ArtifactRef, ContractError, Digest, IdentityDomain, Producer, SourceBinding, TaskSource, MAX_WIRE_BYTES};

const CAPTURED_PROGRAM_FIELDS: &[&str] = &["artifact", "binding", "source_bytes", "program"];
const ARTIFACT_FIELDS: &[&str] = &["kind", "version", "language", "raw_digest", "program_digest", "semantic_digest"];
const SOURCE_BINDING_FIELDS: &[&str] = &["project_id", "program_path", "task", "producer", "source_digest"];
const TASK_SOURCE_FIELDS: &[&str] = &["task_id", "source_fingerprint"];
const PRODUCER_VARIANTS: &[&str] = &["live_agent", "external_author", "user_authored", "fixture"];
const LIVE_AGENT_FIELDS: &[&str] = &["provider", "invocation_id", "session_id", "request_digest"];
const EXTERNAL_AUTHOR_FIELDS: &[&str] = &["description"];
const FIXTURE_FIELDS: &[&str] = &["name"];
const ARTIFACT_KINDS: &[&str] = &["generated_app", "legacy_order", "external_web"];

#[derive(Debug, Serialize)]
pub(crate) struct DecodedCapturedProgram {
    artifact: ArtifactRef,
    binding: SourceBinding,
    source_bytes: Vec<u8>,
    program: DecodedAppDefinition,
}
impl DecodedCapturedProgram {
    pub(crate) fn app(&self) -> &DecodedAppDefinition { &self.program }
    pub(crate) fn validate_app_with(&self, cx: &mut AllocationContext) -> Result<(), AppValidationError> {
        self.program.validate_with(cx)
    }

    pub(crate) fn validate_session_with(
        &self,
        session: &super::fallible_persisted_decode::DecodedSessionState,
        cx: &mut AllocationContext,
    ) -> Result<(), AppValidationError> {
        self.program.validate_with(cx)?;
        self.program.validate_session_with(session).map_err(AppValidationError::Contract)
    }

    pub(crate) fn project_id(&self) -> &str { &self.binding.project_id }
    pub(crate) fn program_digest(&self) -> &Digest { &self.artifact.program_digest }

    pub(crate) fn revision_with(&self, cx: &mut AllocationContext) -> Result<Digest, AppValidationError> {
        crate::product_backup::fallible_codec::canonical_digest(IdentityDomain::Source, self, cx)
            .map_err(codec_validation_error)
    }

    pub(crate) fn validate_with(&self, cx: &mut AllocationContext) -> Result<(), AppValidationError> {
        self.binding.validate().map_err(AppValidationError::Contract)?;
        if self.artifact.version != super::CONTRACT_VERSION
            || self.artifact.kind != ArtifactKind::GeneratedApp
            || self.artifact.language != super::LANGUAGE_VERSION
        {
            return Err(AppValidationError::Contract(ContractError("unsupported executable artifact".into())));
        }
        if self.source_bytes.len() > MAX_WIRE_BYTES {
            return Err(AppValidationError::Contract(ContractError("source exceeds byte limit".into())));
        }
        let syntax = crate::product_json::parse_json_bytes(&self.source_bytes, cx).map_err(|error| match error.cause {
            crate::product_json::Cause::Allocation(failure) => AppValidationError::Allocation(failure),
            _ => AppValidationError::Contract(ContractError("invalid source bytes".into())),
        })?;
        let actual = decode_app_definition(syntax, cx).map_err(|error| match error {
            ValueDecodeError::Allocation(failure) => AppValidationError::Allocation(failure),
            _ => AppValidationError::Contract(ContractError("invalid source program".into())),
        })?;
        actual.validate_with(cx)?;
        let expected_bytes = crate::product_backup::fallible_codec::canonical_bytes(&self.program, cx)
            .map_err(codec_validation_error)?;
        let actual_bytes = crate::product_backup::fallible_codec::canonical_bytes(&actual, cx)
            .map_err(codec_validation_error)?;
        if expected_bytes != actual_bytes {
            return Err(AppValidationError::Contract(ContractError("source bytes do not represent captured program".into())));
        }
        let raw = crate::product_backup::fallible_codec::bytes_digest(IdentityDomain::Source, &self.source_bytes, cx)
            .map_err(codec_validation_error)?;
        if raw.as_str() != self.artifact.raw_digest.as_str()
            || raw.as_str() != self.binding.source_digest.as_str()
        {
            return Err(AppValidationError::Contract(ContractError("raw source digest mismatch".into())));
        }
        let program_digest = crate::product_backup::fallible_codec::canonical_digest(IdentityDomain::Program, &actual, cx)
            .map_err(codec_validation_error)?;
        if program_digest.as_str() != self.artifact.program_digest.as_str() {
            return Err(AppValidationError::Contract(ContractError("program digest mismatch".into())));
        }
        let mut semantic = actual.try_clone_with(cx)?;
        semantic.clear_display_labels();
        let semantic_digest = crate::product_backup::fallible_codec::canonical_digest(IdentityDomain::SemanticProgram, &semantic, cx)
            .map_err(codec_validation_error)?;
        if semantic_digest.as_str() != self.artifact.semantic_digest.as_str() {
            return Err(AppValidationError::Contract(ContractError("program digest mismatch".into())));
        }
        Ok(())
    }
}
fn codec_validation_error(error: crate::product_backup::fallible_codec::CodecError) -> AppValidationError {
    match error {
        crate::product_backup::fallible_codec::CodecError::Allocation(failure) => AppValidationError::Allocation(failure),
        _ => AppValidationError::Contract(ContractError("program codec invariant".into())),
    }
}

pub(super) fn decode_captured_program(
    value: Value,
    cx: &mut AllocationContext,
) -> Result<DecodedCapturedProgram, ValueDecodeError> {
    let fields = struct_fields(value, "a struct CapturedProgram", CAPTURED_PROGRAM_FIELDS, cx)?;
    let mut artifact = None;
    let mut binding = None;
    let mut source_bytes = None;
    let mut program = None;
    for (key, value) in fields.into_entries() {
        match key.as_str() {
            "artifact" => artifact = Some(decode_artifact(value, cx)?),
            "binding" => binding = Some(decode_source_binding(value, cx)?),
            "source_bytes" => source_bytes = Some(decode_source_bytes(value, cx)?),
            "program" => program = Some(decode_app_definition(value, cx)?),
            _ => return Err(unknown_field(key, CAPTURED_PROGRAM_FIELDS)),
        }
    }
    Ok(DecodedCapturedProgram {
        artifact: required(artifact, "artifact")?,
        binding: required(binding, "binding")?,
        source_bytes: required(source_bytes, "source_bytes")?,
        program: required(program, "program")?,
    })
}

pub(super) fn decode_artifact(value: Value, cx: &mut AllocationContext) -> Result<ArtifactRef, ValueDecodeError> {
    let fields = struct_fields(value, "a struct ArtifactRef", ARTIFACT_FIELDS, cx)?;
    let mut kind = None;
    let mut version = None;
    let mut language = None;
    let mut raw_digest = None;
    let mut program_digest = None;
    let mut semantic_digest = None;
    for (key, value) in fields.into_entries() {
        match key.as_str() {
            "kind" => kind = Some(decode_artifact_kind(value)?),
            "version" => version = Some(unsigned_u32(value)?),
            "language" => language = Some(string(value, "a string")?),
            "raw_digest" => raw_digest = Some(digest(value)?),
            "program_digest" => program_digest = Some(digest(value)?),
            "semantic_digest" => semantic_digest = Some(digest(value)?),
            _ => return Err(unknown_field(key, ARTIFACT_FIELDS)),
        }
    }
    Ok(ArtifactRef {
        kind: required(kind, "kind")?,
        version: required(version, "version")?,
        language: required(language, "language")?,
        raw_digest: required(raw_digest, "raw_digest")?,
        program_digest: required(program_digest, "program_digest")?,
        semantic_digest: required(semantic_digest, "semantic_digest")?,
    })
}

fn decode_artifact_kind(value: Value) -> Result<ArtifactKind, ValueDecodeError> {
    let tag = match value {
        Value::String(tag) => tag,
        Value::Object(fields) => {
            if fields.len() != 1 {
                return Err(ValueDecodeError::InvalidLength { actual: fields.len(), expected: 1 });
            }
            let (tag, payload) = fields.into_entries().next().expect("one field was checked");
            if !ARTIFACT_KINDS.contains(&tag.as_str()) {
                return Err(ValueDecodeError::UnknownVariant { tag, expected: ARTIFACT_KINDS });
            }
            match payload {
                Value::Null => tag,
                payload => return Err(invalid_type(payload, "unit")),
            }
        }
        value => return Err(invalid_type(value, "a string variant identifier")),
    };
    match tag.as_str() {
        "generated_app" => Ok(ArtifactKind::GeneratedApp),
        "legacy_order" => Ok(ArtifactKind::LegacyOrder),
        "external_web" => Ok(ArtifactKind::ExternalWeb),
        _ => Err(ValueDecodeError::UnknownVariant { tag, expected: ARTIFACT_KINDS }),
    }
}

pub(super) fn decode_source_binding(value: Value, cx: &mut AllocationContext) -> Result<SourceBinding, ValueDecodeError> {
    let fields = struct_fields(value, "a struct SourceBinding", SOURCE_BINDING_FIELDS, cx)?;
    let mut project_id = None;
    let mut program_path = None;
    // Serde treats a missing Option field in an object map as None.
    let mut task = None;
    let mut producer = None;
    let mut source_digest = None;
    for (key, value) in fields.into_entries() {
        match key.as_str() {
            "project_id" => project_id = Some(string(value, "a string")?),
            "program_path" => program_path = Some(string(value, "a string")?),
            "task" => task = Some(option(value, |v| decode_task_source(v, cx))?),
            "producer" => producer = Some(decode_producer(value)?),
            "source_digest" => source_digest = Some(digest(value)?),
            _ => return Err(unknown_field(key, SOURCE_BINDING_FIELDS)),
        }
    }
    Ok(SourceBinding {
        project_id: required(project_id, "project_id")?,
        program_path: required(program_path, "program_path")?,
        task: task.unwrap_or(None),
        producer: required(producer, "producer")?,
        source_digest: required(source_digest, "source_digest")?,
    })
}

fn decode_task_source(value: Value, cx: &mut AllocationContext) -> Result<TaskSource, ValueDecodeError> {
    let fields = struct_fields(value, "a struct TaskSource", TASK_SOURCE_FIELDS, cx)?;
    let mut task_id = None;
    let mut source_fingerprint = None;
    for (key, value) in fields.into_entries() {
        match key.as_str() {
            "task_id" => task_id = Some(string(value, "a string")?),
            "source_fingerprint" => source_fingerprint = Some(string(value, "a string")?),
            _ => return Err(unknown_field(key, TASK_SOURCE_FIELDS)),
        }
    }
    Ok(TaskSource {
        task_id: required(task_id, "task_id")?,
        source_fingerprint: required(source_fingerprint, "source_fingerprint")?,
    })
}

fn decode_producer(value: Value) -> Result<Producer, ValueDecodeError> {
    let (tag, fields) = tagged(value, PRODUCER_VARIANTS)?;
    match tag.as_str() {
        "live_agent" => {
            let mut provider = None;
            let mut invocation_id = None;
            let mut session_id = None;
            let mut request_digest = None;
            for (key, value) in fields.into_entries() {
                match key.as_str() {
                    "provider" => provider = Some(string(value, "a string")?),
                    "invocation_id" => invocation_id = Some(string(value, "a string")?),
                    "session_id" => session_id = Some(option(value, string_option)?),
                    "request_digest" => request_digest = Some(digest(value)?),
                    _ => return Err(unknown_field(key, LIVE_AGENT_FIELDS)),
                }
            }
            Ok(Producer::LiveAgent {
                provider: required(provider, "provider")?,
                invocation_id: required(invocation_id, "invocation_id")?,
                session_id: session_id.unwrap_or(None),
                request_digest: required(request_digest, "request_digest")?,
            })
        }
        "external_author" => {
            let mut description = None;
            for (key, value) in fields.into_entries() {
                if key == "description" {
                    description = Some(string(value, "a string")?);
                } else {
                    return Err(unknown_field(key, EXTERNAL_AUTHOR_FIELDS));
                }
            }
            Ok(Producer::ExternalAuthor { description: required(description, "description")? })
        }
        "user_authored" => unit(fields, &[]).map(|()| Producer::UserAuthored),
        "fixture" => {
            let mut name = None;
            for (key, value) in fields.into_entries() {
                if key == "name" {
                    name = Some(string(value, "a string")?);
                } else {
                    return Err(unknown_field(key, FIXTURE_FIELDS));
                }
            }
            Ok(Producer::Fixture { name: required(name, "name")? })
        }
        _ => unreachable!("producer variant list is complete"),
    }
}

fn decode_source_bytes(value: Value, cx: &mut AllocationContext) -> Result<Vec<u8>, ValueDecodeError> {
    let values = match value {
        Value::Array(values) => values,
        value => return Err(invalid_type(value, "a sequence")),
    };
    if values.len() > MAX_WIRE_BYTES {
        return Err(ValueDecodeError::LimitExceeded { resource: "source byte count", limit: MAX_WIRE_BYTES });
    }
    let mut bytes = Vec::new();
    product_alloc::try_reserve(&mut bytes, values.len(), cx, AllocationSite::Vector)?;
    for value in values {
        bytes.push(unsigned_u8(value)?);
    }
    Ok(bytes)
}

fn unsigned_u8(value: Value) -> Result<u8, ValueDecodeError> {
    match value {
        Value::Number(Number::U64(value)) => u8::try_from(value).map_err(|_| ValueDecodeError::InvalidValue { actual: NumericValue::Unsigned(value), expected: "u8" }),
        Value::Number(Number::I64(value)) => u8::try_from(value).map_err(|_| ValueDecodeError::InvalidValue { actual: NumericValue::Signed(value), expected: "u8" }),
        value => Err(invalid_type(value, "u8")),
    }
}

fn unsigned_u32(value: Value) -> Result<u32, ValueDecodeError> {
    match value {
        Value::Number(Number::U64(value)) => u32::try_from(value).map_err(|_| ValueDecodeError::InvalidValue { actual: NumericValue::Unsigned(value), expected: "u32" }),
        Value::Number(Number::I64(value)) => u32::try_from(value).map_err(|_| ValueDecodeError::InvalidValue { actual: NumericValue::Signed(value), expected: "u32" }),
        value => Err(invalid_type(value, "u32")),
    }
}

fn digest(value: Value) -> Result<Digest, ValueDecodeError> {
    let value = string(value, "a lowercase 64-character digest")?;
    Digest::from_owned_lower_hex(value).ok_or(ValueDecodeError::InvalidFormat("invalid lowercase digest"))
}

fn tagged(value: Value, variants: &'static [&'static str]) -> Result<(String, OrderedMap<String, Value>), ValueDecodeError> {
    let mut fields = object(value, "an internally tagged enum")?;
    let tag = fields.remove("kind").ok_or(ValueDecodeError::MissingField("kind"))?;
    let tag = string(tag, "a string variant identifier")?;
    if variants.contains(&tag.as_str()) {
        Ok((tag, fields))
    } else {
        Err(ValueDecodeError::UnknownVariant { tag, expected: variants })
    }
}

fn struct_fields(value: Value, expected: &'static str, names: &'static [&'static str], cx: &mut AllocationContext) -> Result<OrderedMap<String, Value>, ValueDecodeError> {
    match value {
        Value::Object(fields) => Ok(fields),
        Value::Array(values) => {
            if values.len() != names.len() {
                return Err(ValueDecodeError::InvalidLength { actual: values.len(), expected: names.len() });
            }
            let mut fields = OrderedMap::new();
            fields.try_reserve(names.len(), cx)?;
            for (name, value) in names.iter().zip(values) {
                let key = product_alloc::try_copy_str(name, cx, AllocationSite::String)?;
                fields.try_insert(key, value, cx)?;
            }
            Ok(fields)
        }
        value => Err(invalid_type(value, expected)),
    }
}

fn unit(fields: OrderedMap<String, Value>, expected: &'static [&'static str]) -> Result<(), ValueDecodeError> {
    if let Some((key, _)) = fields.into_entries().next() {
        Err(unknown_field(key, expected))
    } else {
        Ok(())
    }
}

fn object(value: Value, expected: &'static str) -> Result<OrderedMap<String, Value>, ValueDecodeError> {
    match value {
        Value::Object(fields) => Ok(fields),
        value => Err(invalid_type(value, expected)),
    }
}

fn option<T>(value: Value, decode: impl FnOnce(Value) -> Result<T, ValueDecodeError>) -> Result<Option<T>, ValueDecodeError> {
    match value {
        Value::Null => Ok(None),
        value => decode(value).map(Some),
    }
}

fn string_option(value: Value) -> Result<String, ValueDecodeError> {
    string(value, "a string")
}

fn string(value: Value, expected: &'static str) -> Result<String, ValueDecodeError> {
    match value {
        Value::String(value) => Ok(value),
        value => Err(invalid_type(value, expected)),
    }
}

fn required<T>(value: Option<T>, field: &'static str) -> Result<T, ValueDecodeError> {
    value.ok_or(ValueDecodeError::MissingField(field))
}

fn unknown_field(key: String, expected: &'static [&'static str]) -> ValueDecodeError {
    ValueDecodeError::UnknownField { key, expected }
}

fn invalid_type(value: Value, expected: &'static str) -> ValueDecodeError {
    let actual = match value {
        Value::Null => JsonKind::Null,
        Value::Bool(_) => JsonKind::Boolean,
        Value::Number(Number::U64(_)) => JsonKind::UnsignedInteger,
        Value::Number(Number::I64(_)) => JsonKind::SignedInteger,
        Value::Number(Number::F64(_)) => JsonKind::FloatingPoint,
        Value::String(_) => JsonKind::String,
        Value::Array(_) => JsonKind::Sequence,
        Value::Object(_) => JsonKind::Map,
    };
    ValueDecodeError::InvalidType { actual, expected }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::CapturedProgram;
    use crate::product_alloc::AllocationFailureKind;
    use crate::product_json::{self, Cause};
    use serde_json::Value as SerdeValue;
    use std::num::NonZeroUsize;

    const MINIMAL_APP: &str = r#"{"version":1,"id":"source_fixture","label":"Source fixture","entities":[{"id":"row","label":"Rows","fields":[{"id":"name","label":"Name","value_type":{"kind":"text"}}],"unique":[],"constraints":[]}],"state":[],"actions":[{"id":"check","label":"Check","parameters":{},"guards":[],"steps":[{"kind":"assert","condition":{"kind":"literal","value_type":{"kind":"boolean"},"value":{"kind":"boolean","value":true}},"message":"yes"}],"ensures":[]}],"outputs":[],"observables":[{"id":"yes","label":"Yes","value":{"kind":"literal","value_type":{"kind":"boolean"},"value":{"kind":"boolean","value":true}}}],"views":[{"id":"rows","label":"Rows","kind":{"kind":"list","entity":"row","rows":{"kind":"query","entity":"row","binding":"item","predicate":{"kind":"literal","value_type":{"kind":"boolean"},"value":{"kind":"boolean","value":true}},"sort":[],"limit":1,"include_archived":false},"columns":[{"id":"name","label":"Name","value":{"kind":"field","record":{"kind":"variable","name":"row"},"field":"name"}}],"controls":[],"selection":null},"actions":[],"keys":[]}],"initial_view":"rows"}"#;

    fn syntax(source: &[u8]) -> Value {
        product_json::parse_json_bytes(source, &mut AllocationContext::new()).unwrap()
    }

    fn minimal_app_bytes() -> Vec<u8> {
        let app: super::super::AppDefinition = serde_json::from_str(MINIMAL_APP).unwrap();
        serde_json::to_vec(&app).unwrap()
    }

    fn fixture_capture() -> CapturedProgram {
        let source_bytes = minimal_app_bytes();
        let task = Some(TaskSource { task_id: "task-1".into(), source_fingerprint: "fingerprint".into() });
        CapturedProgram::capture(
            &source_bytes,
            "project",
            Producer::LiveAgent {
                provider: "test".into(),
                invocation_id: "invoke-1".into(),
                session_id: Some("session-1".into()),
                request_digest: Digest::try_from("a".repeat(64)).unwrap(),
            },
            task,
        ).unwrap()
    }

    fn assert_allocation(error: ValueDecodeError, cx: &AllocationContext) {
        let ValueDecodeError::Allocation(failure) = error else { panic!("expected allocation refusal") };
        assert_eq!(Some(failure), cx.first_failure());
        assert_eq!(failure.kind, AllocationFailureKind::Injected);
    }

    #[test]
    fn complete_capture_graph_matches_canonical_bytes_and_preserves_source() {
        let capture = fixture_capture();
        let input = serde_json::to_vec(&capture).unwrap();
        let mut cx = AllocationContext::new();
        let decoded = decode_captured_program(syntax(&input), &mut cx).unwrap();
        assert_eq!(capture.source_bytes, decoded.source_bytes);
        assert_eq!(super::super::canonical_bytes(&capture).unwrap(), super::super::canonical_bytes(&decoded).unwrap());
    }

    #[test]
    fn decoded_capture_validation_composes_fallible_app_validation() {
        let capture = fixture_capture();
        let input = serde_json::to_vec(&capture).unwrap();
        let mut production_cx = AllocationContext::new();
        let wrapped = super::super::decode_captured_program_fallible(syntax(&input), &mut production_cx).unwrap();
        let before = super::super::canonical_bytes(&wrapped).unwrap();
        let mut baseline = AllocationContext::new();
        wrapped.validate_with(&mut baseline).unwrap();
        let growths = baseline.growth_requests();
        assert!(growths > 0);
        for nth in 1..=growths {
            let mut refused = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), true);
            let error = wrapped.validate_with(&mut refused).unwrap_err();
            let AppValidationError::Allocation(failure) = error else {
                panic!("expected original allocation refusal at wrapper validation growth {nth}: {error:?}");
            };
            assert_eq!(Some(failure), refused.first_failure());
            assert_eq!(failure.kind, AllocationFailureKind::Injected);
            assert_eq!(before, super::super::canonical_bytes(&wrapped).unwrap());
            wrapped.validate_with(&mut AllocationContext::new()).unwrap();
        }

        let mut stale_json: SerdeValue = serde_json::to_value(&capture).unwrap();
        stale_json["artifact"]["program_digest"] = SerdeValue::String("0".repeat(64));
        let stale_input = serde_json::to_vec(&stale_json).unwrap();
        let stale = super::super::decode_captured_program_fallible(syntax(&stale_input), &mut AllocationContext::new()).unwrap();
        assert_eq!(
            stale.validate_with(&mut AllocationContext::new()),
            Err(AppValidationError::Contract(ContractError("program digest mismatch".into()))),
        );
    }

    #[test]
    fn artifact_kind_accepts_both_derived_serde_unit_forms() {
        for (tag, expected) in [
            ("generated_app", ArtifactKind::GeneratedApp),
            ("legacy_order", ArtifactKind::LegacyOrder),
            ("external_web", ArtifactKind::ExternalWeb),
        ] {
            let string = SerdeValue::String(tag.into());
            let oracle: ArtifactKind = serde_json::from_value(string.clone()).unwrap();
            assert_eq!(expected, oracle);
            assert_eq!(expected, decode_artifact_kind(syntax(&serde_json::to_vec(&string).unwrap())).unwrap());

            let mut map = serde_json::Map::new();
            map.insert(tag.into(), SerdeValue::Null);
            let map = SerdeValue::Object(map);
            let oracle: ArtifactKind = serde_json::from_value(map.clone()).unwrap();
            assert_eq!(expected, oracle);
            assert_eq!(expected, decode_artifact_kind(syntax(&serde_json::to_vec(&map).unwrap())).unwrap());
        }
        assert!(matches!(decode_artifact_kind(syntax(br#"{"future":null}"#)), Err(ValueDecodeError::UnknownVariant { tag, .. }) if tag == "future"));
        assert!(matches!(decode_artifact_kind(syntax(br#"{"generated_app":true}"#)), Err(ValueDecodeError::InvalidType { expected: "unit", .. })));
        assert!(matches!(decode_artifact_kind(syntax(br#"{"generated_app":null,"external_web":null}"#)), Err(ValueDecodeError::InvalidLength { actual: 2, expected: 1 })));
    }

    #[test]
    fn producer_variants_and_optional_object_fields_keep_serde_defaults() {
        let source_bytes = minimal_app_bytes();
        let base = fixture_capture();
        for kind in ["legacy_order", "external_web"] {
            let mut value = serde_json::to_value(&base).unwrap();
            if let SerdeValue::Object(ref mut fields) = value {
                if let Some(SerdeValue::Object(ref mut artifact)) = fields.get_mut("artifact") {
                    artifact.insert("kind".into(), SerdeValue::String(kind.into()));
                }
            }
            let json = serde_json::to_vec(&value).unwrap();
            let expected: CapturedProgram = serde_json::from_slice(&json).unwrap();
            let decoded = decode_captured_program(syntax(&json), &mut AllocationContext::new()).unwrap();
            assert_eq!(super::super::canonical_bytes(&expected).unwrap(), super::super::canonical_bytes(&decoded).unwrap());
        }

        let producers = [
            Producer::UserAuthored,
            Producer::ExternalAuthor { description: "written by a person".into() },
            Producer::Fixture { name: "fixture".into() },
            Producer::LiveAgent {
                provider: "test".into(),
                invocation_id: "invoke".into(),
                session_id: None,
                request_digest: Digest::try_from("b".repeat(64)).unwrap(),
            },
        ];
        for producer in producers {
            for task in [None, Some(TaskSource { task_id: "task".into(), source_fingerprint: "fp".into() })] {
                let capture = CapturedProgram::capture(&source_bytes, "project", producer.clone(), task).unwrap();
                let mut value = serde_json::to_value(&capture).unwrap();
                if let SerdeValue::Object(ref mut fields) = value {
                    if let Some(SerdeValue::Object(ref mut binding)) = fields.get_mut("binding") {
                        binding.remove("task");
                    }
                    if let Some(SerdeValue::Object(ref mut binding)) = fields.get_mut("binding") {
                        if let Some(SerdeValue::Object(ref mut producer)) = binding.get_mut("producer") {
                            producer.remove("session_id");
                        }
                    }
                }
                let json = serde_json::to_vec(&value).unwrap();
                let expected: CapturedProgram = serde_json::from_slice(&json).unwrap();
                let mut cx = AllocationContext::new();
                let decoded = decode_captured_program(syntax(&json), &mut cx).unwrap();
                assert_eq!(super::super::canonical_bytes(&expected).unwrap(), super::super::canonical_bytes(&decoded).unwrap());
            }
        }
    }

    #[test]
    fn allocation_refusal_at_each_growth_cleans_and_retries() {
        let capture = fixture_capture();
        let input = serde_json::to_vec(&capture).unwrap();
        let expected = super::super::canonical_bytes(&capture).unwrap();
        let mut baseline = AllocationContext::new();
        decode_captured_program(syntax(&input), &mut baseline).unwrap();
        let growths = baseline.growth_requests();
        assert!(growths > 0, "expected capture owner growth to be exercised");
        for nth in 1..=growths {
            let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), true);
            assert_allocation(decode_captured_program(syntax(&input), &mut cx).unwrap_err(), &cx);
            let mut retry = AllocationContext::new();
            let decoded = decode_captured_program(syntax(&input), &mut retry).unwrap();
            assert_eq!(expected, super::super::canonical_bytes(&decoded).unwrap());
        }
    }

    #[test]
    fn malformed_fields_tags_digests_bytes_and_limits_are_typed() {
        let capture = fixture_capture();
        let mut value = serde_json::to_value(&capture).unwrap();
        let SerdeValue::Object(ref mut fields) = value else { unreachable!() };
        let mut cx = AllocationContext::new();
        let duplicate = match product_json::parse_json_bytes(br#"{"artifact":{},"artifact":{}}"#, &mut cx) {
            Err(error) => error,
            Ok(_) => panic!("duplicate CapturedProgram field was accepted"),
        };
        assert_eq!(duplicate.cause, Cause::DuplicateObjectMember);

        let mut missing = value.clone();
        if let SerdeValue::Object(ref mut fields) = missing { fields.remove("program"); }
        let json = serde_json::to_vec(&missing).unwrap();
        assert!(matches!(decode_captured_program(syntax(&json), &mut AllocationContext::new()), Err(ValueDecodeError::MissingField("program"))));

        let mut unknown = value.clone();
        if let SerdeValue::Object(ref mut fields) = unknown { fields.insert("future".into(), SerdeValue::Null); }
        let json = serde_json::to_vec(&unknown).unwrap();
        assert!(matches!(decode_captured_program(syntax(&json), &mut AllocationContext::new()), Err(ValueDecodeError::UnknownField { .. })));

        let mut bad_kind = value.clone();
        if let SerdeValue::Object(ref mut fields) = bad_kind {
            if let Some(SerdeValue::Object(ref mut artifact)) = fields.get_mut("artifact") { artifact.insert("kind".into(), SerdeValue::String("future".into())); }
        }
        let json = serde_json::to_vec(&bad_kind).unwrap();
        assert!(matches!(decode_captured_program(syntax(&json), &mut AllocationContext::new()), Err(ValueDecodeError::UnknownVariant { .. })));

        let mut bad_version = value.clone();
        if let SerdeValue::Object(ref mut fields) = bad_version {
            if let Some(SerdeValue::Object(ref mut artifact)) = fields.get_mut("artifact") {
                artifact.insert("version".into(), serde_json::json!(u64::from(u32::MAX) + 1));
            }
        }
        let json = serde_json::to_vec(&bad_version).unwrap();
        assert!(matches!(decode_captured_program(syntax(&json), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidValue { expected: "u32", .. })));

        let mut bad_digest = value.clone();
        if let SerdeValue::Object(ref mut fields) = bad_digest {
            if let Some(SerdeValue::Object(ref mut artifact)) = fields.get_mut("artifact") { artifact.insert("raw_digest".into(), SerdeValue::String("BAD".into())); }
        }
        let json = serde_json::to_vec(&bad_digest).unwrap();
        assert!(matches!(decode_captured_program(syntax(&json), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidFormat("invalid lowercase digest"))));

        let mut bad_bytes = value.clone();
        if let SerdeValue::Object(ref mut fields) = bad_bytes { fields.insert("source_bytes".into(), SerdeValue::Array(vec![SerdeValue::Number(serde_json::Number::from(256u64))])); }
        let json = serde_json::to_vec(&bad_bytes).unwrap();
        assert!(matches!(decode_captured_program(syntax(&json), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidValue { expected: "u8", .. })));

        let oversized = std::iter::repeat_with(|| Value::Number(Number::U64(0))).take(MAX_WIRE_BYTES + 1).collect();
        assert!(matches!(decode_source_bytes(Value::Array(oversized), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "source byte count", limit: MAX_WIRE_BYTES })));
    }

    #[test]
    fn source_bytes_array_reservation_refusal_is_prepaid_and_retryable() {
        let source = vec![0u8, 1, 2, 255];
        let mut cx = AllocationContext::new();
        assert_eq!(decode_source_bytes(Value::Array(source.iter().copied().map(|b| Value::Number(Number::U64(b as u64))).collect()), &mut cx).unwrap(), source);
        assert_eq!(cx.growth_requests(), 1);
        let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(1).unwrap(), true);
        let values = || std::iter::repeat_with(|| Value::Number(Number::U64(1))).take(source.len()).collect();
        let err = decode_source_bytes(Value::Array(values()), &mut cx).unwrap_err();
        assert_allocation(err, &cx);
        let mut retry = AllocationContext::new();
        assert_eq!(decode_source_bytes(Value::Array(values()), &mut retry).unwrap(), vec![1; source.len()]);
    }
}
