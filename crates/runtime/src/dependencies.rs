use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use lenso_cli::Error;

use crate::project::{Installation, LoadMode, LoadOptions, LockedExtension, Project, ProjectLock};

pub(crate) fn installations(
    config: &Project,
    lock: &ProjectLock,
) -> Result<Vec<Installation>, Error> {
    for (id, preset) in &config.cli.presets {
        for (provider, dependency) in &preset.extensions {
            let entry = lock
                .extensions
                .iter()
                .find(|entry| entry.id == *provider)
                .ok_or_else(|| Error::Invalid(format!("preset {id} requires {provider}")))?;
            let req = semver::VersionReq::parse(&dependency.version)
                .map_err(|e| Error::Invalid(e.to_string()))?;
            let version = semver::Version::parse(&entry.definition.version)
                .map_err(|e| Error::Invalid(e.to_string()))?;
            if entry.source != dependency.source || !req.matches(&version) {
                return Err(Error::Invalid(format!(
                    "preset {id} conflicts with {provider} {version}"
                )));
            }
        }
    }
    let mut result = Vec::new();
    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    fn visit<'a>(
        id: &'a str,
        lock: &'a ProjectLock,
        visiting: &mut BTreeSet<&'a str>,
        visited: &mut BTreeSet<&'a str>,
    ) -> Result<(), Error> {
        if visited.contains(id) {
            return Ok(());
        }
        if !visiting.insert(id) {
            return Err(Error::Invalid(format!(
                "extension dependency cycle at {id}: {visiting:?}"
            )));
        }
        let entry = lock
            .extensions
            .iter()
            .find(|entry| entry.id == id)
            .ok_or_else(|| Error::Invalid(format!("missing extension dependency {id}")))?;
        crate::manifest::validate_metadata(&entry.definition)?;
        for (dependency, requirement) in &entry.definition.dependencies {
            let selected = lock
                .extensions
                .iter()
                .find(|entry| &entry.id == dependency)
                .ok_or_else(|| Error::Invalid(format!("{id} requires missing {dependency}")))?;
            let version = semver::Version::parse(&selected.definition.version)
                .map_err(|e| Error::Invalid(e.to_string()))?;
            let req = semver::VersionReq::parse(&requirement.version)
                .map_err(|e| Error::Invalid(e.to_string()))?;
            if selected.source != requirement.source || !req.matches(&version) {
                return Err(Error::Invalid(format!(
                    "{id} requires {dependency} {} from {}",
                    requirement.version, requirement.source
                )));
            }
            visit(dependency, lock, visiting, visited)?;
        }
        visiting.remove(id);
        visited.insert(id);
        Ok(())
    }
    for root in config.cli.extensions.iter().filter(|entry| entry.enabled) {
        visit(&root.id, lock, &mut visiting, &mut visited)?;
    }
    if visited.len() != lock.extensions.len() {
        return Err(Error::Invalid(
            "lock contains unreachable extensions; run lenso update".into(),
        ));
    }
    for entry in &lock.extensions {
        let installed = config
            .cli
            .extensions
            .iter()
            .find(|root| root.enabled && root.id == entry.id)
            .cloned()
            .unwrap_or(Installation::new(
                entry.id.clone(),
                entry.source.clone(),
                None,
            )?);
        result.push(installed);
    }
    Ok(result)
}

#[derive(Clone)]
struct Constraint {
    source: String,
    version: String,
    parent: String,
}

