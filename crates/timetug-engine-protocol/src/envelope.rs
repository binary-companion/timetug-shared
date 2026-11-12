//! Port of `Envelope.swift` (`JSONValue.swift` is not ported: a JSON document is `serde_json::Value`).

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

/// `Int` as Foundation's `JSONDecoder` reads it: a JSON integer, or a number with no fraction that fits (`1.0`, `1e2`).
pub(crate) fn value_as_i64(value: &Value) -> Option<i64> {
    let Value::Number(n) = value else { return None };
    if let Some(i) = n.as_i64() {
        return Some(i);
    }
    let f = n.as_f64()?;
    // `i64::MAX as f64` is 2^63, which is itself out of range, hence `<`.
    if f.fract() == 0.0 && f >= i64::MIN as f64 && f < i64::MAX as f64 {
        return Some(f as i64);
    }
    None
}

/// `UInt64` as Foundation's `JSONDecoder` reads it (see [`value_as_i64`]).
pub(crate) fn value_as_u64(value: &Value) -> Option<u64> {
    let Value::Number(n) = value else { return None };
    if let Some(u) = n.as_u64() {
        return Some(u);
    }
    let f = n.as_f64()?;
    if f.fract() == 0.0 && f >= 0.0 && f < u64::MAX as f64 {
        return Some(f as u64);
    }
    None
}

/// A command id chosen by the host (number or string). The engine echoes it on any diagnostic the command causes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum MessageId {
    Number(u64),
    String(String),
}

impl MessageId {
    /// A number that is a `UInt64`, else a string, else `None` (the Swift decoder throws).
    pub fn from_value(value: &Value) -> Option<MessageId> {
        if let Some(n) = value_as_u64(value) {
            return Some(MessageId::Number(n));
        }
        value.as_str().map(|s| MessageId::String(s.to_string()))
    }
}

impl Serialize for MessageId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            MessageId::Number(n) => serializer.serialize_u64(*n),
            MessageId::String(s) => serializer.serialize_str(s),
        }
    }
}

impl<'de> Deserialize<'de> for MessageId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<MessageId, D::Error> {
        let value = Value::deserialize(deserializer)?;
        MessageId::from_value(&value)
            .ok_or_else(|| serde::de::Error::custom("an id is a number or a string"))
    }
}

/// Why a text is not an envelope. The hosts and the session map every case to one diagnostic.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum EnvelopeError {
    #[error("not JSON")]
    NotJson,
    #[error("not a JSON object")]
    NotAnObject,
    #[error("member `{0}` is missing or has the wrong type")]
    Member(&'static str),
}

/// `{"v":1,"type":...,"id":...,"payload":...}`. Unknown fields are ignored on decode (adding a field is not breaking).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Envelope {
    pub v: i64,
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<MessageId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
}

impl Envelope {
    /// Swift's `init(v: 1, type:, id: nil, payload: nil)`.
    pub fn new(message_type: impl Into<String>) -> Envelope {
        Envelope {
            v: 1,
            r#type: message_type.into(),
            id: None,
            payload: None,
        }
    }

    #[must_use]
    pub fn with_id(mut self, id: MessageId) -> Envelope {
        self.id = Some(id);
        self
    }

    #[must_use]
    pub fn with_payload(mut self, payload: Value) -> Envelope {
        self.payload = Some(payload);
        self
    }

    /// Swift's `init(from:)`: `v` and `type` are required; `id` and `payload` are optional, a JSON `null` payload and an
    /// absent one both mean "no payload", and an `id` that is neither a number nor a string fails the whole decode.
    pub fn from_value(value: &Value) -> Result<Envelope, EnvelopeError> {
        let object = value.as_object().ok_or(EnvelopeError::NotAnObject)?;
        let v = object
            .get("v")
            .and_then(value_as_i64)
            .ok_or(EnvelopeError::Member("v"))?;
        let message_type = object
            .get("type")
            .and_then(Value::as_str)
            .ok_or(EnvelopeError::Member("type"))?
            .to_string();
        let id = match object.get("id") {
            None | Some(Value::Null) => None,
            Some(other) => Some(MessageId::from_value(other).ok_or(EnvelopeError::Member("id"))?),
        };
        let payload = match object.get("payload") {
            None | Some(Value::Null) => None,
            Some(other) => Some(other.clone()),
        };
        Ok(Envelope {
            v,
            r#type: message_type,
            id,
            payload,
        })
    }

    /// Parses UTF-8 JSON text.
    pub fn parse(text: &str) -> Result<Envelope, EnvelopeError> {
        let value: Value = serde_json::from_str(text).map_err(|_| EnvelopeError::NotJson)?;
        Envelope::from_value(&value)
    }

    /// Parses JSON bytes; bytes that are not UTF-8 are "not JSON".
    pub fn from_slice(bytes: &[u8]) -> Result<Envelope, EnvelopeError> {
        let value: Value = serde_json::from_slice(bytes).map_err(|_| EnvelopeError::NotJson)?;
        Envelope::from_value(&value)
    }

    /// The envelope as a JSON value (`v`, `type`, then `id` and `payload` when present).
    pub fn to_value(&self) -> Value {
        let mut object = serde_json::Map::new();
        object.insert("v".into(), Value::from(self.v));
        object.insert("type".into(), Value::from(self.r#type.clone()));
        if let Some(id) = &self.id {
            object.insert(
                "id".into(),
                match id {
                    MessageId::Number(n) => Value::from(*n),
                    MessageId::String(s) => Value::from(s.clone()),
                },
            );
        }
        if let Some(payload) = &self.payload {
            object.insert("payload".into(), payload.clone());
        }
        Value::Object(object)
    }

    /// Compact JSON text.
    pub fn to_json(&self) -> String {
        // Serializing a `Value` cannot fail (every key is a string).
        serde_json::to_string(&self.to_value()).unwrap_or_default()
    }
}

impl<'de> Deserialize<'de> for Envelope {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Envelope, D::Error> {
        let value = Value::deserialize(deserializer)?;
        Envelope::from_value(&value).map_err(serde::de::Error::custom)
    }
}

pub enum ProtocolVersion {}

impl ProtocolVersion {
    pub const SUPPORTED: [i64; 1] = [1];

    pub fn is_supported(v: i64) -> bool {
        Self::SUPPORTED.contains(&v)
    }
}
