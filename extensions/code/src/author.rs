use crate::generation::{OwnedFile, digest};
use crate::{Profile, ResourceFile, complete, error, local, registry, relative, remote};
use extism_pdk::{FnResult, Json, plugin_fn};
use lenso_codegen::{
    Dependency, Generator, GeneratorInput, GeneratorSpec, Scope, TemplateGenerator,
};
use lenso_composition::{Catalog, Evaluation, Module, Request, Resolution};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    cell::{OnceCell, RefCell},
    collections::{BTreeMap, BTreeSet},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub schema_version: u32,
    pub modules: BTreeMap<String, Module>,
    #[serde(default)]
    pub resources: BTreeMap<String, Vec<ResourceFile>>,
    #[serde(default)]
    pub generator: Option<GeneratorSpec>,
    #[serde(default)]
    pub samples: BTreeMap<String, Request>,
}
pub struct Loaded {
    pub profile: Profile,
    pub document: Document,
    pub catalog: Catalog,
    pub commit: String,
    handle: Option<String>,
    // Freeze author inputs for this call; sample determinism checks share the same snapshot.
    sources: RefCell<BTreeMap<String, String>>,
    templates: OnceCell<BTreeMap<String, String>>,
}
pub struct Rendered {
    pub files: BTreeMap<String, OwnedFile>,
    pub dependencies: Vec<Dependency>,
    pub generated_directory: Option<String>,
}
pub fn destination(root: &str, path: &str) -> String {
    if root.is_empty() {
        path.into()
    } else {
        format!("{root}/{path}")
    }
}
pub fn load(ctx: &Value) -> crate::Result<Result<Loaded, Json<Value>>> {
    let name = match registry(ctx)? {
        Ok(name) => name,
        Err(question) => return Ok(Err(question)),
    };
    let profile: Profile = serde_json::from_value(
        ctx["capabilities"]["extension.options"]["registries"][&name].clone(),
    )?;
    if profile.format != "lenso-compose-v1" {
        return Err(error(
            "composition commands require a lenso-compose-v1 resource registry",
        ));
    }
    if !profile.target.is_empty() {
        lenso_codegen::relative(&profile.target)?;
    }
    lenso_codegen::relative(&profile.path)?;
    let (handle, commit, content) = if let Some(root) = profile.source.strip_prefix("project:") {
        relative(root)?;
        let path = destination(root, &profile.path);
        let content =
            local(&path)?.ok_or_else(|| error(format!("missing author catalog {path}")))?;
        (None, format!("local:{}", digest(&content)), content)
    } else {
        // SAFETY: Extism validates handles; the Host applies source grants and lock policy.
        let snapshot: Value = serde_json::from_str(&unsafe {
            crate::source_resolve(json!({"source":profile.source,"reference":profile.r#ref,"refresh":ctx["input"]["refresh"].as_bool().unwrap_or(false)}).to_string())?
        })?;
        let handle = snapshot["handle"]
            .as_str()
            .ok_or_else(|| error("snapshot handle missing"))?
            .to_owned();
        let commit = snapshot["commit"]
            .as_str()
            .ok_or_else(|| error("snapshot commit missing"))?
            .to_owned();
        let content = remote(&handle, &profile.path)?;
        (Some(handle), commit, content)
    };
    let document: Document = toml::from_str(&content)?;
    let catalog = Catalog {
        schema_version: document.schema_version,
        modules: document.modules.clone(),
    };
    catalog.validate()?;
    let loaded = Loaded {
        profile,
        document,
        catalog,
        commit,
        handle,
        sources: RefCell::new(BTreeMap::new()),
        templates: OnceCell::new(),
    };
    loaded.validate_declarations()?;
    Ok(Ok(loaded))
}
impl Loaded {
    pub fn read(&self, path: &str) -> crate::Result<String> {
        lenso_codegen::relative(path)?;
        if let Some(content) = self.sources.borrow().get(path) {
            return Ok(content.clone());
        }
        let source = path.to_owned();
        let path = self
            .profile
            .path
            .rsplit_once('/')
            .map_or_else(|| path.to_owned(), |(parent, _)| destination(parent, path));
        let content = if let Some(handle) = &self.handle {
            remote(handle, &path)?
        } else {
            let root = self
                .profile
                .source
                .strip_prefix("project:")
                .ok_or_else(|| error("missing author root"))?;
            let path = destination(root, &path);
            local(&path)?.ok_or_else(|| error(format!("missing author source {path}")))?
        };
        let mut sources = self.sources.borrow_mut();
        if content.len() + sources.values().map(String::len).sum::<usize>() > 1024 * 1024 {
            return Err(error("author sources exceed 1 MiB"));
        }
        sources.insert(source, content.clone());
        Ok(content)
    }
    fn validate_declarations(&self) -> crate::Result<()> {
        if self.document.resources.len() > 128 || self.document.samples.len() > 16 {
            return Err(error("oversized author catalog"));
        }
        for module in self.catalog.modules.keys() {
            lenso_codegen::relative(module)?;
        }
        for (module, resources) in &self.document.resources {
            if !self.catalog.modules.contains_key(module) || resources.len() > 127 {
                return Err(error("invalid composition resources"));
            }
            let mut targets = BTreeSet::new();
            for resource in resources {
                let (source, target) = resource_paths(resource);
                lenso_codegen::relative(source)?;
                lenso_codegen::relative(target)?;
                let folded = target.to_lowercase();
                if targets.iter().any(|other: &String| {
                    folded.starts_with(&format!("{other}/"))
                        || other.starts_with(&format!("{folded}/"))
                }) {
                    return Err(error("module destinations cannot contain each other"));
                }
                if !targets.insert(target.to_lowercase()) {
                    return Err(error("duplicate module destination"));
                }
            }
        }
        for sample in self.document.samples.keys() {
            if sample.is_empty()
                || !sample
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
            {
                return Err(error("invalid sample name"));
            }
        }
        if let Some(spec) = &self.document.generator {
            spec.validate(&self.catalog.modules.keys().cloned().collect())?;
            for file in &spec.files {
                if file.scope == Scope::Module {
                    for (module, resources) in &self.document.resources {
                        if file.module.as_ref().is_some_and(|filter| filter != module) {
                            continue;
                        }
                        for resource in resources {
                            let target = resource_paths(resource).1.to_lowercase();
                            let output = file.target.to_lowercase();
                            if target == output
                                || target.starts_with(&format!("{output}/"))
                                || output.starts_with(&format!("{target}/"))
                            {
                                return Err(error(
                                    "template declaration conflicts with a copied source destination",
                                ));
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
    fn templates(&self) -> crate::Result<BTreeMap<String, String>> {
        if let Some(templates) = self.templates.get() {
            return Ok(templates.clone());
        }
        let mut templates = BTreeMap::new();
        if let Some(spec) = &self.document.generator {
            let mut bytes = 0;
            for file in &spec.files {
                if !templates.contains_key(&file.template) {
                    let content = self.read(&file.template)?;
                    bytes += content.len();
                    if bytes > 1024 * 1024 {
                        return Err(error("template sources exceed 1 MiB"));
                    }
                    templates.insert(file.template.clone(), content);
                }
            }
            TemplateGenerator {
                spec,
                templates: &templates,
            }
            .validate_templates()?;
        }
        let _ = self.templates.set(templates.clone());
        Ok(templates)
    }
    pub fn render(&self, request: &Request, resolution: &Resolution) -> crate::Result<Rendered> {
        let directory = self
            .document
            .generator
            .as_ref()
            .map(|spec| spec.directory.clone());
        let root = directory.as_ref().map_or_else(
            || self.profile.target.clone(),
            |directory| destination(&self.profile.target, directory),
        );
        let mut files = BTreeMap::new();
        let selected: BTreeSet<_> = resolution
            .instances
            .values()
            .map(|instance| &instance.module)
            .collect();
        for module in selected {
            for resource in self.document.resources.get(module).into_iter().flatten() {
                let (source, target) = resource_paths(resource);
                let path = destination(&root, &format!("modules/{module}/{target}"));
                let content = self.read(source)?;
                files.insert(
                    path,
                    OwnedFile {
                        scope: Scope::Module,
                        module: module.clone(),
                        owners: resolution
                            .instances
                            .iter()
                            .filter(|(_, instance)| &instance.module == module)
                            .map(|(id, _)| id.clone())
                            .collect(),
                        sha256: digest(&content),
                        base: content,
                    },
                );
            }
        }
        let mut dependencies = Vec::new();
        if let Some(spec) = &self.document.generator {
            let templates = self.templates()?;
            let input = GeneratorInput {
                schema_version: 1,
                request: request.clone(),
                resolution: resolution.clone(),
            };
            let output = TemplateGenerator {
                spec,
                templates: &templates,
            }
            .generate(&input)?;
            dependencies = output.dependencies;
            for file in output.files {
                let path = destination(&root, &file.path);
                let owned = OwnedFile {
                    scope: file.scope,
                    module: file.module.unwrap_or_default(),
                    owners: file.owners,
                    sha256: digest(&file.content),
                    base: file.content,
                };
                if files.insert(path, owned).is_some() {
                    return Err(error("template output conflicts with copied source"));
                }
            }
        }
        if files.len() > 127
            || files.values().map(|file| file.base.len()).sum::<usize>() > 1024 * 1024
        {
            return Err(error("generation exceeds project plan limits"));
        }
        for path in files.keys() {
            if files.keys().any(|other| {
                path != other
                    && (path.eq_ignore_ascii_case(other)
                        || path
                            .to_lowercase()
                            .starts_with(&format!("{}/", other.to_lowercase())))
            }) {
                return Err(error(format!("conflicting generation path {path}")));
            }
        }
        Ok(Rendered {
            files,
            dependencies,
            generated_directory: directory,
        })
    }
}
fn resource_paths(resource: &ResourceFile) -> (&str, &str) {
    match resource {
        ResourceFile::Path(path) => (path, path),
        ResourceFile::Target(file) => (&file.path, &file.target),
    }
}
#[plugin_fn]
pub fn validate(Json(ctx): Json<Value>) -> FnResult<Json<Value>> {
    let loaded = match load(&ctx)? {
        Ok(loaded) => loaded,
        Err(question) => return Ok(question),
    };
    let templates = loaded.templates()?;
    let mut source_bytes = templates.values().map(String::len).sum::<usize>();
    for files in loaded.document.resources.values() {
        for resource in files {
            source_bytes += loaded.read(resource_paths(resource).0)?.len();
            if source_bytes > 1024 * 1024 {
                return Err(error("author sources exceed 1 MiB").into());
            }
        }
    }
    if loaded.document.generator.is_some() && loaded.document.samples.is_empty() {
        return Err(
            error("a generator catalog requires at least one complete author sample").into(),
        );
    }
    let mut results = Vec::new();
    let mut covered = BTreeSet::new();
    for (name, request) in &loaded.document.samples {
        let resolution = match loaded.catalog.evaluate(request)? {
            Evaluation::Complete { resolution } => resolution,
            Evaluation::NeedsInput { choice } => {
                return Err(error(format!(
                    "sample {name} is incomplete: {}",
                    serde_json::to_string(&choice)?
                ))
                .into());
            }
        };
        let first = loaded.render(request, &resolution)?;
        let second = loaded.render(request, &resolution)?;
        crate::native::validate(&first.dependencies, &resolution)?;
        let first_bytes = serde_json::to_vec(&first.files)?;
        if first_bytes != serde_json::to_vec(&second.files)?
            || first.dependencies != second.dependencies
        {
            return Err(error(format!("sample {name} generated nondeterministic output")).into());
        }
        covered.extend(
            resolution
                .instances
                .values()
                .map(|instance| instance.module.clone()),
        );
        results.push(json!({"sample":name,"files":first.files.len(),"instances":resolution.instances.len(),"generation_sha256":digest(std::str::from_utf8(&first_bytes)?),"dependencies":first.dependencies}));
    }
    let uncovered: Vec<_> = loaded
        .catalog
        .modules
        .keys()
        .filter(|module| !covered.contains(*module))
        .collect();
    Ok(complete(
        json!({"message":format!("Validated {} modules and {} author samples",loaded.catalog.modules.len(),results.len()),"validated":true,"commit":loaded.commit,"samples":results,"uncovered_modules":uncovered}),
    ))
}
