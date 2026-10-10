//! Pure local storage tests, never live provider or execution evidence.
#[path = "fixtures/product_discovery/mod.rs"]
mod discovery_fixture;
#[path = "fixtures/product_runtime/mod.rs"]
mod fixture;
#[path = "../src/product_contract.rs"]
mod product_contract;
#[path = "../src/product_protocol.rs"]
mod product_protocol;
// Import the actual provider module; these tests never launch a provider.
#[path = "../src/product_provider/mod.rs"]
mod product_provider;
#[path = "../src/product_response_storage.rs"]
mod product_response_storage;

use flate2::{write::ZlibEncoder, Compression};
use product_contract::DevelopmentResponse;
use product_provider::{digest, MAX_RESULT_BYTES};
use product_response_storage::ExactStoredResponseV1;
use serde_json::{json, Value};
use std::io::Write;

fn response() -> Vec<u8> {
    let (_, result) = discovery_fixture::input();
    serde_json::to_vec_pretty(&result.response).unwrap()
}
fn compressed(raw: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(raw).unwrap();
    encoder.finish().unwrap()
}
// Build untrusted claims independently of the encoder, including invalid JSON.
fn untrusted(raw: &[u8]) -> Value {
    json!({
        "encoding": "gitmanager.host-response.zlib-bytes", "version": 1,
        "raw_bytes": raw.len(), "raw_sha256": digest(raw),
        "compressed_bytes": compressed(raw)
    })
}
fn decode(value: Value, limit: usize) -> Result<Vec<u8>, product_contract::ContractError> {
    serde_json::from_value::<ExactStoredResponseV1>(value)
        .unwrap()
        .to_bytes(limit)
}
fn minimal_response() -> Vec<u8> {
    serde_json::to_vec(&json!({
        "version": 1, "request_digest": "0".repeat(64), "candidates": [],
        "hypotheses": [], "evolutions": [], "unsupported": ["synthetic unsupported capability"]
    }))
    .unwrap()
}

#[test]
fn retained_response_round_trips_exact_bytes_and_actual_receipt_digest() {
    // Retained fictional Modify output. This hash is its actual transport
    // fixture receipt.result_digest, not a newly serialized response digest.
    let raw = include_bytes!("fixtures/product_response_storage/retained_modify.json");
    let receipt_digest = "a1cfb58f146798faecc2a7b311c406a38fddb7e89667f7dcdadc3e5e6a19b8a2";
    assert_eq!(digest(raw), receipt_digest);
    let stored = ExactStoredResponseV1::from_bytes(raw, MAX_RESULT_BYTES)
        .expect("valid retained raw response must encode losslessly");
    let encoded = serde_json::to_vec(&stored).unwrap();
    assert!(encoded.len() < raw.len());
    let restored = ExactStoredResponseV1::parse(&encoded, MAX_RESULT_BYTES)
        .unwrap()
        .to_bytes(MAX_RESULT_BYTES)
        .unwrap();
    assert_eq!(restored, raw);
    assert_eq!(digest(&restored), receipt_digest);
    assert_eq!(
        DevelopmentResponse::parse(&restored)
            .unwrap()
            .candidates
            .len(),
        1
    );
}

#[test]
fn noncanonical_unicode_escapes_and_whitespace_survive_without_reserialization() {
    let mut value: Value = serde_json::from_slice(&response()).unwrap();
    value["unsupported"] = json!(["Fictional café 中文 \"quote\" \\ / 😀"]);
    let raw = format!(
        " \n{}\r\n\t",
        serde_json::to_string_pretty(&value)
            .unwrap()
            .replace("café", "caf\\u00e9")
            .replace('😀', "\\ud83d\\ude00")
            .replace('/', "\\/")
    )
    .into_bytes();
    let parsed = DevelopmentResponse::parse(&raw).unwrap();
    assert_ne!(serde_json::to_vec(&parsed).unwrap(), raw);
    let stored = ExactStoredResponseV1::from_bytes(&raw, raw.len()).unwrap();
    assert_eq!(stored.to_bytes(raw.len()).unwrap(), raw);
}

#[test]
fn raw_boundary_is_inclusive_and_smaller_caller_limits_remain_binding() {
    let mut raw = minimal_response();
    raw.resize(MAX_RESULT_BYTES, b' ');
    let stored = ExactStoredResponseV1::from_bytes(&raw, MAX_RESULT_BYTES).unwrap();
    assert_eq!(stored.to_bytes(MAX_RESULT_BYTES).unwrap(), raw);
    assert!(stored.to_bytes(MAX_RESULT_BYTES - 1).is_err());
    raw.push(b' ');
    assert!(ExactStoredResponseV1::from_bytes(&raw, MAX_RESULT_BYTES).is_err());
    let raw = minimal_response();
    let stored = ExactStoredResponseV1::from_bytes(&raw, raw.len()).unwrap();
    assert_eq!(stored.to_bytes(raw.len()).unwrap(), raw);
    assert!(ExactStoredResponseV1::from_bytes(&raw, raw.len() - 1).is_err());
    assert!(stored.to_bytes(raw.len() - 1).is_err());
    for limit in [0, MAX_RESULT_BYTES + 1, usize::MAX] {
        assert!(ExactStoredResponseV1::from_bytes(&raw, limit).is_err());
        assert!(stored.to_bytes(limit).is_err());
    }
    assert!(ExactStoredResponseV1::from_bytes(b"", MAX_RESULT_BYTES).is_err());
    assert!(decode(untrusted(b""), MAX_RESULT_BYTES).is_err());
}

