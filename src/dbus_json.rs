//! D-Bus bodies as JSON text, read against their signature.
//!
//! Copied from `cprussin/domicile` (`packages/domicile-host/src/dbus_json.rs`),
//! MIT License — Copyright (c) Connor Prussin (https://github.com/cprussin/domicile).
//!
//! A body is a JSON array with one element per complete type in its
//! signature:
//!
//! | D-Bus | JSON |
//! |---|---|
//! | `y n q i u x t` | an integer in the type's range |
//! | `d` | a number |
//! | `b` | a boolean |
//! | `s o g` | a string; `o` must be an object path and `g` a signature |
//! | `v` | `{"signature": "<one type>", "value": <that type>}` |
//! | `a{KV}` | an object; keys that are not strings are their JSON text |
//! | `aT` | an array |
//! | `(…)` | an array of the fields |
//! | `h` | refused when read; written as `null`, since a page cannot hold one |
//!
//! The SDK's `system` module writes and reads these. See
//! `docs/SHELL-SYSTEM-ACCESS.md`.

use serde_json::{Map, Number, Value as Json};
use zbus::zvariant::{Array, Dict, ObjectPath, Signature, Str, StructureBuilder, Value};

/// JSON text that does not fit its signature.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("a D-Bus body that does not fit its signature: {0}")]
pub struct NotABody(String);

/// The values `body` holds, one per complete type in `signature`.
pub fn read(signature: &str, body: &str) -> Result<Vec<Value<'static>>, NotABody> {
    let types = types(signature)?;
    let Json::Array(elements) =
        serde_json::from_str(body).map_err(|err| NotABody(err.to_string()))?
    else {
        return Err(NotABody("the body is not a JSON array".into()));
    };
    if elements.len() != types.len() {
        return Err(NotABody(format!(
            "{} values for {} types",
            elements.len(),
            types.len()
        )));
    }
    types
        .iter()
        .zip(&elements)
        .map(|(of, json)| value(of, json))
        .collect()
}

/// `values` as a body's JSON text.
pub fn written(values: &[Value<'_>]) -> String {
    Json::Array(values.iter().map(json).collect()).to_string()
}

/// The complete types in `signature`, in order.
fn types(signature: &str) -> Result<Vec<Signature>, NotABody> {
    if signature.is_empty() {
        return Ok(Vec::new());
    }
    match format!("({signature})").parse::<Signature>() {
        Ok(Signature::Structure(fields)) => Ok(fields.iter().cloned().collect()),
        _ => Err(NotABody(format!("{signature} is not a body signature"))),
    }
}

fn value(of: &Signature, json: &Json) -> Result<Value<'static>, NotABody> {
    let wrong = || NotABody(format!("{json} is not a {of}"));
    Ok(match of {
        Signature::U8 => Value::U8(integer(json).ok_or_else(wrong)?),
        Signature::I16 => Value::I16(integer(json).ok_or_else(wrong)?),
        Signature::U16 => Value::U16(integer(json).ok_or_else(wrong)?),
        Signature::I32 => Value::I32(integer(json).ok_or_else(wrong)?),
        Signature::U32 => Value::U32(integer(json).ok_or_else(wrong)?),
        Signature::I64 => Value::I64(json.as_i64().ok_or_else(wrong)?),
        Signature::U64 => Value::U64(json.as_u64().ok_or_else(wrong)?),
        Signature::F64 => Value::F64(json.as_f64().ok_or_else(wrong)?),
        Signature::Bool => Value::Bool(json.as_bool().ok_or_else(wrong)?),
        Signature::Str => Value::Str(Str::from(json.as_str().ok_or_else(wrong)?.to_string())),
        Signature::ObjectPath => Value::ObjectPath(
            ObjectPath::try_from(json.as_str().ok_or_else(wrong)?.to_string())
                .map_err(|_| wrong())?,
        ),
        Signature::Signature => Value::Signature(
            json.as_str()
                .ok_or_else(wrong)?
                .parse()
                .map_err(|_| wrong())?,
        ),
        Signature::Variant => {
            let inner = json.as_object().ok_or_else(wrong)?;
            let of: Signature = inner
                .get("signature")
                .and_then(Json::as_str)
                .ok_or_else(wrong)?
                .parse()
                .map_err(|_| wrong())?;
            let held = inner.get("value").ok_or_else(wrong)?;
            Value::Value(Box::new(value(&of, held)?))
        }
        Signature::Array(element) => {
            let mut array = Array::new(element);
            for item in json.as_array().ok_or_else(wrong)? {
                array
                    .append(value(element, item)?)
                    .map_err(|err| NotABody(err.to_string()))?;
            }
            Value::Array(array)
        }
        Signature::Dict { key, value: held } => {
            let mut dict = Dict::new(key, held);
            for (name, item) in json.as_object().ok_or_else(wrong)? {
                dict.append(self::key(key, name)?, value(held, item)?)
                    .map_err(|err| NotABody(err.to_string()))?;
            }
            Value::Dict(dict)
        }
        Signature::Structure(fields) => {
            let items = json.as_array().ok_or_else(wrong)?;
            if items.len() != fields.len() {
                return Err(wrong());
            }
            fields
                .iter()
                .zip(items)
                .try_fold(StructureBuilder::new(), |built, (field, item)| {
                    Ok::<_, NotABody>(built.append_field(value(field, item)?))
                })?
                .build()
                .map(Value::Structure)
                .map_err(|err| NotABody(err.to_string()))?
        }
        Signature::Unit | Signature::Fd => return Err(wrong()),
    })
}

