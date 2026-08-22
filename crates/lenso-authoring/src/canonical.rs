use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

pub(crate) fn canonical_pretty_json<T: Serialize>(value: &T) -> Vec<u8> {
    let mut value = serde_json::to_value(value).expect("authoring values are serializable");
    sort_json_value(&mut value);
    let mut bytes = serde_json::to_vec_pretty(&value).expect("authoring values are serializable");
    bytes.push(b'\n');
    bytes
}

pub(crate) fn canonical_json_bytes<T: Serialize>(value: &T) -> Vec<u8> {
    serde_json::to_vec(&canonical_value(
        serde_json::to_value(value).expect("authoring values are serializable"),
    ))
    .expect("authoring values are serializable")
}

pub(crate) fn canonical_json_string(value: &Value) -> String {
    serde_json::to_string(&canonical_value(value.clone())).expect("JSON values are serializable")
}

fn canonical_value(value: Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.into_iter().map(canonical_value).collect()),
        Value::Object(object) => {
            let mut sorted = BTreeMap::new();
            for (key, value) in object {
                sorted.insert(key, canonical_value(value));
            }
            Value::Object(sorted.into_iter().collect())
        }
        value => value,
    }
}

pub(crate) fn sort_json_value(value: &mut Value) {
    *value = canonical_value(value.take());
}