#[test]
fn declared_length_and_digest_are_checked_before_returning_bytes() {
    let raw = minimal_response();
    for len in [
        0,
        raw.len() - 1,
        raw.len() + 1,
        MAX_RESULT_BYTES + 1,
        usize::MAX,
    ] {
        let mut value = untrusted(&raw);
        value["raw_bytes"] = json!(len);
        assert!(decode(value, MAX_RESULT_BYTES).is_err(), "length {len}");
    }
    for hash in [
        "".to_owned(),
        "a".repeat(63),
        "g".repeat(64),
        "A".repeat(64),
        "0".repeat(64),
    ] {
        let mut value = untrusted(&raw);
        value["raw_sha256"] = json!(hash);
        assert!(decode(value, MAX_RESULT_BYTES).is_err());
    }
}

#[test]
fn corruption_truncation_trailing_data_and_concatenated_streams_are_rejected() {
    let raw = minimal_response();
    let stream = compressed(&raw);
    let mut streams = vec![vec![], vec![0], vec![0xff; 12]];
    for end in [1, 2, stream.len() / 2, stream.len() - 4, stream.len() - 1] {
        streams.push(stream[..end].to_vec());
    }
    let mut corrupt = stream.clone();
    *corrupt.last_mut().unwrap() ^= 1;
    streams.push(corrupt);
    let mut trailing = stream.clone();
    trailing.push(0);
    streams.push(trailing);
    let mut concatenated = stream.clone();
    concatenated.extend_from_slice(&stream);
    streams.push(concatenated);
    for bytes in streams {
        let mut value = untrusted(&raw);
        value["compressed_bytes"] = json!(bytes);
        assert!(decode(value, MAX_RESULT_BYTES).is_err());
    }
}

#[test]
fn preset_dictionary_stream_cannot_request_external_state() {
    let mut value = untrusted(&minimal_response());
    // Valid zlib header with FDICT plus dictionary identifier. No dictionary
    // can be supplied through this storage interface.
    value["compressed_bytes"] = json!([0x78, 0x20, 0, 0, 0, 1, 3, 0, 0, 0, 0, 1]);
    assert!(decode(value, MAX_RESULT_BYTES).is_err());
}

#[test]
fn compressed_input_and_absolute_expansion_caps_reject_bombs() {
    let mut value = untrusted(&minimal_response());
    value["compressed_bytes"] = json!(vec![0; MAX_RESULT_BYTES + 1]);
    assert!(decode(value, MAX_RESULT_BYTES)
        .unwrap_err()
        .0
        .contains("compressed"));
    let bomb = vec![b' '; MAX_RESULT_BYTES * 8];
    let mut value = untrusted(&bomb);
    value["raw_bytes"] = json!(MAX_RESULT_BYTES);
    assert!(decode(value.clone(), MAX_RESULT_BYTES)
        .unwrap_err()
        .0
        .contains("length"));
    value["raw_bytes"] = json!(17);
    assert!(decode(value, 17).unwrap_err().0.contains("length"));
}

#[test]
fn stored_json_rejects_duplicates_unknown_fields_versions_depth_and_oversize() {
    let value = untrusted(&minimal_response());
    let text = serde_json::to_string(&value).unwrap();
    let duplicate = text.replacen("\"version\":1", "\"version\":1,\"vers\\u0069on\":1", 1);
    assert!(ExactStoredResponseV1::parse(duplicate.as_bytes(), MAX_RESULT_BYTES).is_err());
    for (key, replacement) in [
        ("extra", json!(true)),
        ("version", json!(2)),
        ("encoding", json!("gzip")),
        ("raw_bytes", json!(-1)),
        ("compressed_bytes", json!([256])),
    ] {
        let mut invalid = value.clone();
        invalid[key] = replacement;
        assert!(ExactStoredResponseV1::parse(
            &serde_json::to_vec(&invalid).unwrap(),
            MAX_RESULT_BYTES
        )
        .is_err());
    }
    let deep = format!("{}0{}", "[".repeat(65), "]".repeat(65));
    assert!(
        ExactStoredResponseV1::parse(deep.as_bytes(), MAX_RESULT_BYTES)
            .unwrap_err()
            .0
            .contains("nesting")
    );
    let mut oversized = text.into_bytes();
    oversized.resize(MAX_RESULT_BYTES + 1, b' ');
    assert!(ExactStoredResponseV1::parse(&oversized, MAX_RESULT_BYTES).is_err());
}

