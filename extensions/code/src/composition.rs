use std::collections::BTreeSet;

use extism_pdk::{FnResult, Json, plugin_fn};
use lenso_composition::{Binding, Choice, Evaluation, OptionKind, Request, Resolution};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::author::destination;
use crate::{complete, error, local, question};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Artifact {
    schema_version: u32,
    source: String,
    catalog: String,
    commit: String,
    request: Request,
    resolution: Resolution,
    digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    generation: Option<crate::generation::Receipt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    generated_directory: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    dependencies: Vec<lenso_codegen::Dependency>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    native_catalog: Option<String>,
}

fn answer(request: &mut Request, choice: &Choice, value: &Value) -> crate::Result<()> {
    match choice {
        Choice::Context { field, values } => {
            let value = value
                .as_str()
                .filter(|value| values.iter().any(|allowed| allowed == value))
                .ok_or_else(|| error("invalid project context choice"))?;
            request.context.insert(field.clone(), value.into());
        }
        Choice::Provider {
            slot,
            modules,
            instances,
            optional,
            ..
        } => {
            let text = value
                .as_str()
                .ok_or_else(|| error("provider choice must be a string"))?;
            let binding = if text == "none" && *optional {
                Binding::Disabled
            } else if let Some(module) = text
                .strip_prefix("module:")
                .filter(|module| modules.iter().any(|allowed| allowed == module))
            {
                Binding::Module {
                    module: module.into(),
                }
            } else if let Some(instance) = text
                .strip_prefix("instance:")
                .filter(|instance| instances.iter().any(|allowed| allowed == instance))
            {
                Binding::Instance {
                    instance: instance.into(),
                }
            } else {
                return Err(error("invalid provider choice"));
            };
            request.bindings.insert(slot.clone(), binding);
        }
        Choice::Configuration {
            instance,
            field,
            option,
        } => {
            let value = match &option.kind {
                OptionKind::Boolean => match value.as_str() {
                    Some("true") => json!(true),
                    Some("false") => json!(false),
                    _ => return Err(error("choose true or false")),
                },
                OptionKind::Integer { .. } => json!(
                    value
                        .as_str()
                        .ok_or_else(|| error("integer answer must be text"))?
                        .parse::<i64>()?
                ),
                OptionKind::Environment => {
                    json!({"env":value.as_str().ok_or_else(||error("environment answer must name a variable"))?})
                }
                _ => value.clone(),
            };
            if !option.accepts(&value) {
                return Err(error(format!("invalid value for {instance}.{field}")));
            }
            request
                .configuration
                .entry(instance.clone())
                .or_default()
                .insert(field.clone(), value);
        }
    }
    Ok(())
}

fn prompt(choice: &Choice) -> (String, Value) {
    match choice {
        Choice::Context { field, values } => (
            format!("Project {field}"),
            json!({"type":"select","options":values}),
        ),
        Choice::Provider {
            slot,
            contract,
            modules,
            instances,
            optional,
        } => {
            let mut choices = modules
                .iter()
                .map(|id| format!("module:{id}"))
                .chain(instances.iter().map(|id| format!("instance:{id}")))
                .collect::<Vec<_>>();
            if *optional {
                choices.push("none".into());
            }
            (
                format!("Provider for {slot} ({contract})"),
                json!({"type":"select","options":choices}),
            )
        }
        Choice::Configuration {
            instance,
            field,
            option,
        } => {
            let kind = match &option.kind {
                OptionKind::Boolean => json!({"type":"select","options":["false","true"]}),
                OptionKind::Select { values } => json!({"type":"select","options":values}),
                _ => json!({"type":"text"}),
            };
            (format!("{instance}.{field}: {}", option.prompt), kind)
        }
    }
}