/// A dictionary key, from the JSON object key that names it.
fn key(of: &Signature, name: &str) -> Result<Value<'static>, NotABody> {
    match of {
        Signature::Str | Signature::ObjectPath | Signature::Signature => {
            value(of, &Json::String(name.to_string()))
        }
        _ => value(
            of,
            &serde_json::from_str(name).map_err(|_| NotABody(format!("{name} is not a {of}")))?,
        ),
    }
}

/// A JSON integer that fits `T`.
fn integer<T: TryFrom<i64>>(json: &Json) -> Option<T> {
    json.as_i64().and_then(|whole| T::try_from(whole).ok())
}

fn json(value: &Value<'_>) -> Json {
    match value {
        Value::U8(number) => Json::from(*number),
        Value::Bool(truth) => Json::from(*truth),
        Value::I16(number) => Json::from(*number),
        Value::U16(number) => Json::from(*number),
        Value::I32(number) => Json::from(*number),
        Value::U32(number) => Json::from(*number),
        Value::I64(number) => Json::from(*number),
        Value::U64(number) => Json::from(*number),
        // NaN and the infinities have no JSON; D-Bus services do not send them.
        Value::F64(number) => Number::from_f64(*number).map_or(Json::Null, Json::Number),
        Value::Str(text) => Json::from(text.as_str()),
        Value::Signature(signature) => Json::from(signature.to_string()),
        Value::ObjectPath(path) => Json::from(path.as_str()),
        Value::Value(held) => Json::Object(Map::from_iter([
            (
                "signature".to_string(),
                Json::from(held.value_signature().to_string()),
            ),
            ("value".to_string(), json(held)),
        ])),
        Value::Array(array) => Json::Array(array.inner().iter().map(json).collect()),
        Value::Dict(dict) => Json::Object(
            dict.iter()
                .map(|(key, held)| (key_text(key), json(held)))
                .collect(),
        ),
        Value::Structure(structure) => Json::Array(structure.fields().iter().map(json).collect()),
        Value::Fd(_) => Json::Null,
    }
}

