//! Native application scaffolding and reviewed toolchain invocations.
mod merge;
use extism_pdk::{FnResult, Json, host_fn};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[host_fn("extism:host/user")]
unsafe extern "ExtismHost" {
    fn read_file(path: String) -> String;
}
type Result<T> = std::result::Result<T, extism_pdk::Error>;
fn error(message: impl Into<String>) -> extism_pdk::Error {
    extism_pdk::Error::msg(message.into())
}
fn complete(output: Value) -> Json<Value> {
    Json(json!({"status":"complete","output":output}))
}
fn read(path: &str) -> Result<Option<String>> {
    // SAFETY: The PDK validates memory; the Host enforces the declared read grant.
    Ok(serde_json::from_str(&unsafe { read_file(path.into())? })?)
}
fn language(ctx: &Value) -> Result<Option<&str>> {
    let old = ctx["capabilities"]["extension.options"]["language"].as_str();
    let chosen = ctx["input"]["language"]
        .as_str()
        .or_else(|| ctx["answers"]["app:language"].as_str())
        .or(old);
    if let Some(chosen) = chosen {
        if !matches!(chosen, "typescript" | "rust") {
            return Err(error("choose typescript or rust"));
        }
        if old.is_some_and(|old| old != chosen) {
            return Err(error(
                "changing application language requires a new project target",
            ));
        }
    }
    Ok(chosen)
}
fn port(ctx: &Value) -> Result<u16> {
    let value = ctx["input"]
        .get("port")
        .or_else(|| ctx["capabilities"]["extension.options"].get("port"));
    value
        .map(|v| {
            v.as_u64()
                .and_then(|v| u16::try_from(v).ok())
                .ok_or_else(|| error("port must be 0..65535"))
        })
        .unwrap_or(Ok(3000))
}
fn command(language: &str, action: &str) -> Value {
    if language == "typescript" {
        let args = vec![
            "run",
            match action {
                "prepare" => {
                    return json!({"program":"npm","args":["install","--ignore-scripts","--no-audit","--no-fund"]});
                }
                "check" => "check",
                "migrate" => "migrate",
                _ => "start",
            },
        ];
        json!({"program":"npm","args":args})
    } else {
        let args = match action {
            "prepare" => vec!["fetch"],
            "check" => vec!["check"],
            "migrate" => vec!["run", "--release", "--", "--migrate-only"],
            _ => vec!["run", "--release"],
        };
        json!({"program":"cargo","args":args})
    }
}
const FILES: &[&str] = &[
    "package.json",
    "tsconfig.json",
    "Cargo.toml",
    "build.rs",
    "rust-toolchain.toml",
    "README.md",
    ".gitignore",
    ".env.example",
    "lenso.code.toml",
    "lenso/app.lock",
];
#[cfg_attr(not(test), extism_pdk::plugin_fn)]
pub fn configure(Json(ctx): Json<Value>) -> FnResult<Json<Value>> {
    let Some(lang) = language(&ctx)? else {
        return Ok(Json(
            json!({"status":"needs_input","question":{"id":"app:language","message":"Choose the application language","kind":{"type":"select","options":["typescript","rust"]}}}),
        ));
    };
    let port = port(&ctx)?;
    Ok(complete(
        json!({"installation":{"read_roots":["src"],"write_roots":["src"],"reads":FILES,"files":FILES,
        "processes":[command(lang,"prepare"),command(lang,"check"),command(lang,"run"),command(lang,"migrate")],
        "aliases":{format!("app run-{lang}"):["app","run"],format!("app migrate-{lang}"):["app","migrate"]},
        "disabled_commands":[format!("app run-{}",if lang=="rust"{"typescript"}else{"rust"}),format!("app migrate-{}",if lang=="rust"{"typescript"}else{"rust"})],
        "options":{"language":lang,"port":port}}}),
    ))
}
fn project_config(text: Option<&str>, lang: &str) -> Result<(String, String)> {
    let value: toml::Table = toml::from_str(text.unwrap_or(""))?;
    for key in ["project", "paths", "conventions"] {
        if value.get(key).is_some_and(|value| !value.is_table()) {
            return Err(error(format!("Code config {key} must be a table")));
        }
    }
    if value
        .get("paths")
        .and_then(|v| v.get("modules"))
        .is_some_and(|v| !v.is_str())
    {
        return Err(error("paths.modules must be a string"));
    }
    let mut doc = text.unwrap_or("").parse::<toml_edit::DocumentMut>()?;
    for (key, value) in [
        ("language", lang),
        ("runtime", if lang == "rust" { "tokio" } else { "node26" }),
    ] {
        if doc
            .get("project")
            .and_then(|v| v.get(key))
            .is_some_and(|v| v.as_str() != Some(value))
        {
            return Err(error(format!("project {key} conflicts with App target")));
        }
        doc["project"][key] = toml_edit::value(value);
    }
    if doc
        .get("schema_version")
        .is_some_and(|v| v.as_integer() != Some(1))
    {
        return Err(error("unsupported Code config schema"));
    }
    doc["schema_version"] = toml_edit::value(1);
    if doc
        .get("conventions")
        .and_then(|v| v.get("app_host"))
        .is_some_and(|v| v.as_str() != Some("lenso-http-v2"))
    {
        return Err(error(
            "existing app_host differs; configure a new App target",
        ));
    }
    doc["conventions"]["app_host"] = toml_edit::value("lenso-http-v2");
    let modules = doc
        .get("paths")
        .and_then(|v| v.get("modules"))
        .and_then(|v| v.as_str())
        .unwrap_or("src/modules")
        .to_owned();
    lenso_codegen::relative(&modules)?;
    if !modules.starts_with("src/") {
        return Err(error("App modules must remain below src"));
    }
    doc["paths"]["modules"] = toml_edit::value(&modules);
    if lang == "typescript"
        && doc
            .get("paths")
            .and_then(|v| v.get("import_alias"))
            .is_none()
    {
        doc["paths"]["import_alias"] = toml_edit::value("#modules");
    }
    Ok((doc.to_string(), modules))
}
fn scaffold(lang: &str, port: u16, config: Option<&str>) -> Result<BTreeMap<String, String>> {
    let (config, modules) = project_config(config, lang)?;
    let mut files = BTreeMap::from([
        ("lenso.code.toml".into(), config),
        (
            ".gitignore".into(),
            "node_modules/\ndist/\ntarget/\n.env\n*.sqlite*\ndata/\n".into(),
        ),
        (".env.example".into(), format!("PORT={port}\n")),
        (
            "README.md".into(),
            include_str!("templates/README.md").into(),
        ),
    ]);
    if lang == "typescript" {
        files.insert("package.json".into(),serde_json::to_string_pretty(&json!({"name":"lenso-app","private":true,"type":"module","scripts":{"build":"node --input-type=module -e \"import { rmSync } from 'node:fs'; rmSync('dist', { recursive: true, force: true });\" && tsc -p tsconfig.json","start":"npm run build && node dist/main.js","migrate":"npm run build && node dist/main.js --migrate-only","check":"tsc -p tsconfig.json --noEmit"},"dependencies":{"smol-toml":"^1"},"devDependencies":{"typescript":"^6","@types/node":"^26"},"imports":{"#modules/*":{"types":format!("./{modules}/*"),"default":format!("./dist/{}/*",modules.trim_start_matches("src/"))}}}))?+"\n");
        files.insert("tsconfig.json".into(),serde_json::to_string_pretty(&json!({"compilerOptions":{"target":"ES2024","module":"NodeNext","moduleResolution":"NodeNext","strict":true,"types":["node"],"noEmitOnError":true,"noUnusedLocals":true,"noUnusedParameters":true,"verbatimModuleSyntax":true,"rootDir":"src","outDir":"dist"},"include":["src/**/*.ts"]}))?+"\n");
        files.insert(
            "src/lenso-host.ts".into(),
            include_str!("templates/lenso-host.ts").into(),
        );
        files.insert(
            "src/main.ts".into(),
            include_str!("templates/main.ts").replace("__APP_PORT__", &port.to_string()),
        );
    } else {
        files.insert("Cargo.toml".into(),"[package]\nname=\"lenso-app-consumer\"\nversion=\"0.1.0\"\nedition=\"2024\"\nrust-version=\"1.99\"\n[workspace]\n[dependencies]\naxum=\"0.8\"\nserde_json=\"1\"\ndotenvy=\"0.15\"\ntokio={version=\"1\",features=[\"fs\",\"io-util\",\"macros\",\"net\",\"rt\",\"rt-multi-thread\",\"signal\",\"sync\",\"time\"]}\n[build-dependencies]\ntoml=\"1\"\n".into());
        files.insert(
            "rust-toolchain.toml".into(),
            "[toolchain]\nchannel=\"1.99.0\"\nprofile=\"minimal\"\n".into(),
        );
        files.insert(
            "src/main.rs".into(),
            include_str!("templates/main.rs").replace("__APP_PORT__", &port.to_string()),
        );
        files.insert(
            "src/lenso_host.rs".into(),
            include_str!("templates/lenso_host.rs").into(),
        );
        files.insert("build.rs".into(), include_str!("templates/build.rs").into());
    }
    Ok(files)
}
#[cfg_attr(not(test), extism_pdk::plugin_fn)]
pub fn bootstrap(Json(ctx): Json<Value>) -> FnResult<Json<Value>> {
    bootstrap_context(ctx)
}
fn bootstrap_context(ctx: Value) -> FnResult<Json<Value>> {
    let lang = language(&ctx)?.ok_or_else(|| error("configure App language first"))?;
    if receipt_ready(lang)? {
        return Ok(complete(
            json!({"files":[],"commands":[],"message":"App is initialized; application source is user-owned"}),
        ));
    }
    if read(if lang == "rust" {
        "package.json"
    } else {
        "Cargo.toml"
    })?
    .is_some()
    {
        return Err(error(
            "App bootstrap requires an empty native target; another language manifest exists",
        )
        .into());
    }
    let mut files = Vec::new();
    for (path, content) in scaffold(lang, port(&ctx)?, read("lenso.code.toml")?.as_deref())? {
        let current = read(&path)?;
        if path != "lenso.code.toml" && current.as_ref().is_some_and(|s| s != &content) {
            return Err(error(format!("Refusing to overwrite existing {path}; App bootstrap requires an empty native target")).into());
        }
        if current.as_ref() != Some(&content) {
            files.push(json!({"path":path,"content":content}));
        }
    }
    let receipt =
        format!("schema_version=1\nlanguage={lang:?}\nversion=\"0.2.0\"\nstate=\"ready\"\n");
    files.push(json!({"path":"lenso/app.lock","content":receipt.replace("state=\"ready\"","state=\"pending\"")}));
    Ok(complete(
        json!({"files":files,"commands":[command(lang,"prepare")],"finalizations":[{"path":"lenso/app.lock","content":receipt}],"message":"Create native App files and prepare dependencies; run lenso app check or lenso app run"}),
    ))
}
fn receipt_ready(lang: &str) -> Result<bool> {
    let Some(text) = read("lenso/app.lock")? else {
        return Ok(false);
    };
    let receipt: toml::Table = toml::from_str(&text)?;
    if receipt.get("language").and_then(toml::Value::as_str) != Some(lang)
        || receipt
            .get("schema_version")
            .and_then(toml::Value::as_integer)
            != Some(1)
    {
        return Err(error("App receipt does not match this target"));
    }
    match receipt.get("state").and_then(toml::Value::as_str) {
        Some("ready") => Ok(true),
        Some("pending") => Ok(false),
        _ => Err(error("Invalid App receipt state")),
    }
}
fn native_action(ctx: Value, action: &str) -> FnResult<Json<Value>> {
    let lang = language(&ctx)?.ok_or_else(|| error("configure App language first"))?;
    if !receipt_ready(lang)? {
        if action == "prepare" {
            return bootstrap_context(ctx);
        }
        return Err(error("initialize or retry App with lenso app init first").into());
    }
    Ok(complete(
        json!({"files":[],"commands":[command(lang,action)]}),
    ))
}
#[cfg_attr(not(test), extism_pdk::plugin_fn)]
pub fn prepare(Json(ctx): Json<Value>) -> FnResult<Json<Value>> {
    native_action(ctx, "prepare")
}
#[cfg_attr(not(test), extism_pdk::plugin_fn)]
pub fn check(Json(ctx): Json<Value>) -> FnResult<Json<Value>> {
    native_action(ctx, "check")
}
fn authorize(ctx: Value, action: &str) -> FnResult<Json<Value>> {
    let lang = language(&ctx)?.ok_or_else(|| error("configure App first"))?;
    if !receipt_ready(lang)? {
        return Err(error("initialize App first").into());
    }
    let id = format!(
        "app:{action}:{lang}:{}",
        ctx["project"]
            .as_str()
            .ok_or_else(|| error("project context is missing"))?
    );
    match ctx["answers"][&id].as_bool() {
        Some(true) => Ok(complete(json!({"command":command(lang,action)}))),
        Some(false) => Err(error("Application command declined").into()),
        None => Ok(Json(
            json!({"status":"needs_input","question":{"id":id,"message":format!("Authorize native application command {}? Project scripts and module code will execute.",command(lang,action)),"kind":{"type":"confirm"}}}),
        )),
    }
}
#[cfg_attr(not(test), extism_pdk::plugin_fn)]
pub fn authorize_run(Json(ctx): Json<Value>) -> FnResult<Json<Value>> {
    authorize(ctx, "run")
}
#[cfg_attr(not(test), extism_pdk::plugin_fn)]
pub fn authorize_migrate(Json(ctx): Json<Value>) -> FnResult<Json<Value>> {
    authorize(ctx, "migrate")
}

