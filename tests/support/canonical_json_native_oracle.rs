// TEST-ONLY verbatim original canonical_bytes; never a production fallback.
// Original capture_native_oracle/product_contract.rs lines171-188 inclusive.
use serde::Serialize;
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;
#[derive(Debug)]
pub struct ContractError(pub String);
type Result<T> = std::result::Result<T, ContractError>;
pub fn canonical_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    fn sorted(value: JsonValue) -> JsonValue {
        match value {
            JsonValue::Object(map) => JsonValue::Object(
                map.into_iter()
                    .collect::<BTreeMap<_, _>>()
                    .into_iter()
                    .map(|(k, v)| (k, sorted(v)))
                    .collect(),
            ),
            JsonValue::Array(array) => JsonValue::Array(array.into_iter().map(sorted).collect()),
            scalar => scalar,
        }
    }
    serde_json::to_value(value)
        .and_then(|v| serde_json::to_vec(&sorted(v)))
        .map_err(|e| ContractError(e.to_string()))
}