#[test]
fn exact_outer_dto_boundary_is_checked_independently_of_raw_response_limit() {
    let value = untrusted(&minimal_response());
    let mut text = serde_json::to_vec(&value).unwrap();
    text.resize(MAX_RESULT_BYTES, b' ');
    assert!(ExactStoredResponseV1::parse(&text, MAX_RESULT_BYTES).is_ok());
    text.push(b' ');
    assert!(ExactStoredResponseV1::parse(&text, MAX_RESULT_BYTES).is_err());

    // Valid but intentionally incompressible text. The raw domain response
    // fits; JSON's decimal-byte vector does not fit the existing journal cap.
    let mut state = 1u32;
    let unsupported: Vec<String> = (0..220)
        .map(|_| {
            (0..4096)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 17;
                    state ^= state << 5;
                    (b'!' + (state % 90) as u8) as char
                })
                .collect()
        })
        .collect();
    let mut response: Value = serde_json::from_slice(&minimal_response()).unwrap();
    response["unsupported"] = json!(unsupported);
    let raw = serde_json::to_vec(&response).unwrap();
    assert!(raw.len() < MAX_RESULT_BYTES);
    DevelopmentResponse::parse(&raw).unwrap();
    assert!(serde_json::to_vec(&untrusted(&raw)).unwrap().len() > MAX_RESULT_BYTES);
    assert!(ExactStoredResponseV1::from_bytes(&raw, MAX_RESULT_BYTES)
        .unwrap_err()
        .0
        .contains("stored response JSON"));
    assert!(decode(untrusted(&raw), MAX_RESULT_BYTES)
        .unwrap_err()
        .0
        .contains("stored response JSON"));
}

#[test]
fn decoded_response_keeps_strict_json_domain_and_nested_source_validation() {
    let raw = String::from_utf8(minimal_response()).unwrap();
    let duplicate = raw.replacen("\"version\":1", "\"version\":1,\"vers\\u0069on\":1", 1);
    let deep = format!("{}0{}", "[".repeat(65), "]".repeat(65));
    let mut cases = vec![
        b" ".to_vec(),
        b"\xff".to_vec(),
        b"{}".to_vec(),
        duplicate.into_bytes(),
        deep.into_bytes(),
    ];
    for (key, replacement) in [
        ("version", json!(2)),
        ("unexpected", json!(true)),
        ("unsupported", json!([])),
    ] {
        let mut invalid: Value = serde_json::from_str(&raw).unwrap();
        invalid[key] = replacement;
        cases.push(serde_json::to_vec(&invalid).unwrap());
    }
    let mut invalid: Value = serde_json::from_slice(&response()).unwrap();
    invalid["candidates"][0]["source_json"] = json!("{\"version\":1,\"version\":1}");
    cases.push(serde_json::to_vec(&invalid).unwrap());
    let mut trailing = raw.into_bytes();
    trailing.extend_from_slice(b" {}");
    cases.push(trailing);
    for raw in cases {
        assert!(ExactStoredResponseV1::from_bytes(&raw, MAX_RESULT_BYTES).is_err());
        assert!(decode(untrusted(&raw), MAX_RESULT_BYTES).is_err());
    }
}

#[test]
fn successful_decode_does_not_establish_request_correlation_or_receipt_authority() {
    let (request, result) = discovery_fixture::input();
    let raw = serde_json::to_vec(&result.response).unwrap();
    let stored = ExactStoredResponseV1::from_bytes(&raw, MAX_RESULT_BYTES).unwrap();
    let restored = stored.to_bytes(MAX_RESULT_BYTES).unwrap();
    let parsed = DevelopmentResponse::parse(&restored).unwrap();
    parsed.validate_for(&request).unwrap();
    let mut other_request = request.clone();
    other_request
        .request
        .push_str(" with a different approved need");
    assert!(parsed.validate_for(&other_request).is_err());
    assert_ne!(digest(&restored), digest(b"another receipt's raw result"));
    assert_eq!(parsed, result.response);
}

#[test]
fn cancellation_before_and_during_decode_returns_no_partial_response() {
    use std::cell::Cell;
    let mut raw = minimal_response();
    raw.resize(MAX_RESULT_BYTES, b' ');
    let stored = ExactStoredResponseV1::from_bytes(&raw, MAX_RESULT_BYTES).unwrap();
    for stop_at in [1, 4] {
        let calls = Cell::new(0);
        let error = stored
            .to_bytes_with_cancel(MAX_RESULT_BYTES, &|| {
                calls.set(calls.get() + 1);
                calls.get() >= stop_at
            })
            .unwrap_err();
        assert!(error.0.contains("cancelled"));
        assert_eq!(calls.get(), stop_at);
    }
    // Cancellation never alters the stored field or yields partial bytes.
    assert_eq!(stored.to_bytes(MAX_RESULT_BYTES).unwrap(), raw);
}