#[cfg_attr(not(test), extism_pdk::plugin_fn)]
pub fn upgrade(Json(ctx): Json<Value>) -> FnResult<Json<Value>> {
    let lang = language(&ctx)?.ok_or_else(|| error("configure App language first"))?;
    if !receipt_ready(lang)? {
        return Err(error("initialize App first").into());
    }
    let old_receipt = read("lenso/app.lock")?.ok_or_else(|| error("missing App receipt"))?;
    let receipt: toml::Table = toml::from_str(&old_receipt)?;
    if receipt.get("version").and_then(toml::Value::as_str) == Some("0.2.0") {
        return Ok(complete(
            json!({"files":[],"commands":[],"message":"App host is already current"}),
        ));
    }
    if receipt.get("version").and_then(toml::Value::as_str) != Some("0.1.0") {
        return Err(error("unsupported App upgrade baseline").into());
    }
    let config = read("lenso.code.toml")?.ok_or_else(|| error("missing Code config"))?;
    let mut doc = config.parse::<toml_edit::DocumentMut>()?;
    if doc["conventions"]["app_host"].as_str() != Some("lenso-http-v1") {
        return Err(error("review the existing application host before upgrading").into());
    }
    doc["conventions"]["app_host"] = toml_edit::value("lenso-http-v2");
    let desired = scaffold(lang, port(&ctx)?, Some(&doc.to_string()))?;
    let legacy = if lang == "typescript" {
        vec![("src/main.ts", include_str!("legacy/main.ts"))]
    } else {
        vec![
            ("src/main.rs", include_str!("legacy/main.rs")),
            ("build.rs", include_str!("legacy/build.rs")),
        ]
    };
    let mut files = Vec::new();
    for (path, base) in legacy {
        let current = read(path)?.ok_or_else(|| {
            error(format!(
                "Missing {path}; restore or upgrade the host manually"
            ))
        })?;
        let base = base.replace("__APP_PORT__", &port(&ctx)?.to_string());
        let next = desired
            .get(path)
            .ok_or_else(|| error("missing upgrade template"))?;
        let content = if let Some(merged) = merge::merge(&base, &current, next) {
            merged
        } else {
            let id = format!(
                "app:upgrade:{path}:{:x}",
                Sha256::digest(format!("{current}\0{next}").as_bytes())
            );
            match ctx["answers"][&id].as_str() {
                Some("replace") => next.clone(),
                Some("cancel") => return Err(error("App upgrade cancelled").into()),
                Some(_) => return Err(error("choose replace or cancel").into()),
                None => {
                    return Ok(Json(
                        json!({"status":"needs_input","question":{"id":id,"message":format!("App host conflict in {path}. Replacing discards overlapping edits; cancel to merge manually."),"kind":{"type":"select","options":["cancel","replace"]}}}),
                    ));
                }
            }
        };
        if content != current {
            files.push(json!({"path":path,"content":content}));
        }
    }
    let host_path = if lang == "typescript" {
        "src/lenso-host.ts"
    } else {
        "src/lenso_host.rs"
    };
    let content = &desired[host_path];
    if read(host_path)?.is_some_and(|current| current != *content) {
        return Err(error("native host protocol file already exists with user changes").into());
    }
    files.push(json!({"path":host_path,"content":content}));
    if lang == "rust" {
        let text = read("Cargo.toml")?.ok_or_else(|| error("missing Cargo.toml"))?;
        let mut manifest = text.parse::<toml_edit::DocumentMut>()?;
        let dependency = manifest
            .get_mut("dependencies")
            .and_then(|v| v.get_mut("tokio"))
            .ok_or_else(|| error("missing Tokio dependency"))?;
        if let Some(version) = dependency.as_str().map(str::to_owned) {
            let mut table = toml_edit::InlineTable::new();
            table.insert("version", version.into());
            *dependency = toml_edit::Item::Value(table.into());
        }
        let table = dependency
            .as_table_like_mut()
            .ok_or_else(|| error("review Tokio dependency options manually"))?;
        let mut features: std::collections::BTreeSet<String> = table
            .get("features")
            .and_then(|v| v.as_array())
            .map(|v| {
                v.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        features.extend(["fs".into(), "io-util".into()]);
        let mut array = toml_edit::Array::new();
        for feature in features {
            array.push(feature);
        }
        table.insert("features", toml_edit::value(array));
        let content = manifest.to_string();
        if content != text {
            files.push(json!({"path":"Cargo.toml","content":content}));
        }
    }
    let mut ignored = read(".gitignore")?.unwrap_or_default();
    if !ignored
        .lines()
        .any(|line| line == "data/" || line == "/data/")
    {
        if !ignored.ends_with('\n') {
            ignored.push('\n');
        }
        ignored.push_str("data/\n");
        files.push(json!({"path":".gitignore","content":ignored}));
    }
    files.push(json!({"path":"lenso.code.toml","content":desired["lenso.code.toml"]}));
    files.push(json!({"path":"lenso/app.lock","content":format!("schema_version=1\nlanguage={lang:?}\nversion=\"0.2.0\"\nstate=\"ready\"\n")}));
    Ok(complete(
        json!({"files":files,"commands":[],"message":"Upgrade the native host. Review Auth update with reconfigure before adding identity-dependent modules."}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn project_conventions_are_preserved_and_languages_cannot_change() {
        let (text,path)=project_config(Some("# retained\n[paths]\nmodules='src/features'\n[modules.auth.options]\nttl_ms=120000\n"),"rust").unwrap();
        assert!(text.contains("# retained"));
        assert_eq!(path, "src/features");
        assert!(text.contains("120000"));
        assert!(project_config(Some(&text), "typescript").is_err());
    }
    #[test]
    fn paths_and_host_conflicts_fail_closed() {
        for text in [
            "[paths]\nmodules='../escape'",
            "[paths]\nmodules='lib/modules'",
            "[conventions]\napp_host='other'",
        ] {
            assert!(project_config(Some(text), "rust").is_err());
        }
    }
    #[test]
    fn confirmation_does_not_select_a_language() {
        assert!(language(&json!({"input":{"yes":true}})).unwrap().is_none());
    }
    #[test]
    fn scaffolds_have_native_manifests_and_no_auth_implementation() {
        for language in ["rust", "typescript"] {
            let files = scaffold(language, 0, None).unwrap();
            assert!(files.contains_key(if language == "rust" {
                "Cargo.toml"
            } else {
                "package.json"
            }));
            assert!(
                !files
                    .values()
                    .any(|value| value.contains("SqliteAuthStore"))
            );
        }
    }
}
