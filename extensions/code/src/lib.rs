//! Optional repository code distribution. The Host owns transport, pins and permissions.
use extism_pdk::{FnResult, Json, host_fn, plugin_fn};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;
type Result<T> = std::result::Result<T, extism_pdk::Error>;
#[host_fn("extism:host/user")]
unsafe extern "ExtismHost" {
    fn source_resolve(request: String) -> String;
    fn source_read(request: String) -> String;
    fn read_file(path: String) -> String;
}
fn error(message: impl Into<String>) -> extism_pdk::Error {
    extism_pdk::Error::msg(message.into())
}
fn complete(output: Value) -> Json<Value> {
    Json(json!({"status":"complete","output":output}))
}
fn question(id: &str, message: &str, kind: Value) -> Json<Value> {
    Json(json!({"status":"needs_input","question":{"id":id,"message":message,"kind":kind}}))
}
fn local(path: &str) -> Result<Option<String>> {
    // SAFETY: Extism validates the string memory handles.
    Ok(serde_json::from_str(&unsafe { read_file(path.into())? })?)
}
fn remote(handle: &str, path: &str) -> Result<String> {
    // SAFETY: Extism validates the string memory handles; the Host validates the handle.
    unsafe { source_read(json!({"handle":handle,"path":path}).to_string()) }
}
fn relative(path: &str) -> Result<()> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains(['\\', ':', '\0'])
        || path
            .split('/')
            .any(|p| p.is_empty() || matches!(p, "." | ".." | ".git" | ".lenso"))
    {
        return Err(error("resource paths must be relative without traversal"));
    }
    Ok(())
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    source: String,
    #[serde(default = "head")]
    r#ref: String,
    #[serde(default = "catalog_path")]
    path: String,
    #[serde(default = "format")]
    format: String,
    #[serde(default)]
    target: String,
    #[serde(default)]
    items: Vec<Item>,
}
fn head() -> String {
    "HEAD".into()
}
fn catalog_path() -> String {
    "catalog.toml".into()
}
fn format() -> String {
    "lenso-code-v1".into()
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    schema_version: u32,
    #[serde(default)]
    name: String,
    items: Vec<Item>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Item {
    name: String,
    #[serde(default)]
    description: String,
    files: Vec<ResourceFile>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(untagged)]
enum ResourceFile {
    Path(String),
    Target(File),
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct File {
    path: String,
    target: String,
}
fn registry(ctx: &Value) -> Result<std::result::Result<String, Json<Value>>> {
    let registries = ctx["capabilities"]["extension.options"]["registries"]
        .as_object()
        .ok_or_else(|| error("configure this extension's options.registries"))?;
    if registries.is_empty() || registries.len() > 32 {
        return Err(error("configure between 1 and 32 resource registries"));
    }
    if let Some(name) = ctx["input"]["registry"]
        .as_str()
        .or_else(|| ctx["answers"]["registry"].as_str())
    {
        if !registries.contains_key(name) {
            return Err(error("resource registry is not configured"));
        }
        return Ok(Ok(name.into()));
    }
    if registries.len() == 1 {
        return Ok(Ok(registries
            .keys()
            .next()
            .ok_or_else(|| error("registry is missing"))?
            .clone()));
    }
    Ok(Err(question(
        "registry",
        "Choose a code resource registry",
        json!({"type":"select","options":registries.keys().collect::<Vec<_>>()}),
    )))
}
fn load(ctx: &Value, name: &str, refresh: bool) -> Result<(Profile, Catalog, String, String)> {
    let profile: Profile = serde_json::from_value(
        ctx["capabilities"]["extension.options"]["registries"][name].clone(),
    )?;
    if !profile.target.is_empty() {
        relative(&profile.target)?;
    }
    // SAFETY: the Host returns a scoped snapshot handle and attested commit.
    let raw = unsafe {
        source_resolve(
            json!({"source":profile.source,"reference":profile.r#ref,"refresh":refresh})
                .to_string(),
        )?
    };
    let snapshot: Value = serde_json::from_str(&raw)?;
    let handle = snapshot["handle"]
        .as_str()
        .ok_or_else(|| error("snapshot handle is missing"))?
        .to_owned();
    let catalog = match profile.format.as_str() {
        "lenso-code-v1" => {
            relative(&profile.path)?;
            let text = remote(&handle, &profile.path)?;
            toml::from_str::<Catalog>(&text)
                .map_err(|e| error(format!("invalid code catalog: {e}")))?
        }
        "inline" => Catalog {
            schema_version: 1,
            name: name.into(),
            items: profile.items.clone(),
        },
        _ => {
            return Err(error(
                "supported catalog formats are lenso-code-v1 and inline",
            ));
        }
    };
    if catalog.schema_version != 1 || catalog.items.len() > 128 {
        return Err(error("invalid catalog schema or item count"));
    }
    let mut names = BTreeSet::new();
    for item in &catalog.items {
        relative(&item.name)?;
        if !names.insert(&item.name) || item.files.is_empty() || item.files.len() > 128 {
            return Err(error("catalog items need unique names and 1-128 files"));
        }
    }
    Ok((
        profile,
        catalog,
        handle,
        snapshot["commit"].as_str().unwrap_or_default().into(),
    ))
}
fn selection(ctx: &Value, refresh: bool) -> Result<Json<Value>> {
    let name = match registry(ctx)? {
        Ok(name) => name,
        Err(question) => return Ok(question),
    };
    let (_, catalog, _, commit) = load(ctx, &name, refresh)?;
    let input = ctx["input"]
        .get("item")
        .or_else(|| ctx["answers"].get("item"));
    let Some(input) = input else {
        return Ok(question(
            "item",
            "Choose a code resource",
            json!({"type":"select","options":catalog.items.iter().map(|i|&i.name).collect::<Vec<_>>()}),
        ));
    };
    let items: Vec<String> = if let Some(name) = input.as_str() {
        vec![name.into()]
    } else {
        serde_json::from_value(input.clone())?
    };
    if items.is_empty()
        || items.len() > 32
        || items
            .iter()
            .any(|name| !catalog.items.iter().any(|item| &item.name == name))
    {
        return Err(error("select 1-32 declared catalog items"));
    }
    Ok(complete(
        json!({"selection":{"registry":name,"items":items,"commit":commit}}),
    ))
}
#[plugin_fn]
pub fn select(Json(ctx): Json<Value>) -> FnResult<Json<Value>> {
    Ok(selection(&ctx, false)?)
}
#[plugin_fn]
pub fn select_update(Json(ctx): Json<Value>) -> FnResult<Json<Value>> {
    Ok(selection(&ctx, true)?)
}
fn resolve_items(ctx: &Value, diff: bool) -> Result<Json<Value>> {
    let selection = ctx["steps"]
        .as_object()
        .and_then(|steps| steps.values().find_map(|s| s.get("selection")))
        .ok_or_else(|| error("resource selection is missing"))?;
    let name = selection["registry"]
        .as_str()
        .ok_or_else(|| error("registry is missing"))?;
    let (profile, catalog, handle, commit) = load(ctx, name, false)?;
    if selection["commit"] != commit {
        return Err(error("repository source changed during selection"));
    }
    let items: Vec<String> = serde_json::from_value(selection["items"].clone())?;
    let mut files = Vec::new();
    let mut targets = BTreeSet::new();
    let mut kept = Vec::new();
    let mut unchanged = 0;
    let mut checked = 0;
    for selected in &items {
        let item = catalog
            .items
            .iter()
            .find(|i| &i.name == selected)
            .ok_or_else(|| error("selected resource disappeared"))?;
        for file in &item.files {
            let (path, target) = match file {
                ResourceFile::Path(path) => (path, path),
                ResourceFile::Target(file) => (&file.path, &file.target),
            };
            relative(path)?;
            relative(target)?;
            let source = if profile.format == "inline" {
                path.clone()
            } else {
                profile
                    .path
                    .rsplit_once('/')
                    .map_or_else(|| path.clone(), |(base, _)| format!("{base}/{path}"))
            };
            let target = if profile.target.is_empty() {
                target.clone()
            } else {
                format!("{}/{target}", profile.target)
            };
            if !targets.insert(target.clone()) {
                return Err(error("selected resources have duplicate destinations"));
            }
            checked += 1;
            let content = remote(&handle, &source)?;
            if let Some(existing) = local(&target)? {
                if existing == content {
                    unchanged += 1;
                    continue;
                }
                if !diff {
                    let id = format!("conflict:{target}");
                    match ctx["answers"][&id].as_str() {
                        None => {
                            return Ok(question(
                                &id,
                                &format!(
                                    "Existing {target} differs.\n--- current\n{existing}\n+++ proposed\n{content}\nChoose overwrite or keep."
                                ),
                                json!({"type":"select","options":["overwrite","keep"]}),
                            ));
                        }
                        Some("keep") => {
                            kept.push(target);
                            continue;
                        }
                        Some("overwrite") => {}
                        _ => return Err(error("invalid conflict decision")),
                    }
                }
            }
            files.push(json!({"path":target,"content":content}));
        }
    }
    Ok(complete(
        json!({"summary":{"checked":checked,"changed":files.len(),"unchanged":unchanged,"kept":kept.len()},"kept":kept,"files":files,"commands":[],"items":items,"repository":profile.source,"commit":commit,"catalog":catalog.name}),
    ))
}
#[plugin_fn]
pub fn resolve(Json(ctx): Json<Value>) -> FnResult<Json<Value>> {
    Ok(resolve_items(&ctx, false)?)
}
#[plugin_fn]
pub fn diff(Json(ctx): Json<Value>) -> FnResult<Json<Value>> {
    Ok(resolve_items(&ctx, true)?)
}
#[plugin_fn]
pub fn verify(Json(ctx): Json<Value>) -> FnResult<Json<Value>> {
    let resolved = ctx["steps"]
        .as_object()
        .and_then(|s| s.values().find(|v| v.get("files").is_some()))
        .ok_or_else(|| error("resolved files are missing"))?;
    for entry in resolved["files"]
        .as_array()
        .ok_or_else(|| error("invalid resolved files"))?
    {
        let path = entry["path"]
            .as_str()
            .ok_or_else(|| error("file path is missing"))?;
        if local(path)?.as_deref() != entry["content"].as_str() {
            return Err(error("resource file verification failed").into());
        }
    }
    Ok(complete(
        json!({"verified":resolved["items"].as_array().ok_or_else(||error("items are missing"))?.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")}),
    ))
}
#[plugin_fn]
pub fn search(Json(ctx): Json<Value>) -> FnResult<Json<Value>> {
    let name = match registry(&ctx)? {
        Ok(name) => name,
        Err(question) => return Ok(question),
    };
    let (_, catalog, _, commit) = load(&ctx, &name, false)?;
    let query = ctx["input"]["query"].as_str().unwrap_or("").to_lowercase();
    let results:Vec<_>=catalog.items.iter().filter(|i|format!("{} {}",i.name,i.description).to_lowercase().contains(&query)).map(|i|json!({"name":i.name,"description":i.description,"registry":name,"type":"code:resource","add":i.name})).collect();
    Ok(complete(
        json!({"pagination":{"total":results.len(),"offset":0,"limit":128,"hasMore":false},"results":results,"commit":commit}),
    ))
}
