use extism_pdk::{FnResult, Json, plugin_fn};
use serde_json::{Value, json};

#[plugin_fn]
pub fn greeting(Json(ctx): Json<Value>) -> FnResult<Json<Value>> {
    if ctx["abi_version"] != 1 {
        return Err(extism_pdk::Error::msg("unsupported Lenso ABI").into());
    }
    let name = ctx["input"]["name"]
        .as_str()
        .or_else(|| ctx["answers"]["name"].as_str());
    let Some(name) = name.filter(|s| !s.trim().is_empty()) else {
        return Ok(Json(json!({"status":"needs_input", "question":{
            "id":"name", "message":"Choose a greeting recipient",
            "kind":{"type":"select", "options":["World", "Rust", "Wasm"]}
        }})));
    };
    Ok(Json(json!({"status":"complete", "output":{
        "files":[{"path":"greeting.txt", "content":format!("Hello, {name}!\n")}]
    }})))
}

#[plugin_fn]
pub fn validate_plan(Json(ctx): Json<Value>) -> FnResult<Json<Value>> {
    if !ctx["hooks"]["result"]["plan"]["files"].is_array() {
        return Err(extism_pdk::Error::msg("native plan output must contain files").into());
    }
    Ok(Json(
        json!({"status":"complete", "output":{"validated_by":"Wasm plan validator"}}),
    ))
}