#[plugin_fn]
pub fn compose(Json(ctx): Json<Value>) -> FnResult<Json<Value>> {
    let loaded = match crate::author::load(&ctx)? {
        Ok(loaded) => loaded,
        Err(question) => return Ok(question),
    };
    let path = destination(&loaded.profile.target, "composition.json");
    plan(&ctx, loaded, &path)
}
pub(crate) fn plan(
    ctx: &Value,
    mut loaded: crate::author::Loaded,
    artifact_path: &str,
) -> FnResult<Json<Value>> {
    let policy = crate::project_config::load(ctx)?;
    if let Some(root) = &policy.paths.modules {
        let prefix = format!("{}/", loaded.profile.target);
        let relative=if root==&loaded.profile.target{Some("")}else{root.strip_prefix(&prefix)}.ok_or_else(||error("paths.modules must stay within the configured Code target; review Code setup to change target grants"))?;
        if let Some(generator) = loaded.document.generator.as_mut() {
            let family = generator
                .directory
                .rsplit('/')
                .next()
                .unwrap_or("module")
                .to_owned();
            generator.directory = if relative.is_empty() {
                family
            } else {
                format!("{relative}/{family}")
            };
        }
    }
    let profile = &loaded.profile;
    let commit = &loaded.commit;
    let catalog = &loaded.catalog;
    let previous: Option<Artifact> = local(artifact_path)?
        .map(|content| serde_json::from_str(&content))
        .transpose()?;
    if let Some(previous) = &previous {
        if previous.schema_version != 1
            || previous.source != profile.source
            || previous.catalog != profile.path
                && !ctx["input"]["reconfigure"].as_bool().unwrap_or(false)
        {
            return Err(error("composition artifact belongs to a different source").into());
        }
        if let Some(receipt) = &previous.generation {
            let root = previous.generated_directory.as_ref().map_or_else(
                || profile.target.clone(),
                |directory| destination(&profile.target, directory),
            );
            if let Some(directory) = &previous.generated_directory {
                lenso_codegen::relative(directory)?;
            }
            for (path, file) in &receipt.files {
                let module_owners: BTreeSet<_> = previous
                    .resolution
                    .instances
                    .iter()
                    .filter(|(_, instance)| instance.module == file.module)
                    .map(|(id, _)| id.clone())
                    .collect();
                let valid = match file.scope {
                    lenso_codegen::Scope::Module => {
                        file.owners == module_owners
                            && path.starts_with(&destination(
                                &root,
                                &format!("modules/{}/", file.module),
                            ))
                    }
                    lenso_codegen::Scope::Instance => {
                        file.owners.len() == 1
                            && file.owners.is_subset(&module_owners)
                            && file.owners.iter().all(|id| {
                                path.starts_with(&destination(&root, &format!("instances/{id}/")))
                            })
                            && previous.generated_directory.is_some()
                    }
                    lenso_codegen::Scope::Composition => {
                        file.module.is_empty()
                            && file.owners
                                == previous.resolution.instances.keys().cloned().collect()
                            && path.starts_with(&destination(&root, "assembly/"))
                            && previous.generated_directory.is_some()
                    }
                };
                if !valid || file.owners.is_empty() {
                    return Err(error("generation receipt path or ownership does not match the previous composition").into());
                }
            }
        }
    }
    let mut request = Request::default();
    if let Some(instances) = ctx["input"].get("instances") {
        request.instances = serde_json::from_value(instances.clone())?;
    }
    if let Some(module) = ctx["input"]["module"].as_str() {
        if !request.instances.is_empty() {
            return Err(error("choose a module argument or explicit instances, not both").into());
        }
        request.instances.insert("app".into(), module.into());
    }
    if request.instances.is_empty()
        && ctx["input"]["sample"].is_null()
        && let Some(previous) = &previous
    {
        request = previous.request.clone();
    }
    if let Some(sample) = ctx["input"]["sample"].as_str() {
        if !request.instances.is_empty() {
            return Err(error("choose a sample or explicit roots, not both").into());
        }
        request = loaded
            .document
            .samples
            .get(sample)
            .ok_or_else(|| error(format!("unknown author sample {sample}")))?
            .clone();
    }
    if request.instances.is_empty() && !loaded.document.samples.is_empty() {
        let id = format!("compose-sample:{commit}");
        if let Some(sample) = ctx["answers"][&id].as_str() {
            request = loaded
                .document
                .samples
                .get(sample)
                .ok_or_else(|| error("unknown author sample"))?
                .clone();
        } else {
            return Ok(question(
                &id,
                "Choose an author sample",
                json!({"type":"select","options":loaded.document.samples.keys().collect::<Vec<_>>()}),
            ));
        }
    }
    if request.instances.is_empty() {
        let id = format!("compose-root:{commit}");
        if let Some(module) = ctx["answers"][&id].as_str() {
            request.instances.insert("app".into(), module.into());
        } else {
            return Ok(question(
                &id,
                "Choose the root module",
                json!({"type":"select","options":catalog.modules.keys().collect::<Vec<_>>()}),
            ));
        }
    }
    if let Some(value) = ctx["input"].get("configuration") {
        request.configuration = serde_json::from_value(value.clone())?;
    }
    if let Some(value) = ctx["input"].get("bindings") {
        request.bindings = serde_json::from_value(value.clone())?;
    }
    if let Some(value) = ctx["input"].get("context") {
        request.context = serde_json::from_value(value.clone())?;
    }
    if let Some(selection) = ctx["capabilities"]["extension.options"]["selection"].as_object() {
        for field in ["language", "runtime"] {
            if let Some(value) = selection.get(field).and_then(Value::as_str) {
                if request
                    .context
                    .get(field)
                    .is_some_and(|existing| existing != value)
                {
                    return Err(error(format!("composition {field} conflicts with the installed target; configure a new project target")).into());
                }
                request.context.insert(field.into(), value.into());
            }
        }
    }
    policy.constrain(
        &mut request,
        ctx["input"]["reconfigure"].as_bool().unwrap_or(false),
    )?;
    policy.module_defaults(&mut request);
    let resolution = loop {
        match catalog.evaluate(&request)? {
            Evaluation::Complete { resolution } => break resolution,
            Evaluation::NeedsInput { choice } => {
                if let Choice::Context { field, .. } = &choice
                    && policy.fixed().contains_key(field)
                {
                    return Err(error(format!(
                        "module does not support project convention {field}"
                    ))
                    .into());
                }
                if let Some(binding) = policy.provider(&choice)?
                    && let Choice::Provider { slot, .. } = &choice
                {
                    request.bindings.insert(slot.clone(), binding);
                    policy.module_defaults(&mut request);
                    continue;
                }

                let choice_bytes = serde_json::to_vec(
                    &json!({"commit":commit,"request":request,"choice":choice}),
                )?;
                let id = format!("compose:{:x}", Sha256::digest(choice_bytes));
                if let Some(value) = ctx["answers"].get(&id) {
                    answer(&mut request, &choice, value)?;
                } else {
                    let (message, kind) = prompt(&choice);
                    return Ok(question(&id, &message, kind));
                }
            }
        }
    };
    // Preserve validated defaults and automatically selected bindings for future reconfiguration.
    for (id, instance) in &resolution.instances {
        request
            .configuration
            .insert(id.clone(), instance.configuration.clone());
    }
    for (id, instance) in &resolution.instances {
        for (slot, target) in &instance.bindings {
            if let Some(target) = target {
                let binding = if request.instances.contains_key(target) {
                    Binding::Instance {
                        instance: target.clone(),
                    }
                } else {
                    Binding::Module {
                        module: resolution.instances[target].module.clone(),
                    }
                };
                request.bindings.insert(format!("{id}.{slot}"), binding);
            }
        }
    }
    let digest = resolution.digest()?;
    let modules = resolution
        .instances
        .values()
        .map(|instance| instance.module.clone())
        .collect::<BTreeSet<_>>();
    let rendered = loaded.render(&request, &resolution)?;
    let changes = match crate::generation::reconcile(
        ctx,
        rendered.files,
        previous
            .as_ref()
            .and_then(|artifact| artifact.generation.as_ref()),
    )? {
        Ok(changes) => changes,
        Err(question) => return Ok(question),
    };
    let native_catalog = previous
        .as_ref()
        .map(|a| a.native_catalog.as_deref().unwrap_or(&a.catalog))
        .unwrap_or(&profile.path)
        .to_owned();
    lenso_codegen::relative(&native_catalog)?;
    let group = crate::generation::digest(&serde_json::to_string(
        &json!({"source":profile.source,"catalog":native_catalog,"artifact":artifact_path}),
    )?);
    if ctx["capabilities"]["extension.options"]["native"].is_null()
        && previous
            .as_ref()
            .is_some_and(|artifact| !artifact.dependencies.is_empty())
    {
        return Err(error(
            "retire native dependencies before removing native target configuration",
        )
        .into());
    }
    let native = match crate::native::plan(ctx, &rendered.dependencies, &resolution, &group)? {
        Ok(plan) => plan,
        Err(question) => return Ok(question),
    };
    let mut files = changes.files;
    files.extend(native.files);

    let artifact = Artifact {
        schema_version: 1,
        source: profile.source.clone(),
        catalog: profile.path.clone(),
        commit: commit.clone(),
        request,
        resolution,
        digest: digest.clone(),
        generation: Some(changes.receipt),
        generated_directory: rendered.generated_directory,
        dependencies: rendered.dependencies,
        native_catalog: Some(native_catalog),
    };
    let content = format!("{}\n", serde_json::to_string_pretty(&artifact)?);
    if files
        .iter()
        .filter_map(|file| file["content"].as_str())
        .map(str::len)
        .sum::<usize>()
        + content.len()
        > 1024 * 1024
    {
        return Err(error("composition artifact exceeds project plan limits").into());
    }
    if local(artifact_path)?.as_deref() != Some(&content) {
        files.push(json!({"path":artifact_path,"content":content}));
    }
    Ok(complete(
        json!({"message":format!("Composed {} instances from {} module definitions",artifact.resolution.instances.len(),modules.len()),"files":files,"deletions":changes.deletions,"expected":changes.expected,"summary":changes.summary,"commands":native.commands,"finalizations":native.finalizations,"native":native.summary,"dependencies":artifact.dependencies,"items":modules,"composition":artifact,"composition_id":digest}),
    ))
}