pub(crate) fn resolve(
    project: &Path,
    config: &Project,
    options: &LoadOptions,
) -> Result<ProjectLock, Error> {
    if options.locked && options.mode == LoadMode::Update {
        return Err(Error::Invalid(
            "update cannot be combined with --locked".into(),
        ));
    }
    let previous = crate::project::read_lock(project)?;
    if options.mode != LoadMode::Update
        && let Some(lock) = &previous
        && crate::project::declarations_match(lock, config)
        && installations(config, lock).is_ok()
    {
        for entry in &lock.extensions {
            if let Some(id) = entry.source.strip_prefix("builtin:")
                && options
                    .providers
                    .get(id)
                    .is_none_or(|provider| provider.version != entry.definition.version)
            {
                return Err(lenso_cli::Diagnostic::new(lenso_cli::DiagnosticCode::DependencyUnavailable)
                    .with_details(serde_json::json!({"provider":id,"reason":"native_provider_changed_or_unavailable"}))
                    .with_retryable(false).with_recovery("review_provider", "Use a compatible Host, or explicitly review the provider upgrade and update the lock.").into());
            }
        }
        return Ok(lock.clone());
    }
    if options.locked {
        return Err(Error::Invalid(
            "lenso.lock is missing or declarations changed with --locked".into(),
        ));
    }
    let mut constraints: BTreeMap<String, Vec<Constraint>> = BTreeMap::new();
    for root in config.cli.extensions.iter().filter(|entry| entry.enabled) {
        constraints
            .entry(root.id.clone())
            .or_default()
            .push(Constraint {
                source: root.source.clone(),
                version: root.version.clone().unwrap_or("*".into()),
                parent: "[project declaration]".into(),
            });
    }
    for (id, preset) in &config.cli.presets {
        for (provider, dependency) in &preset.extensions {
            constraints
                .entry(provider.clone())
                .or_default()
                .push(Constraint {
                    source: dependency.source.clone(),
                    version: dependency.version.clone(),
                    parent: id.clone(),
                });
        }
    }
    let mut resolver = Resolver {
        project,
        config,
        options,
        preferred: previous
            .as_ref()
            .map(|lock| {
                lock.extensions
                    .iter()
                    .map(|entry| (entry.id.clone(), entry.clone()))
                    .collect()
            })
            .unwrap_or_default(),
        attempts: 0,
    };
    let entries = resolver.search(constraints, BTreeMap::new())?;
    let lock = ProjectLock {
        schema_version: 1,
        presets: config.cli.presets.clone(),
        extensions: entries.into_values().collect(),
        resources: previous.map(|lock| lock.resources).unwrap_or_default(),
    };
    installations(config, &lock)?;
    Ok(lock)
}

struct Resolver<'a> {
    project: &'a Path,
    config: &'a Project,
    options: &'a LoadOptions,
    preferred: BTreeMap<String, LockedExtension>,
    attempts: usize,
}

fn dependency_chain(parent: &str, constraints: &BTreeMap<String, Vec<Constraint>>) -> String {
    let mut path = vec![parent];
    let mut seen = BTreeSet::from([parent]);
    let mut cursor = parent;
    while let Some(requirement) = constraints.get(cursor).and_then(|values| values.first()) {
        cursor = &requirement.parent;
        path.push(cursor);
        if !seen.insert(cursor) {
            break;
        }
    }
    path.reverse();
    path.join(" -> ")
}

