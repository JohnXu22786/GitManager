use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value};
use std::fmt;
struct Strict(Value);
impl<'de> Deserialize<'de> for Strict {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Json;
        impl<'de> Visitor<'de> for Json {
            type Value = Strict;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("JSON without duplicate keys")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_f64<E: de::Error>(self, _: f64) -> Result<Strict, E> {
                Err(E::custom(
                    "floating point values are not part of the store format",
                ))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Strict, E> {
                Ok(Strict(Value::Null))
            }
            fn visit_none<E: de::Error>(self) -> Result<Strict, E> {
                Ok(Strict(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Strict, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = seq.next_element::<Strict>()? {
                    values.push(value.0);
                }
                Ok(Strict(Value::Array(values)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Strict, A::Error> {
                let mut values = Map::new();
                while let Some((key, value)) = map.next_entry::<String, Strict>()? {
                    if values.insert(key, value.0).is_some() {
                        return Err(de::Error::custom("duplicate JSON member"));
                    }
                }
                Ok(Strict(Value::Object(values)))
            }
        }
        deserializer.deserialize_any(Json)
    }
}
pub(super) fn parse<T: de::DeserializeOwned>(bytes: &[u8]) -> Result<T, serde_json::Error> {
    let Strict(value) = serde_json::from_slice(bytes)?;
    serde_json::from_value(value)
}
