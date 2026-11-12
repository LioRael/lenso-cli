//! Registry consumption through a restricted host, with reviewed project changes.

mod layout;
mod npm;
mod registry;
mod search;
mod workspace;

use extism_pdk::{FnResult, Json, host_fn, plugin_fn};
use serde_json::{Value, json};

use layout::{Project, file};

type Result<T> = std::result::Result<T, extism_pdk::Error>;

#[host_fn("extism:host/user")]
unsafe extern "ExtismHost" {
    fn read_file(path: String) -> String;
    fn http_get(url: String) -> String;
    fn http_request(request: String) -> String;
    fn list_files(request: String) -> String;
}

fn read(path: &str) -> Result<Option<String>> {
    // SAFETY: the PDK owns and validates the host memory handles.
    Ok(serde_json::from_str(&unsafe {
        read_file(path.to_owned())?
    })?)
}

fn get(url: &str) -> Result<Value> {
    // SAFETY: the PDK owns and validates the host memory handles.
    Ok(serde_json::from_str(&unsafe { http_get(url.to_owned())? })?)
}

fn namespace(config: &str, registry: &str, name: &str) -> Result<Value> {
    namespace_request(json!({"config":config,"registry":registry,"name":name}))
}

fn request_http(request: Value) -> Result<Value> {
    // SAFETY: the PDK validates the host memory handles.
    let response = unsafe { http_request(request.to_string())? };
    Ok(serde_json::from_str(&response)?)
}

fn namespace_request(request: Value) -> Result<Value> {
    let config_path = string(&request, "config")?;
    let namespace = string(&request, "registry")?;
    let name = string(&request, "name")?;
    registry::Reference::parse(&format!("{namespace}/{name}"), None)?;
    let config: Value = serde_json::from_str(
        &read(config_path)?.ok_or_else(|| error("Registry configuration is missing"))?,
    )?;
    let entry = config["registries"]
        .get(namespace)
        .ok_or_else(|| error(format!("Registry namespace {namespace} is not configured")))?;
    let template = entry
        .as_str()
        .or_else(|| entry["url"].as_str())
        .ok_or_else(|| error("Registry URL is missing"))?;
    if !template.contains("{name}") || template.contains("${") {
        return Err(error(
            "Registry URL must use {name}; authentication variables belong in headers or params",
        ));
    }
    let target = template.replace("{name}", name);
    registry::Reference::parse(&target, None)?;
    let mut params = json!({});
    if let Some(search) = request.get("search") {
        if url::Url::parse(&target)?
            .query_pairs()
            .any(|(name, _)| matches!(name.as_ref(), "q" | "type" | "limit" | "offset"))
        {
            return Err(error("Registry URL conflicts with search parameters"));
        }
        params = json!({"q":search["query"],"type":search["type"],"limit":search["limit"].to_string(),"offset":search["offset"].to_string()});
    }
    let authenticated = ["headers", "params"].iter().any(|key| {
        entry[*key]
            .as_object()
            .is_some_and(|fields| !fields.is_empty())
    });
    let mut operation = json!({"url":target,"params":params});
    if authenticated {
        operation["credential"] = json!(format!("{config_path}#{namespace}"));
    }
    request_http(operation)
}

fn npm(name: &str, requirements: &[String]) -> Result<String> {
    npm::resolve(name, requirements)
}

fn inventory() -> Result<Vec<String>> {
    // SAFETY: the PDK validates returned memory; file contents need separate read grants.
    let raw =
        unsafe { list_files(json!({"exclude":["node_modules","target","dist"]}).to_string())? };
    Ok(serde_json::from_str(&raw)?)
}

fn workspace_paths() -> Result<Vec<String>> {
    workspace::directories()
}

fn error(message: impl Into<String>) -> extism_pdk::Error {
    extism_pdk::Error::msg(message.into())
}
fn complete(output: Value) -> Json<Value> {
    Json(json!({"status":"complete","output":output}))
}
fn question(id: &str, message: String, kind: Value) -> Json<Value> {
    Json(json!({"status":"needs_input","question":{"id":id,"message":message,"kind":kind}}))
}
fn dependency<'a>(ctx: &'a Value, key: &str) -> Result<&'a Value> {
    ctx["steps"]
        .as_object()
        .and_then(|steps| steps.values().find_map(|output| output.get(key)))
        .ok_or_else(|| error(format!("missing {key} dependency")))
}
fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| error(format!("missing string field {key}")))
}

