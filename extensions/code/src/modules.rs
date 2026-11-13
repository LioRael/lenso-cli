//! Code-owned module discovery and dispatch into the existing composition planner.
use crate::{Profile, Result, complete, error, local, question, remote};
use extism_pdk::Json;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Directory {
    schema_version: u32,
    namespace: String,
    modules: BTreeMap<String, Entry>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    description: String,
    root: String,
    releases: Vec<Release>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Release {
    version: String,
    reference: String,
    targets: Vec<Target>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Target {
    #[serde(default)]
    when: BTreeMap<String, Value>,
    language: String,
    runtime: String,
    catalog: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Installed {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    when: BTreeMap<String, Value>,
    version: String,
    profile: Profile,
    artifact: String,
    root: String,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    schema_version: u32,
    language: String,
    runtime: String,
    modules: BTreeMap<String, Installed>,
}
fn slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}
fn profile(ctx: &Value) -> Result<Option<Profile>> {
    let Some(profiles) = ctx["capabilities"]["extension.options"]["registries"].as_object() else {
        return Ok(None);
    };
    let selected = ctx["input"]["registry"]
        .as_str()
        .or_else(|| ctx["answers"]["registry"].as_str());
    let value = if let Some(name) = selected {
        profiles.get(name)
    } else if profiles.len() == 1 {
        profiles.values().next()
    } else {
        None
    };
    value
        .map(|value| -> Result<Profile> {
            crate::setup::require_current_profile(value)?;
            Ok(serde_json::from_value(value.clone())?)
        })
        .transpose()
}
pub fn enabled(ctx: &Value) -> Result<bool> {
    Ok(profile(ctx)?
        .is_some_and(|p| matches!(p.format.as_str(), "lenso-modules-v1" | "lenso-compose-v1")))
}
fn directory(p: &Profile, refresh: bool) -> Result<Directory> {
    crate::relative(&p.path)?;
    let text = if let Some(root) = p.source.strip_prefix("project:") {
        crate::relative(root)?;
        local(&crate::author::destination(root, &p.path))?
            .ok_or_else(|| error("module directory is missing"))?
    } else {
        // SAFETY: The Host verifies source grants, immutable snapshot handles and lock policy.
        let snapshot: Value = serde_json::from_str(&unsafe {
            crate::source_resolve(
                json!({"source":p.source,"reference":p.r#ref,"refresh":refresh}).to_string(),
            )?
        })?;
        remote(
            snapshot["handle"]
                .as_str()
                .ok_or_else(|| error("missing directory snapshot"))?,
            &p.path,
        )?
    };
    if text.len() > 1024 * 1024 {
        return Err(error("module directory exceeds 1 MiB"));
    }
    let d: Directory = toml::from_str(&text)?;
    if d.schema_version != 1 || !slug(&d.namespace) || d.modules.is_empty() || d.modules.len() > 128
    {
        return Err(error("invalid module directory"));
    }
    for (id, entry) in &d.modules {
        if !slug(id)
            || !slug(&entry.root)
            || entry.releases.is_empty()
            || entry.releases.len() > 128
        {
            return Err(error("invalid module entry"));
        }
        let mut versions = std::collections::BTreeSet::new();
        for release in &entry.releases {
            let version = semver::Version::parse(&release.version)?;
            if !versions.insert(version)
                || release.reference.is_empty()
                || release.targets.is_empty()
                || release.targets.len() > 32
            {
                return Err(error("invalid or duplicate module release"));
            }
            let mut targets = std::collections::BTreeSet::new();
            for t in &release.targets {
                crate::relative(&t.catalog)?;
                if t.when.len() > 32
                    || t.when
                        .values()
                        .any(|v| crate::project_config::scalar(v).is_none())
                {
                    return Err(error(
                        "target conditions must be scalar project conventions",
                    ));
                }
                if !slug(&t.language)
                    || !slug(&t.runtime)
                    || !targets.insert((&t.language, &t.runtime, serde_json::to_string(&t.when)?))
                {
                    return Err(error("invalid module target"));
                }
            }
        }
    }

    Ok(d)
}
pub fn search(ctx: &Value) -> Result<Json<Value>> {
    let p = profile(ctx)?.ok_or_else(|| error("choose a registry"))?;
    if p.format == "lenso-compose-v1" {
        let loaded = crate::author::load_profile(ctx, p)?;
        let query = ctx["input"]["query"]
            .as_str()
            .or_else(|| ctx["input"]["term"].as_str())
            .unwrap_or("");
        let results: Vec<_> = loaded
            .catalog
            .modules
            .keys()
            .filter(|id| id.contains(query))
            .map(|id| json!({"name":id,"type":"code:module","add":id}))
            .collect();
        return Ok(complete(json!({"results":results})));
    }
    let d = directory(&p, false)?;
    let query = ctx["input"]["query"]
        .as_str()
        .or_else(|| ctx["input"]["term"].as_str())
        .unwrap_or("")
        .to_lowercase();
    let results:Vec<_>=d.modules.iter().filter(|(id,e)|format!("{id} {}",e.description).to_lowercase().contains(&query)).map(|(id,e)|json!({"name":id,"id":format!("@{}/{id}",d.namespace),"description":e.description,"type":"code:module","add":id})).collect();
    Ok(complete(
        json!({"results":results,"pagination":{"total":results.len(),"offset":0,"limit":128,"hasMore":false}}),
    ))
}
fn item(ctx: &Value) -> Result<Option<String>> {
    let v = &ctx["input"]["item"];
    if v.is_null() {
        return Ok(None);
    }
    if let Some(s) = v.as_str() {
        return Ok(Some(s.into()));
    }
    let items: Vec<String> = serde_json::from_value(v.clone())?;
    if items.len() != 1 {
        return Err(error("add or update one module at a time"));
    }
    Ok(items.into_iter().next())
}
fn roots(ctx: &mut Value, root: &str, previous: Option<&str>) -> Result<()> {
    if !ctx["input"]["instances"].is_null() || !ctx["input"]["sample"].is_null() {
        return Ok(());
    }
    let mut saved = previous
        .map(local)
        .transpose()?
        .flatten()
        .map(|s| serde_json::from_str::<Value>(&s))
        .transpose()?;
    if let Some(saved) = &mut saved
        && !ctx["input"]["bindings"].is_null()
    {
        let mut request = serde_json::from_value(saved["request"].clone())?;
        crate::composition::replace_bindings(&mut request, &ctx["input"]["bindings"])?;
        saved["request"] = serde_json::to_value(request)?;
    }
    if let Some(saved) = &saved {
        for field in ["bindings", "configuration", "context"] {
            if ctx["input"][field].is_null() {
                ctx["input"][field] = saved["request"][field].clone();
            }
        }
    }
    let mut instances = saved
        .and_then(|v| v["request"]["instances"].as_object().cloned())
        .unwrap_or_default();
    if !instances.values().any(|v| v == root) {
        instances.entry(root.to_owned()).or_insert(json!(root));
    }
    ctx["input"]["instances"] = json!(instances);
    Ok(())
}
pub fn plan(ctx: &Value, refresh: bool) -> Result<Json<Value>> {
    let refresh = refresh || ctx["input"]["refresh"].as_bool().unwrap_or(false);
    let mut policy = crate::project_config::load(ctx)?;
    let mut fixed = policy.fixed();
    let p = profile(ctx)?.ok_or_else(|| error("choose a registry"))?;
    let name = item(ctx)?;
    if p.format == "lenso-compose-v1" {
        let mut context = ctx.clone();
        context["input"]["refresh"] = json!(refresh);
        let path = crate::author::destination(&p.target, "composition.json");
        if let Some(name) = name {
            roots(&mut context, &name, Some(&path))?;
        }
        let loaded = crate::author::load_profile(&context, p)?;
        return crate::composition::plan(&context, loaded, &path).map_err(|e| e.0);
    }
    let index_path = crate::author::destination(&p.target, "code.modules.json");
    let mut state: State = local(&index_path)?
        .map(|s| serde_json::from_str(&s))
        .transpose()?
        .unwrap_or_default();
    if state.schema_version != 0 && state.schema_version != 1 || state.modules.len() > 64 {
        return Err(error("invalid module installation index"));
    }
    if state.language.is_empty() {
        let legacy = crate::author::destination(&p.target, "composition.json");
        if let Some(saved) = local(&legacy)? {
            let saved: Value = serde_json::from_str(&saved)?;
            state.language = saved["request"]["context"]["language"]
                .as_str()
                .unwrap_or("")
                .into();
            state.runtime = saved["request"]["context"]["runtime"]
                .as_str()
                .unwrap_or("")
                .into();
        }
    }
    let d = directory(&p, refresh)?;
    let Some(name) = name.or_else(|| ctx["answers"]["code:module"].as_str().map(str::to_owned))
    else {
        return Ok(question(
            "code:module",
            "Choose a code module",
            json!({"type":"select","options":d.modules.keys().collect::<Vec<_>>()}),
        ));
    };
    let short = name
        .strip_prefix(&format!("@{}/", d.namespace))
        .unwrap_or(&name);
    let entry = d
        .modules
        .get(short)
        .ok_or_else(|| error(format!("unknown code module {name}")))?;
    let id = format!("@{}/{short}", d.namespace);
    let previous = state.modules.get(&id);
    if state.modules.len() >= 64 && previous.is_none() {
        return Err(error("module installation limit reached"));
    }
    let release = entry
        .releases
        .iter()
        .filter(|r| previous.is_none_or(|old| refresh || r.version == old.version))
        .filter_map(|r| {
            semver::Version::parse(&r.version)
                .ok()
                .filter(|v| v.pre.is_empty())
                .map(|v| (v, r))
        })
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, r)| r)
        .ok_or_else(|| error("the pinned module version is absent from the directory"))?;
    let languages: std::collections::BTreeSet<_> =
        release.targets.iter().map(|t| t.language.clone()).collect();
    let q = format!(
        "code:module-language:{}",
        crate::generation::digest(&serde_json::to_string(
            &json!({"id":id,"version":release.version,"languages":languages})
        )?)
    );
    if let Some(configured) = fixed.get("language") {
        if ctx["input"]["language"]
            .as_str()
            .is_some_and(|v| v != configured)
        {
            return Err(error("language conflicts with lenso.code.toml"));
        }
        if !state.language.is_empty() && state.language != *configured {
            return Err(error("changing project language requires a new target"));
        }
    }
    let language = ctx["input"]["language"]
        .as_str()
        .or_else(|| ctx["answers"][&q].as_str())
        .or_else(|| (!state.language.is_empty()).then_some(state.language.as_str()))
        .or_else(|| fixed.get("language").map(String::as_str))
        .map(str::to_owned);
    let Some(language) = language else {
        return Ok(question(
            &q,
            "Choose the project language",
            json!({"type":"select","options":languages}),
        ));
    };
    if !state.language.is_empty() && state.language != language {
        return Err(error("changing language requires a new project target"));
    }
    let rq = format!("code:module-runtime:{id}:{}:{language}", release.version);
    let selected_runtime = ctx["input"]["runtime"]
        .as_str()
        .or_else(|| ctx["answers"][&rq].as_str())
        .or_else(|| (!state.runtime.is_empty()).then_some(state.runtime.as_str()))
        .or_else(|| fixed.get("runtime").map(String::as_str))
        .map(str::to_owned);
    let mut discovered = BTreeMap::new();
    loop {
        let candidates: Vec<_> = release
            .targets
            .iter()
            .filter(|t| {
                t.language == language
                    && selected_runtime.as_ref().is_none_or(|r| &t.runtime == r)
                    && t.when.iter().all(|(k, v)| {
                        fixed.get(k).is_none_or(|current| {
                            crate::project_config::scalar(v).as_ref() == Some(current)
                        })
                    })
            })
            .collect();
        if candidates.is_empty() {
            return Err(error(
                "module does not support the configured project conventions",
            ));
        }
        let missing: std::collections::BTreeSet<_> = candidates
            .iter()
            .flat_map(|t| t.when.keys())
            .filter(|key| !fixed.contains_key(*key))
            .cloned()
            .collect();
        let Some(field) = missing.into_iter().next() else {
            break;
        };
        let values: BTreeMap<_, _> = candidates
            .iter()
            .filter_map(|t| t.when.get(&field))
            .filter_map(|v| crate::project_config::scalar(v).map(|key| (key, v.clone())))
            .collect();
        let question_id = format!(
            "code:project-convention:{}",
            crate::generation::digest(&serde_json::to_string(
                &json!({"module":id,"version":release.version,"field":field,"values":values})
            )?)
        );
        let supplied = ctx["input"]["context"]
            .get(&field)
            .or_else(|| ctx["answers"].get(&question_id))
            .and_then(crate::project_config::scalar);
        let Some(supplied) = supplied else {
            return Ok(question(
                &question_id,
                &format!("Choose project convention {field}; it will be saved after approval"),
                json!({"type":"select","options":values.keys().collect::<Vec<_>>()}),
            ));
        };
        let selected = values
            .get(&supplied)
            .ok_or_else(|| error("invalid project convention choice"))?
            .clone();
        policy.conventions.insert(field.clone(), selected.clone());
        discovered.insert(field, selected);
        fixed = policy.fixed();
    }
    policy.validate()?;
    let targets: Vec<_> = release
        .targets
        .iter()
        .filter(|t| {
            t.language == language
                && selected_runtime.as_ref().is_none_or(|r| &t.runtime == r)
                && t.when
                    .iter()
                    .all(|(k, v)| crate::project_config::scalar(v).as_ref() == fixed.get(k))
        })
        .collect();
    if targets.is_empty() {
        return Err(error("module does not support the selected language"));
    }
    let mut runtimes = std::collections::BTreeSet::new();
    if targets
        .iter()
        .any(|target| !runtimes.insert(&target.runtime))
    {
        return Err(error(
            "ambiguous module targets for the configured project conventions",
        ));
    }
    if let Some(configured) = fixed.get("runtime")
        && (ctx["input"]["runtime"]
            .as_str()
            .is_some_and(|v| v != configured)
            || !state.runtime.is_empty() && state.runtime != *configured)
    {
        return Err(error(
            "runtime conflicts with lenso.code.toml; use a new target",
        ));
    }
    let runtime = ctx["input"]["runtime"]
        .as_str()
        .or_else(|| ctx["answers"][&rq].as_str())
        .or_else(|| (!state.runtime.is_empty()).then_some(state.runtime.as_str()))
        .or_else(|| fixed.get("runtime").map(String::as_str));
    let target = if let Some(runtime) = runtime {
        targets
            .iter()
            .find(|t| t.runtime == runtime)
            .copied()
            .ok_or_else(|| error("unsupported runtime for the selected module"))?
    } else if targets.len() == 1 {
        targets[0]
    } else {
        return Ok(question(
            &rq,
            "Choose the project runtime",
            json!({"type":"select","options":targets.iter().map(|t|&t.runtime).collect::<Vec<_>>()}),
        ));
    };
    if !state.runtime.is_empty() && state.runtime != target.runtime {
        return Err(error("changing runtime requires a new project target"));
    }
    let mut source = p.clone();
    source.format = "lenso-compose-v1".into();
    source.path = target.catalog.clone();
    source.r#ref = release.reference.clone();
    let legacy = crate::author::destination(&p.target, "composition.json");
    let legacy_matches = local(&legacy)?
        .map(|s| serde_json::from_str::<Value>(&s))
        .transpose()?
        .is_some_and(|v| v["source"] == source.source && v["catalog"] == source.path);
    let artifact = previous.map(|r| r.artifact.clone()).unwrap_or_else(|| {
        if legacy_matches {
            legacy
        } else {
            crate::author::destination(
                &p.target,
                &format!("code/modules/{}/{short}.json", d.namespace),
            )
        }
    });
    crate::relative(&artifact)?;
    if !artifact.starts_with(&format!("{}/", p.target)) {
        return Err(error("module artifact escapes target"));
    }
    if let Some(old) = previous
        && old.version == release.version
    {
        if old.profile.source != source.source
            || old.profile.r#ref != source.r#ref
            || old.root != entry.root
        {
            return Err(error(
                "published module descriptor changed without a new version",
            ));
        }
        if old.profile.path != source.path {
            if old.when == target.when {
                return Err(error(
                    "published module descriptor changed without a new version",
                ));
            }
            if !ctx["input"]["reconfigure"].as_bool().unwrap_or(false) {
                return Err(error(
                    "project conventions select a different implementation; preview with --reconfigure to review the change",
                ));
            }
        }
    }
    let mut context = ctx.clone();
    context["input"]["refresh"] = json!(refresh);
    context["capabilities"]["extension.options"]["selection"] =
        json!({"language":language,"runtime":target.runtime});
    for (key, value) in &discovered {
        context["input"]["context"][key] = json!(
            crate::project_config::scalar(value)
                .ok_or_else(|| error("invalid project convention"))?
        );
    }
    roots(&mut context, &entry.root, Some(&artifact))?;
    let loaded = crate::author::load_profile(&context, source.clone())?;
    let mut result = crate::composition::plan_with_policy(&context, loaded, &artifact, &policy)
        .map_err(|e| e.0)?;
    if result.0["status"] != "complete" {
        return Ok(result);
    }
    if !discovered.is_empty() {
        if !ctx["capabilities"]["host.reads"]
            .as_array()
            .is_some_and(|paths| paths.iter().any(|p| p == crate::project_config::FILE))
        {
            return Err(error("review Code setup before saving project conventions"));
        }
        let mut document = local(crate::project_config::FILE)?
            .map(|s| s.parse::<toml_edit::DocumentMut>())
            .transpose()?
            .unwrap_or_default();
        let encoded = toml_edit::ser::to_document(&policy)?;
        for key in discovered.keys() {
            document["conventions"][key] = encoded["conventions"][key].clone();
        }
        let content = document.to_string();
        result.0["output"]["files"]
            .as_array_mut()
            .ok_or_else(|| error("missing module files"))?
            .push(json!({"path":crate::project_config::FILE,"content":content}));
        result.0["output"]["expected"][crate::project_config::FILE] = json!(content);
    }
    let files = &result.0["output"]["composition"]["generation"]["files"];
    for (other, record) in &state.modules {
        if other == &id {
            continue;
        }
        crate::relative(&record.artifact)?;
        let owned: Value = serde_json::from_str(
            &local(&record.artifact)?.ok_or_else(|| error("module receipt is missing"))?,
        )?;
        if owned["generation"]["files"]
            .as_object()
            .is_some_and(|owned| owned.keys().any(|path| !files[path].is_null()))
        {
            return Err(error("generated output overlaps another installed module"));
        }
    }
    state.schema_version = 1;
    state.language = language;
    state.runtime = target.runtime.clone();
    state.modules.insert(
        id.clone(),
        Installed {
            when: target.when.clone(),
            version: release.version.clone(),
            profile: source,
            artifact,
            root: entry.root.clone(),
        },
    );
    let content = format!("{}\n", serde_json::to_string_pretty(&state)?);
    if local(&index_path)?.as_deref() != Some(&content) {
        result.0["output"]["files"]
            .as_array_mut()
            .ok_or_else(|| error("module plan has no files"))?
            .push(json!({"path":index_path,"content":content}));
    }
    result.0["output"]["expected"][&index_path] = json!(content);
    result.0["output"]["module"] = json!({"id":id,"version":release.version,"language":state.language,"runtime":state.runtime});
    Ok(result)
}
