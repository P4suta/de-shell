use serde::Deserialize;
use serde::de::{self, DeserializeOwned, MapAccess, SeqAccess, Visitor};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fmt;

pub(crate) fn parse(input: &[u8]) -> Result<Value, String> {
    let mut deserializer = serde_json::Deserializer::from_slice(input);
    let value = StrictValue::deserialize(&mut deserializer)
        .map_err(|error| format!("invalid JSON: {error}"))?;
    deserializer
        .end()
        .map_err(|error| format!("invalid JSON: {error}"))?;
    Ok(value.0)
}

pub(crate) fn decode<T: DeserializeOwned>(input: &[u8]) -> Result<T, String> {
    let value = parse(input)?;
    serde_json::from_value(value).map_err(|error| format!("invalid document: {error}"))
}

/// Parse untrusted host-format JSON without imposing Effect IR's integer-only
/// number model. Duplicate keys are still rejected so scanning never silently
/// overwrites a command-bearing field.
pub(crate) fn parse_host(input: &[u8]) -> Result<Value, String> {
    let mut deserializer = serde_json::Deserializer::from_slice(input);
    let value = HostValue::deserialize(&mut deserializer)
        .map_err(|error| format!("invalid JSON: {error}"))?;
    deserializer
        .end()
        .map_err(|error| format!("invalid JSON: {error}"))?;
    Ok(value.0)
}

struct StrictValue(Value);

impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(StrictValueVisitor)
    }
}

struct StrictValueVisitor;

struct HostValue(Value);

impl<'de> Deserialize<'de> for HostValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(HostValueVisitor)
    }
}

struct HostValueVisitor;

impl<'de> Visitor<'de> for HostValueVisitor {
    type Value = HostValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value without duplicate object keys")
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(HostValue(Value::Null))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(HostValue(Value::Null))
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(HostValue(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(HostValue(Value::Number(value.into())))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(HostValue(Value::Number(value.into())))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        serde_json::Number::from_f64(value)
            .map(Value::Number)
            .map(HostValue)
            .ok_or_else(|| E::custom("JSON number must be finite"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(HostValue(Value::String(value.to_owned())))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(HostValue(Value::String(value)))
    }

    fn visit_seq<A>(self, mut values: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut output = Vec::with_capacity(values.size_hint().unwrap_or_default());
        while let Some(value) = values.next_element::<HostValue>()? {
            output.push(value.0);
        }
        Ok(HostValue(Value::Array(output)))
    }

    fn visit_map<A>(self, mut fields: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut seen = BTreeSet::new();
        let mut output = serde_json::Map::new();
        while let Some(key) = fields.next_key::<String>()? {
            if !seen.insert(key.clone()) {
                return Err(de::Error::custom(format!("duplicate JSON key: {key}")));
            }
            let value = fields.next_value::<HostValue>()?;
            output.insert(key, value.0);
        }
        Ok(HostValue(Value::Object(output)))
    }
}

impl<'de> Visitor<'de> for StrictValueVisitor {
    type Value = StrictValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("JSON containing only signed 64-bit integer numbers")
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Null))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Null))
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Number(value.into())))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        let signed = i64::try_from(value)
            .map_err(|_| E::custom("JSON integer is outside signed 64-bit range"))?;
        Ok(StrictValue(Value::Number(signed.into())))
    }

    fn visit_f64<E>(self, _value: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Err(E::custom("floating-point JSON numbers are not supported"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::String(value.to_owned())))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::String(value)))
    }

    fn visit_seq<A>(self, mut values: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut output = Vec::with_capacity(values.size_hint().unwrap_or_default());
        while let Some(value) = values.next_element::<StrictValue>()? {
            output.push(value.0);
        }
        Ok(StrictValue(Value::Array(output)))
    }

    fn visit_map<A>(self, mut fields: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut seen = BTreeSet::new();
        let mut output = serde_json::Map::new();
        while let Some(key) = fields.next_key::<String>()? {
            if !seen.insert(key.clone()) {
                return Err(de::Error::custom(format!("duplicate JSON key: {key}")));
            }
            let value = fields.next_value::<StrictValue>()?;
            output.insert(key, value.0);
        }
        Ok(StrictValue(Value::Object(output)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    struct Sample {
        value: i64,
    }

    #[test]
    fn rejects_duplicate_keys_at_every_depth() {
        assert!(
            parse(br#"{"value":1,"value":2}"#)
                .unwrap_err()
                .contains("duplicate")
        );
        assert!(
            parse(br#"{"outer":{"x":1,"x":2}}"#)
                .unwrap_err()
                .contains("duplicate")
        );
    }

    #[test]
    fn typed_decode_rejects_unknown_fields() {
        let error = decode::<Sample>(br#"{"value":1,"future":true}"#).unwrap_err();
        assert!(error.contains("unknown field"), "{error}");
    }

    #[test]
    fn rejects_floats_and_trailing_documents() {
        assert!(parse(br#"{"value":1.5}"#).is_err());
        assert!(parse(br#"{"value":1} {"value":2}"#).is_err());
    }

    #[test]
    fn decodes_a_strict_document() {
        assert_eq!(
            decode::<Sample>(br#"{"value":-7}"#).unwrap(),
            Sample { value: -7 }
        );
    }

    #[test]
    fn host_json_allows_floats_but_still_rejects_duplicate_keys() {
        assert_eq!(parse_host(br#"{"value":1.5}"#).unwrap()["value"], 1.5);
        assert!(
            parse_host(br#"{"scripts":{"build":"one","build":"two"}}"#)
                .unwrap_err()
                .contains("duplicate")
        );
    }
}