#[plugin_fn]
pub fn detect(Json(ctx): Json<Value>) -> FnResult<Json<Value>> {
    if ctx["abi_version"] != 1 {
        return Err(error("unsupported Lenso ABI").into());
    }
    let workspace_dirs = workspace_paths()?;
    let root = layout::load_package("", workspace_dirs.is_empty().then(layout::default_config))?;
    let mut packages = vec![root];
    for directory in workspace_dirs {
        packages.push(layout::load_package(&directory, None)?);
    }
    let mut names = std::collections::BTreeSet::new();
    for package in &packages {
        if !package.name.is_empty() && !names.insert(&package.name) {
            return Err(error("duplicate workspace package names").into());
        }
    }
    let candidates: Vec<String> = packages
        .iter()
        .filter(|package| !package.config.is_null())
        .map(|package| package.dir.clone())
        .collect();
    let requested = ctx["input"]["cwd"]
        .as_str()
        .or_else(|| ctx["answers"]["workspace"].as_str());
    let active = if let Some(requested) = requested {
        let path = layout::normalize("", requested)?;
        if !candidates.contains(&path) {
            return Err(error("selected workspace needs components.json").into());
        }
        path
    } else if let Some(cwd) = ctx["capabilities"]["host.cwd"]
        .as_str()
        .filter(|cwd| !cwd.is_empty())
    {
        candidates
            .iter()
            .filter(|directory| {
                !directory.is_empty()
                    && (cwd == directory.as_str() || cwd.starts_with(&format!("{directory}/")))
            })
            .max_by_key(|directory| directory.len())
            .cloned()
            .unwrap_or_default()
    } else {
        String::new()
    };
    let active = if candidates.contains(&active) {
        active
    } else if candidates.len() == 1 {
        candidates[0].clone()
    } else {
        if candidates.is_empty() {
            return Err(error(
                "a workspace needs components.json before installing Registry resources",
            )
            .into());
        }
        return Ok(question(
            "workspace",
            "Choose the app or workspace to modify".into(),
            json!({"type":"select","options":candidates}),
        ));
    };
    let package = packages
        .iter()
        .find(|package| package.dir == active)
        .ok_or_else(|| error("workspace disappeared"))?;
    if package.initialize {
        match ctx["answers"]["initialize"].as_bool() {
            None => {
                return Ok(question(
                    "initialize",
                    "Create components.json for this prepared TypeScript/Tailwind 4 project?"
                        .into(),
                    json!({"type":"confirm"}),
                ));
            }
            Some(false) => return Err(error("configuration initialization declined").into()),
            Some(true) => {}
        }
    }
    let manager = packages[0].package["packageManager"]
        .as_str()
        .or_else(|| package.package["packageManager"].as_str())
        .map(|value| value.split('@').next().unwrap_or(value))
        .or_else(|| ctx["answers"]["packageManager"].as_str());
    let manager = match manager {
        Some(manager @ ("npm" | "pnpm" | "bun")) => manager,
        Some(_) => return Err(error("supported package managers are npm, pnpm, and bun").into()),
        None => {
            return Ok(question(
                "packageManager",
                "Choose the package manager".into(),
                json!({"type":"select","options":["npm","pnpm","bun"]}),
            ));
        }
    };
    read("package-lock.json")?;
    if manager == "pnpm" {
        read("pnpm-lock.yaml")?;
    }
    if manager == "bun" {
        read("bun.lock")?;
    }
    let manager = manager.to_owned();
    let project = layout::build(&active, packages, &manager)?;
    Ok(complete(json!({"project":project})))
}

#[plugin_fn]
pub fn select(Json(ctx): Json<Value>) -> FnResult<Json<Value>> {
    select_items(ctx)
}

