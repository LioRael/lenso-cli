use std::{fs, path::Path};

use serde_json::{Map, Value};

use crate::{AuthoringError, Module};

const SUPPORTED_SCHEMA_KEYWORDS: &[&str] = &[
    "additionalProperties",
    "const",
    "enum",
    "items",
    "properties",
    "required",
    "type",
    "x-lenso-sensitive",
];
const SCHEMA_METADATA_KEYWORDS: &[&str] = &[
    "$anchor",
    "$comment",
    "$id",
    "$schema",
    "default",
    "deprecated",
    "description",
    "examples",
    "readOnly",
    "title",
    "writeOnly",
];

pub(crate) fn validate_configuration(root: &Path, module: &Module) -> Result<(), AuthoringError> {
    if let Some(schema) = module.configuration_schema() {
        let path = root.join(schema);
        let schema: Value =
            serde_json::from_slice(&read_file(&path)?).map_err(|source| AuthoringError::Json {
                path: path.clone(),
                source,
            })?;
        validate_json_schema(
            module.configuration(),
            &schema,
            &format!("{}.configuration", module.key()),
        )?;
    } else if module.configuration() != &Value::Object(Map::new()) {
        return Err(AuthoringError::InvalidConfiguration {
            path: format!("{}.configuration", module.key()),
            detail: "non-empty configuration requires a schema so secret fields are explicit"
                .to_owned(),
        });
    }
    Ok(())
}

fn read_file(path: &Path) -> Result<Vec<u8>, AuthoringError> {
    fs::read(path).map_err(|source| AuthoringError::Io {
        path: path.to_owned(),
        source,
    })
}

fn is_secret_reference(value: &Value) -> bool {
    value.as_object().is_some_and(|object| {
        object.len() == 1 && object.get("secret_ref").is_some_and(Value::is_string)
    })
}

fn validate_json_schema(value: &Value, schema: &Value, path: &str) -> Result<(), AuthoringError> {
    let schema = schema
        .as_object()
        .ok_or_else(|| invalid_configuration(path, "configuration schema must be a JSON object"))?;
    if let Some(keyword) = schema.keys().find(|keyword| {
        !SUPPORTED_SCHEMA_KEYWORDS.contains(&keyword.as_str())
            && !SCHEMA_METADATA_KEYWORDS.contains(&keyword.as_str())
    }) {
        return Err(invalid_configuration(
            path,
            format!("unsupported JSON Schema keyword {keyword}"),
        ));
    }
    if schema
        .get("x-lenso-sensitive")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        if !is_secret_reference(value) {
            return Err(AuthoringError::SecretValue {
                path: path.to_owned(),
            });
        }
        return Ok(());
    }
    validate_schema_type(value, schema, path)?;
    validate_schema_value_constraints(value, schema, path)?;
    validate_schema_required(value, schema, path)?;
    validate_schema_properties(value, schema, path)?;
    validate_schema_items(value, schema, path)
}

fn invalid_configuration(path: &str, detail: impl Into<String>) -> AuthoringError {
    AuthoringError::InvalidConfiguration {
        path: path.to_owned(),
        detail: detail.into(),
    }
}

fn validate_schema_type(
    value: &Value,
    schema: &Map<String, Value>,
    path: &str,
) -> Result<(), AuthoringError> {
    if let Some(expected) = schema.get("type") {
        let expected = expected
            .as_str()
            .ok_or_else(|| invalid_configuration(path, "schema type must be a string"))?;
        let actual = match value {
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
        };
        let supported = [
            "array", "boolean", "integer", "null", "number", "object", "string",
        ];
        if !supported.contains(&expected) {
            return Err(invalid_configuration(
                path,
                format!("unsupported JSON Schema type {expected}"),
            ));
        }
        let integer = value
            .as_number()
            .is_some_and(|number| number.is_i64() || number.is_u64());
        if expected != actual && !(expected == "integer" && integer) {
            return Err(invalid_configuration(
                path,
                format!("expected {expected}, found {actual}"),
            ));
        }
    }
    Ok(())
}

fn validate_schema_value_constraints(
    value: &Value,
    schema: &Map<String, Value>,
    path: &str,
) -> Result<(), AuthoringError> {
    if let Some(expected) = schema.get("const")
        && value != expected
    {
        return Err(invalid_configuration(
            path,
            "value does not match schema const",
        ));
    }
    if let Some(values) = schema.get("enum") {
        let values = values
            .as_array()
            .ok_or_else(|| invalid_configuration(path, "schema enum must be an array"))?;
        if !values.iter().any(|expected| expected == value) {
            return Err(invalid_configuration(path, "value is not in schema enum"));
        }
    }
    Ok(())
}

fn validate_schema_required(
    value: &Value,
    schema: &Map<String, Value>,
    path: &str,
) -> Result<(), AuthoringError> {
    if let Some(required) = schema.get("required") {
        let required = required
            .as_array()
            .ok_or_else(|| invalid_configuration(path, "schema required must be an array"))?;
        let object = value
            .as_object()
            .ok_or_else(|| invalid_configuration(path, "required fields need an object"))?;
        for name in required {
            let name = name.as_str().ok_or_else(|| {
                invalid_configuration(path, "schema required entries must be strings")
            })?;
            if !object.contains_key(name) {
                return Err(invalid_configuration(
                    &format!("{path}.{name}"),
                    "required field is missing",
                ));
            }
        }
    }
    Ok(())
}

fn validate_schema_properties(
    value: &Value,
    schema: &Map<String, Value>,
    path: &str,
) -> Result<(), AuthoringError> {
    let empty_properties = Map::new();
    let properties = match schema.get("properties") {
        Some(properties) => properties
            .as_object()
            .ok_or_else(|| invalid_configuration(path, "schema properties must be an object"))?,
        None => &empty_properties,
    };
    let Some(object) = value.as_object() else {
        return Ok(());
    };
    for (name, child_schema) in properties {
        if let Some(child) = object.get(name) {
            validate_json_schema(child, child_schema, &format!("{path}.{name}"))?;
        }
    }
    let Some(additional) = schema.get("additionalProperties") else {
        return Ok(());
    };
    match additional {
        Value::Bool(true) => Ok(()),
        Value::Bool(false) => {
            for name in object.keys() {
                if !properties.contains_key(name) {
                    return Err(invalid_configuration(
                        &format!("{path}.{name}"),
                        "additional property is not allowed",
                    ));
                }
            }
            Ok(())
        }
        additional_schema => {
            for (name, child) in object {
                if !properties.contains_key(name) {
                    validate_json_schema(child, additional_schema, &format!("{path}.{name}"))?;
                }
            }
            Ok(())
        }
    }
}

fn validate_schema_items(
    value: &Value,
    schema: &Map<String, Value>,
    path: &str,
) -> Result<(), AuthoringError> {
    let Some(items) = schema.get("items") else {
        return Ok(());
    };
    if !items.is_object() {
        return Err(invalid_configuration(
            path,
            "schema items must be an object",
        ));
    }
    if let Some(values) = value.as_array() {
        for (index, child) in values.iter().enumerate() {
            validate_json_schema(child, items, &format!("{path}[{index}]"))?;
        }
    }
    Ok(())
}
