//! Optional repository code distribution. The Host owns transport, pins and permissions.
mod author;
mod composition;
mod generation;
mod modules;
mod native;
mod project_config;
mod setup;
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
#[derive(Clone, Deserialize, Serialize)]
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
    setup::require_current_profile(&ctx["capabilities"]["extension.options"]["registries"][name])?;
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
    let refresh = refresh || ctx["input"]["refresh"].as_bool().unwrap_or(false);
    if modules::enabled(ctx)? {
        return Ok(complete(json!({"module_request":true,"refresh":refresh})));
    }
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
    if modules::enabled(ctx)? {
        let refresh = ctx["steps"]
            .as_object()
            .is_some_and(|steps| steps.values().any(|s| s["refresh"] == true));
        return modules::plan(ctx, refresh).map_err(|e| error(e.to_string()));
    }
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
        let content = resolved["finalizations"]
            .as_array()
            .and_then(|files| files.iter().find(|file| file["path"] == entry["path"]))
            .unwrap_or(entry);
        if local(path)?.as_deref() != content["content"].as_str() {
            return Err(error("resource file verification failed").into());
        }
    }
    if let Some(expected) = resolved["expected"].as_object() {
        for (path, content) in expected {
            if local(path)?.as_deref() != content.as_str() {
                return Err(error("generated file postcondition failed").into());
            }
        }
    }
    Ok(complete(
        json!({"verified":resolved["items"].as_array().ok_or_else(||error("items are missing"))?.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")}),
    ))
}
#[plugin_fn]
pub fn search(Json(ctx): Json<Value>) -> FnResult<Json<Value>> {
    if modules::enabled(&ctx)? {
        return Ok(modules::search(&ctx)?);
    }
    let name = match registry(&ctx)? {
        Ok(name) => name,
        Err(question) => return Ok(question),
    };
    let (_, catalog, _, commit) = load(&ctx, &name, false)?;
    let query = ctx["input"]["query"]
        .as_str()
        .or_else(|| ctx["input"]["term"].as_str())
        .unwrap_or("")
        .to_lowercase();
    let results:Vec<_>=catalog.items.iter().filter(|i|format!("{} {}",i.name,i.description).to_lowercase().contains(&query)).map(|i|json!({"name":i.name,"description":i.description,"registry":name,"type":"code:resource","add":i.name})).collect();
    Ok(complete(
        json!({"pagination":{"total":results.len(),"offset":0,"limit":128,"hasMore":false},"results":results,"commit":commit}),
    ))
}

/// Ask for repository settings without reading files or fetching code.
#[plugin_fn]
pub fn configure(Json(ctx): Json<Value>) -> FnResult<Json<Value>> {
    configure_context(ctx)
}
fn configure_context(ctx: Value) -> FnResult<Json<Value>> {
    if !ctx["input"].is_object() {
        return Err(error("setup input must be an object").into());
    }
    if let Some(options) =
        setup::migration_options(&ctx).or_else(|| setup::configured_options(&ctx))
    {
        let mut migration = ctx.clone();
        if migration["input"]["native"].is_null() {
            if let Some(native) = options.get("native") {
                migration["input"]["native"] = native.clone();
            } else if options["registries"].as_object().is_some_and(|profiles| {
                profiles.values().any(|p| p["format"] == "lenso-modules-v1")
            }) {
                migration["input"]["native"] =
                    json!({"npm":{"manifest":"package.json"},"cargo":{"manifest":"Cargo.toml"}});
            }
        }
        let mut proposal = json!({"installation":{"options":options}});
        native_setup(&migration, &mut proposal)?;
        preserve_setup_grants(&ctx, &mut proposal)?;
        return Ok(complete(proposal));
    }
    let (prepared, selection) = match setup::prepare(&ctx)? {
        setup::Preparation::Question(question) => return Ok(question),
        setup::Preparation::Ready { context, selection } => (context, selection),
    };
    let mut result = configure_input(prepared)?;
    setup::finish(&ctx, selection, &mut result.0);
    Ok(result)
}

fn configure_input(mut ctx: Value) -> FnResult<Json<Value>> {
    let explicit = ctx["input"]["repo"].is_string() || ctx["input"]["author_root"].is_string();
    if !explicit
        && (ctx["input"]["format"].is_null() || ctx["input"]["format"] == "lenso-modules-v1")
        && ctx["capabilities"]["extension.options"]["registries"]
            .as_object()
            .is_none_or(|p| p.is_empty())
    {
        let input = ctx["input"]
            .as_object_mut()
            .ok_or_else(|| error("invalid setup input"))?;
        input.insert("repo".into(), json!(setup::OFFICIAL_REPO));
        input.insert("format".into(), json!("lenso-modules-v1"));
        input
            .entry("catalog")
            .or_insert(json!(setup::MODULE_DIRECTORY));
    }
    if ctx["input"]["repo"]
        .as_str()
        .is_some_and(setup::official_repo)
        && ctx["input"]["format"].is_null()
        && (ctx["input"]["catalog"].is_null() || ctx["input"]["catalog"] == setup::MODULE_DIRECTORY)
    {
        ctx["input"]["format"] = json!("lenso-modules-v1");
    }
    if ctx["input"]["format"] == "lenso-modules-v1" && ctx["input"]["catalog"].is_null() {
        ctx["input"]["catalog"] = json!(setup::MODULE_DIRECTORY);
    }
    if ctx["input"]["format"] == "lenso-modules-v1" && ctx["input"]["native"].is_null() {
        ctx["input"]["native"] =
            json!({"npm":{"manifest":"package.json"},"cargo":{"manifest":"Cargo.toml"}});
    }
    if ctx["input"].get("native").is_some()
        && ctx["input"].get("author_root").is_none()
        && ctx["input"].get("repo").is_none()
        && ctx["capabilities"]["extension.options"]["registries"].is_object()
    {
        let mut proposal =
            json!({"installation":{"options":ctx["capabilities"]["extension.options"]}});
        native_setup(&ctx, &mut proposal)?;
        preserve_setup_grants(&ctx, &mut proposal)?;
        return Ok(complete(proposal));
    }

    if let Some(root) = ctx["input"]["author_root"].as_str() {
        relative(root)?;
        let target = ctx["input"]["target"].as_str().unwrap_or("src");
        let catalog = ctx["input"]["catalog"].as_str().unwrap_or("catalog.toml");
        lenso_codegen::relative(target)?;
        lenso_codegen::relative(catalog)?;
        let mut proposal = json!({"installation":{"sources":[],"read_roots":[root,target],"write_roots":[target],"options":{"registries":{"main":{"source":format!("project:{root}"),"ref":"working-tree","path":catalog,"format":"lenso-compose-v1","target":target}}}}});
        if ctx["input"]["format"] == "lenso-modules-v1" {
            proposal["installation"]["options"]["registries"]["main"]["format"] =
                json!("lenso-modules-v1");
        }
        native_setup(&ctx, &mut proposal)?;
        return Ok(complete(proposal));
    }

    let repo = ctx["input"]["repo"]
        .as_str()
        .or_else(|| ctx["answers"]["code:repository"].as_str());
    let Some(repo) = repo else {
        return Ok(question(
            "code:repository",
            "GitHub repository containing your code catalog (owner/repo)",
            json!({"type":"text"}),
        ));
    };
    if repo.split('/').count() != 2
        || repo.split('/').any(|part| {
            part.is_empty()
                || !part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        })
    {
        return Err(error("repository must be owner/repo").into());
    }
    let target = ctx["input"]["target"].as_str().unwrap_or("src");
    let catalog = ctx["input"]["catalog"].as_str().unwrap_or("catalog.toml");
    relative(target)?;
    relative(catalog)?;
    let format = ctx["input"]["format"].as_str().unwrap_or("lenso-code-v1");
    if !matches!(
        format,
        "lenso-code-v1" | "lenso-compose-v1" | "lenso-modules-v1"
    ) {
        return Err(error("unsupported catalog format").into());
    }
    let reference = ctx["input"]["ref"].as_str().unwrap_or("HEAD");
    let mut proposal = json!({"installation":{"sources":[format!("github:{repo}")],"read_roots":[target],"write_roots":[target],"options":{"registries":{"main":{"source":format!("github-repo:{repo}"),"ref":reference,"path":catalog,"format":format,"target":target}}}}});
    native_setup(&ctx, &mut proposal)?;
    Ok(complete(proposal))
}

#[plugin_fn]
pub fn ready(Json(ctx): Json<Value>) -> FnResult<Json<Value>> {
    let composition = ctx["capabilities"]["extension.options"]["registries"]
        .as_object()
        .is_some_and(|profiles| {
            profiles
                .values()
                .any(|profile| profile["format"] == "lenso-compose-v1")
        });
    let command = if composition {
        "compose-preview"
    } else {
        "search"
    };
    Ok(complete(
        json!({"message":format!("Code source configured; use lenso code {command}")}),
    ))
}

fn native_setup(ctx: &Value, proposal: &mut Value) -> Result<()> {
    if let Some(value) = ctx["input"].get("native") {
        let (settings, paths, commands) = native::configure(value)?;
        proposal["installation"]["options"]["native"] = settings;
        proposal["installation"]["reads"] = json!(paths);
        proposal["installation"]["files"] = json!(
            paths
                .into_iter()
                .filter(|path| path != "Cargo.lock"
                    && !path.ends_with("/Cargo.lock")
                    && path != "package-lock.json"
                    && !path.ends_with("/package-lock.json"))
                .collect::<Vec<_>>()
        );
        proposal["installation"]["processes"] = json!(commands);
    }
    for field in ["reads", "files"] {
        if proposal["installation"][field].is_null() {
            proposal["installation"][field] = json!([]);
        }
        let paths = proposal["installation"][field]
            .as_array_mut()
            .ok_or_else(|| error("invalid configuration grants"))?;
        if !paths.iter().any(|p| p == project_config::FILE) {
            paths.push(json!(project_config::FILE));
        }
    }

    Ok(())
}

fn preserve_setup_grants(ctx: &Value, proposal: &mut Value) -> Result<()> {
    for (field, capability) in [
        ("reads", "host.reads"),
        ("files", "project.files"),
        ("processes", "host.processes"),
    ] {
        if proposal["installation"][field].is_null() {
            proposal["installation"][field] = json!([]);
        }
        if let Some(existing) = ctx["capabilities"][capability].as_array() {
            let proposed = proposal["installation"][field]
                .as_array_mut()
                .ok_or_else(|| error("missing native setup field"))?;
            for value in existing {
                if !proposed.contains(value) {
                    proposed.push(value.clone());
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod configuration_tests {
    use super::*;
    fn context(input: Value) -> Value {
        json!({"input":input,"answers":{},"capabilities":{"extension.options":{}}})
    }
    #[test]
    fn empty_setup_prepares_a_directory_without_selecting_a_language() {
        let output = configure_input(context(json!({}))).unwrap().0;
        assert_eq!(output["status"], "complete");
        assert_eq!(
            output["output"]["installation"]["options"]["registries"]["main"]["format"],
            "lenso-modules-v1"
        );
        assert!(output["output"]["installation"]["options"]["selection"].is_null());
    }
    #[test]
    fn explicit_resource_format_keeps_its_repository_question() {
        let output = configure_input(context(json!({"format":"lenso-code-v1"})))
            .unwrap()
            .0;
        assert_eq!(output["question"]["id"], "code:repository");
    }
    #[test]
    fn explicit_resource_repository_is_not_replaced() {
        let output = configure_input(context(json!({"repo":"acme/resources"})))
            .unwrap()
            .0;
        assert_eq!(
            output["output"]["installation"]["options"]["registries"]["main"]["source"],
            "github-repo:acme/resources"
        );
    }
}

#[cfg(test)]
mod official_directory_tests {
    use super::*;
    #[test]
    fn empty_profile_map_uses_official_defaults() {
        let result = configure_input(
            json!({"input":{},"answers":{},"capabilities":{"extension.options":{"registries":{}}}}),
        )
        .unwrap()
        .0;
        assert_eq!(
            result["output"]["installation"]["options"]["registries"]["main"]["path"],
            setup::MODULE_DIRECTORY
        );
    }
    #[test]
    fn explicit_official_repository_uses_the_module_directory() {
        let result = configure_input(json!({"input":{"repo":"LioRael/lenso-modules"},"answers":{},"capabilities":{"extension.options":{}}})).unwrap().0;
        assert_eq!(
            result["output"]["installation"]["options"]["registries"]["main"]["format"],
            "lenso-modules-v1"
        );
        assert_eq!(
            result["output"]["installation"]["options"]["registries"]["main"]["path"],
            "lenso.modules.toml"
        );
    }
    #[test]
    fn module_format_uses_a_module_directory_for_custom_and_local_authors() {
        for source in [
            json!({"repo":"acme/modules","format":"lenso-modules-v1"}),
            json!({"author_root":"author","format":"lenso-modules-v1"}),
        ] {
            let result = configure_input(
                json!({"input":source,"answers":{},"capabilities":{"extension.options":{}}}),
            )
            .unwrap()
            .0;
            assert_eq!(
                result["output"]["installation"]["options"]["registries"]["main"]["path"],
                "lenso.modules.toml"
            );
        }
    }
    #[test]
    fn migration_preserves_custom_profiles_options_and_grants() {
        let options = json!({"selection":{"language":"rust","runtime":"tokio"},"unrelated":"retained","registries":{
            "official":{"source":"github-repo:LioRael/lenso-modules","ref":"pinned-ref","path":"catalog.toml","format":"lenso-code-v1","target":"src/custom"},
            "custom":{"source":"github-repo:acme/resources","ref":"v1","path":"catalog.toml","format":"lenso-code-v1","target":"src/other"}}});
        let result=configure_context(json!({"input":{},"answers":{},"capabilities":{"extension.options":options,"host.reads":["custom.toml"],"project.files":["custom.toml"],"host.processes":[{"program":"custom","args":[]}]}})).unwrap().0;
        assert_eq!(result["status"], "complete");
        let installation = &result["output"]["installation"];
        let migrated = &installation["options"]["registries"]["official"];
        assert_eq!(migrated["format"], "lenso-modules-v1");
        assert_eq!(migrated["path"], "lenso.modules.toml");
        assert_eq!(migrated["ref"], "pinned-ref");
        assert_eq!(migrated["target"], "src/custom");
        assert_eq!(installation["options"]["selection"], options["selection"]);
        assert_eq!(
            installation["options"]["registries"]["custom"],
            options["registries"]["custom"]
        );
        assert_eq!(installation["options"]["unrelated"], "retained");
        assert!(
            installation["reads"]
                .as_array()
                .unwrap()
                .contains(&json!("custom.toml"))
        );
        assert!(
            installation["processes"]
                .as_array()
                .unwrap()
                .contains(&json!({"program":"custom","args":[]}))
        );
        assert!(installation.get("sources").is_none());
        assert!(installation.get("write_roots").is_none());
    }
    #[test]
    fn custom_and_current_profiles_are_never_reinterpreted() {
        for profile in [
            json!({"source":"github-repo:acme/resources","ref":"v1","format":"lenso-code-v1","path":"catalog.toml","target":"src"}),
            json!({"source":"github-repo:LioRael/lenso-modules","ref":"v0.2.0","format":"lenso-compose-v1","path":"auth.rust.toml","target":"src"}),
            json!({"source":"github-repo:LioRael/lenso-modules","ref":"custom","format":"lenso-code-v1","path":"custom.toml","target":"src"}),
            json!({"source":"github-repo:LioRael/lenso-modules","ref":"pinned","format":"lenso-modules-v1","path":"lenso.modules.toml","target":"src"}),
        ] {
            let options = json!({"registries":{"named":profile},"unrelated":true});
            let ctx = json!({"input":{},"answers":{},"capabilities":{"extension.options":options}});
            assert!(setup::migration_options(&ctx).is_none());
            let result = configure_context(ctx).unwrap().0;
            assert_eq!(
                result["output"]["installation"]["options"]["registries"],
                options["registries"]
            );
            assert_eq!(
                result["output"]["installation"]["options"]["unrelated"],
                true
            );
        }
    }
    #[test]
    fn legacy_profile_stops_before_attempting_a_repository_read() {
        let result = modules::enabled(
            &json!({"input":{},"answers":{},"capabilities":{"extension.options":{"registries":{"main":{"source":"github-repo:LioRael/lenso-modules","ref":"HEAD","path":"catalog.toml","format":"lenso-code-v1","target":"src"}}}}}),
        );
        let diagnostic = result
            .expect_err("legacy profile reached the resource loader")
            .to_string();
        assert!(diagnostic.contains("lenso ext configure code"));
    }
}