/// A dictionary key as a JSON object key: a string as is, anything else as
/// its JSON text.
fn key_text(key: &Value<'_>) -> String {
    match json(key) {
        Json::String(text) => text,
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_roundtrip_json(signature: &str, body_json: &str) {
        let values = read(signature, body_json)
            .unwrap_or_else(|e| panic!("read({signature:?}, {body_json}) failed: {e}"));
        let reencoded = written(&values);
        let expected: Json = serde_json::from_str(body_json).unwrap();
        let actual: Json = serde_json::from_str(&reencoded).unwrap();
        assert_eq!(actual, expected, "round-trip mismatch for {signature:?}");
    }

    #[test]
    fn roundtrips_empty_body() {
        let values = read("", "[]").unwrap();
        assert!(values.is_empty());
        assert_eq!(written(&values), "[]");
    }

    #[test]
    fn roundtrips_primitives() {
        assert_roundtrip_json(
            "ynqiuxtdbsog",
            r#"[255,-32768,65535,-2147483648,4294967295,-9223372036854775808,18446744073709551615,3.5,true,"hello","/org/example/Obj","a{sv}"]"#,
        );

        let vals = read("yisob", r#"[42,-7,"hi","/a/b",false]"#).unwrap();
        assert_eq!(vals.len(), 5);
        assert!(matches!(vals[0], Value::U8(42)));
        assert!(matches!(vals[1], Value::I32(-7)));
        assert!(matches!(vals[2], Value::Str(ref s) if s.as_str() == "hi"));
        assert!(matches!(vals[3], Value::ObjectPath(ref p) if p.as_str() == "/a/b"));
        assert!(matches!(vals[4], Value::Bool(false)));
    }

    #[test]
    fn roundtrips_variants() {
        assert_roundtrip_json(
            "vvv",
            r#"[{"signature":"u","value":42},{"signature":"s","value":"nested"},{"signature":"v","value":{"signature":"b","value":true}}]"#,
        );
    }

    #[test]
    fn roundtrips_dicts_structs_and_arrays() {
        assert_roundtrip_json(
            "a{sv}",
            r#"[{"count":{"signature":"i","value":-5},"enabled":{"signature":"b","value":true},"tags":{"signature":"as","value":["x","y"]}}]"#,
        );
        assert_roundtrip_json("a{is}", r#"[{"1":"one","-2":"minus two"}]"#);
        assert_roundtrip_json("(us)", r#"[[7,"seven"]]"#);
        assert_roundtrip_json("as", r#"[["alpha","beta",""]]"#);
        assert_roundtrip_json("as", r#"[[]]"#);
    }

    #[test]
    fn rejects_mismatched_arity_and_malformed_bodies() {
        assert!(read("us", "[1]").is_err());
        assert!(read("u", "[1, 2]").is_err());
        assert!(read("", "[1]").is_err());
        assert!(read("(us)", "[[1]]").is_err());
        assert!(read("(us)", "[[1, \"ok\", true]]").is_err());
        assert!(read("u", "1").is_err());
        assert!(read("u", "{not json").is_err());
        assert!(read("(", "[]").is_err());
    }

    #[test]
    fn rejects_out_of_range_integers() {
        assert!(read("y", "[256]").is_err());
        assert!(read("y", "[-1]").is_err());
        assert!(read("n", "[32768]").is_err());
        assert!(read("q", "[-1]").is_err());
        assert!(read("u", "[-1]").is_err());
    }

    #[test]
    fn rejects_invalid_object_paths_and_signatures() {
        assert!(read("o", r#"["relative/path"]"#).is_err());
        assert!(read("o", r#"["/trailing/"]"#).is_err());
        assert!(read("o", r#"[""]"#).is_err());
        assert!(read("g", r#"["(unclosed"]"#).is_err());
    }

    #[test]
    fn refuses_fd_on_read() {
        assert!(read("h", "[0]").is_err());
        assert!(read("h", "[null]").is_err());
        assert!(read("(uh)", "[[1, 0]]").is_err());
    }
}