fn select_items(ctx: Value) -> FnResult<Json<Value>> {
    let input = ctx["input"]
        .get("item")
        .or_else(|| ctx["input"].get("items"))
        .or_else(|| ctx["answers"].get("item"));
    if let Some(input) = input {
        let items: Vec<String> = if let Some(item) = input.as_str() {
            vec![item.into()]
        } else {
            serde_json::from_value(input.clone())?
        };
        if items.is_empty() || items.len() > 32 {
            return Err(error("select between 1 and 32 Registry resources").into());
        }
        for item in &items {
            registry::Reference::parse(item, None)?;
        }
        return Ok(complete(
            json!({"selection":{"item":items.join(", "),"items":items}}),
        ));
    }
    let index = get("https://ui.shadcn.com/r/index.json")?;
    let items = index
        .as_array()
        .or_else(|| index["items"].as_array())
        .ok_or_else(|| error("invalid Registry index"))?;
    let options: Vec<&str> = items
        .iter()
        .filter_map(|item| item["name"].as_str())
        .collect();
    if options.is_empty() {
        return Err(error("Registry catalog is empty").into());
    }
    Ok(question(
        "item",
        "Choose a shadcn Registry resource (or pass a URL / @namespace/name)".into(),
        json!({"type":"select","options":options}),
    ))
}

#[plugin_fn]
pub fn initialize(Json(ctx): Json<Value>) -> FnResult<Json<Value>> {
    let project: Project = serde_json::from_value(dependency(&ctx, "project")?.clone())?;
    let files = if project.initialize {
        vec![
            json!({"path":file(&project.active,"components.json"),"content":format!("{}\n",serde_json::to_string_pretty(&project.config)?)}),
        ]
    } else {
        Vec::new()
    };
    Ok(complete(json!({"files":files})))
}

#[plugin_fn]
pub fn resolve(Json(ctx): Json<Value>) -> FnResult<Json<Value>> {
    registry::resolve(&ctx, registry::Mode::Add).map_err(Into::into)
}

#[plugin_fn]
pub fn search(Json(ctx): Json<Value>) -> FnResult<Json<Value>> {
    search::search(&ctx).map_err(Into::into)
}

#[plugin_fn]
pub fn diff(Json(ctx): Json<Value>) -> FnResult<Json<Value>> {
    registry::resolve(&ctx, registry::Mode::Diff).map_err(Into::into)
}

#[plugin_fn]
pub fn update(Json(ctx): Json<Value>) -> FnResult<Json<Value>> {
    registry::resolve(&ctx, registry::Mode::Update).map_err(Into::into)
}

#[plugin_fn]
pub fn select_update(Json(ctx): Json<Value>) -> FnResult<Json<Value>> {
    if ctx["input"].get("item").is_none()
        && ctx["input"].get("items").is_none()
        && ctx["answers"].get("item").is_none()
    {
        return Ok(question(
            "item",
            "Registry resource to compare or update (name, @namespace/name, URL, or local JSON)"
                .into(),
            json!({"type":"text"}),
        ));
    }
    select_items(ctx)
}

#[plugin_fn]
pub fn validate_plan(Json(ctx): Json<Value>) -> FnResult<Json<Value>> {
    let plan = &ctx["hooks"]["result"]["plan"];
    if !plan["files"].is_array()
        || dependency(&ctx, "registry_items")?
            .as_array()
            .is_none_or(Vec::is_empty)
    {
        return Err(error("plan requires files and resolved Registry provenance").into());
    }
    Ok(complete(json!({"validated":"shadcn/plan"})))
}

#[plugin_fn]
pub fn verify(Json(ctx): Json<Value>) -> FnResult<Json<Value>> {
    let output = ctx["steps"]
        .as_object()
        .and_then(|steps| steps.values().find(|output| output.get("files").is_some()))
        .ok_or_else(|| error("missing resolved files"))?;
    for file in output["files"]
        .as_array()
        .ok_or_else(|| error("invalid resolved files"))?
    {
        let path = string(file, "path")?;
        if read(path)?.as_deref() != file["content"].as_str() {
            return Err(error(format!("verification failed for {path}")).into());
        }
    }
    Ok(complete(
        json!({"verified":output["installed_item"],"package_manager":output["package_manager"],"scope":"reviewed files; run the project's build to verify compilation"}),
    ))
}