impl Resolver<'_> {
    fn search(
        &mut self,
        constraints: BTreeMap<String, Vec<Constraint>>,
        selected: BTreeMap<String, LockedExtension>,
    ) -> Result<BTreeMap<String, LockedExtension>, Error> {
        self.attempts += 1;
        if self.attempts > 512 || constraints.len() > 128 {
            return Err(Error::Invalid(
                "extension dependency resolution limit exceeded".into(),
            ));
        }
        let (project, config, options) = (self.project, self.config, self.options);
        for (id, requirements) in &constraints {
            if let Some(entry) = selected.get(id) {
                let version = semver::Version::parse(&entry.definition.version)
                    .map_err(|e| Error::Invalid(e.to_string()))?;
                for req in requirements {
                    if req.source != entry.source
                        || !semver::VersionReq::parse(&req.version)
                            .map_err(|e| Error::Invalid(e.to_string()))?
                            .matches(&version)
                    {
                        return Err(Error::Invalid(format!(
                            "dependency conflict: {} -> {id} {} from {}; selected {} from {}",
                            dependency_chain(&req.parent, &constraints),
                            req.version,
                            req.source,
                            version,
                            entry.source
                        )));
                    }
                }
            }
        }
        let Some((id, requirements)) = constraints
            .iter()
            .find(|(id, _)| !selected.contains_key(*id))
        else {
            return Ok(selected);
        };
        if requirements
            .iter()
            .any(|req| req.source != requirements[0].source)
        {
            return Err(Error::Invalid(format!(
                "conflicting sources for {id}: {}",
                requirements
                    .iter()
                    .map(|req| format!(
                        "{} -> {id} from {}",
                        dependency_chain(&req.parent, &constraints),
                        req.source
                    ))
                    .collect::<Vec<_>>()
                    .join("; ")
            )));
        }
        let mut requirement = requirements
            .iter()
            .map(|req| req.version.as_str())
            .filter(|req| *req != "*")
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .join(", ");
        if requirement.is_empty() {
            requirement = "*".into();
        }
        let mut last = None;
        let mut preferred = self
            .preferred
            .get(id)
            .filter(|entry| {
                options.mode != LoadMode::Update
                    && entry.source == requirements[0].source
                    && entry.registry.as_deref()
                        == crate::registry::route(&entry.source, &config.cli.registries)
                            .ok()
                            .flatten()
                    && entry
                        .github
                        .as_ref()
                        .is_none_or(|pin| pin.api == config.cli.github.api)
                    && semver::Version::parse(&entry.definition.version)
                        .ok()
                        .is_some_and(|version| {
                            semver::VersionReq::parse(&requirement)
                                .ok()
                                .is_some_and(|req| req.matches(&version))
                        })
            })
            .cloned();
        let mut tried = BTreeSet::new();
        loop {
            let mut candidate = config.clone();
            candidate.cli.extensions = vec![Installation::new(
                id.clone(),
                requirements[0].source.clone(),
                Some(requirement.clone()),
            )?];
            let mut resolution = options.clone();
            resolution.locked = false;
            let resolved = preferred.take().map(Ok).unwrap_or_else(|| {
                crate::project::resolve_flat(project, &candidate, &resolution)
                    .map(|mut lock| lock.extensions.remove(0))
            });
            let mut entry = match resolved {
                Ok(entry) => entry,
                Err(error) => {
                    if matches!(&error, Error::Diagnostic(_) | Error::Execution { .. }) {
                        return Err(last.unwrap_or(error));
                    }
                    return Err(last.unwrap_or_else(|| {
                        Error::Invalid(format!(
                            "{} -> {id}: {error}",
                            requirements
                                .iter()
                                .map(|req| dependency_chain(&req.parent, &constraints))
                                .collect::<Vec<_>>()
                                .join("; ")
                        ))
                    }));
                }
            };
            let version = entry.definition.version.clone();
            if !tried.insert(version.clone()) {
                requirement = if requirement == "*" {
                    format!("<{version}")
                } else {
                    format!("{requirement}, <{version}")
                };
                continue;
            }
            entry.requirement = config
                .cli
                .extensions
                .iter()
                .find(|root| root.id == *id)
                .and_then(|root| root.version.clone());
            let mut next = selected.clone();
            next.insert(id.clone(), entry.clone());
            let mut pending = constraints.clone();
            for (dependency, req) in &entry.definition.dependencies {
                pending
                    .entry(dependency.clone())
                    .or_default()
                    .push(Constraint {
                        source: req.source.clone(),
                        version: req.version.clone(),
                        parent: id.clone(),
                    });
            }
            match self.search(pending, next) {
                Ok(solution) => {
                    let test = ProjectLock {
                        schema_version: 1,
                        presets: config.cli.presets.clone(),
                        extensions: solution.values().cloned().collect(),
                        resources: BTreeMap::new(),
                    };
                    match installations(config, &test) {
                        Ok(_) => return Ok(solution),
                        Err(error) => last = Some(error),
                    }
                }
                Err(error) => last = Some(error),
            }
            if tried.len() == 1
                && self
                    .preferred
                    .get(id)
                    .is_some_and(|entry| entry.definition.version == version)
                && options.mode != LoadMode::Update
                && (requirements[0].source.starts_with("registry:")
                    || requirements[0].source.starts_with("github-release:"))
            {
                continue;
            }
            // Excluding the failed candidate allows earlier selections to backtrack.
            requirement = if requirement == "*" {
                format!("<{version}")
            } else {
                format!("{requirement}, <{version}")
            };
            if requirements[0].source.starts_with("builtin:")
                || (!requirements[0].source.starts_with("registry:")
                    && !requirements[0].source.starts_with("github-release:"))
            {
                return Err(last.expect("failed candidate has a diagnostic"));
            }
        }
    }
}
