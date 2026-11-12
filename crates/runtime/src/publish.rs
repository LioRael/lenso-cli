//! Author-only GitHub publishing; consumers continue to download prebuilt artifacts.
use crate::{
    GithubConfig, LoadOptions,
    cache::digest,
    github::{Github, Repo},
    package::{ReleaseMetadata, archive_files, publication_conventions},
    project::{atomic_write, read_bounded},
};
use lenso_cli::{Context, Error, Outcome, Question, QuestionKind};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Clone)]
struct Options {
    repository: Repo,
    config: GithubConfig,
    target: String,
    draft: bool,
    dry_run: bool,
    output: PathBuf,
    cache: crate::Cache,
}
struct Asset {
    name: String,
    bytes: Vec<u8>,
    sha256: String,
}
#[derive(Clone, Deserialize, Serialize)]
struct RemoteAsset {
    id: u64,
    name: String,
    size: u64,
    state: String,
    #[serde(default)]
    digest: Option<String>,
}
#[derive(Clone, Deserialize, Serialize)]
struct Release {
    id: u64,
    tag_name: String,
    target_commitish: String,
    draft: bool,
    prerelease: bool,
    #[serde(default)]
    immutable: bool,
    upload_url: String,
    html_url: String,
    #[serde(default)]
    assets: Vec<RemoteAsset>,
}
struct Prepared {
    options: Options,
    metadata: ReleaseMetadata,
    tag: String,
    assets: Vec<Asset>,
    commit: String,
    existing: Option<Release>,
}
fn text<'a>(ctx: &'a Context, name: &str) -> Result<Option<&'a str>, Error> {
    ctx.input
        .get(name)
        .map(|value| {
            value
                .as_str()
                .filter(|v| !v.is_empty())
                .ok_or_else(|| Error::Invalid(format!("--{name} needs a nonempty string")))
        })
        .transpose()
}
fn flag(ctx: &Context, name: &str) -> Result<bool, Error> {
    ctx.input
        .get(name)
        .map(|v| {
            v.as_bool()
                .ok_or_else(|| Error::Invalid(format!("--{name} must be a boolean")))
        })
        .transpose()
        .map(|v| v.unwrap_or(false))
}
fn in_actions() -> bool {
    std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true")
}
fn options(ctx: &Context, load: &LoadOptions) -> Result<Options, Error> {
    if !flag(ctx, "github")? {
        return Err(Error::Invalid("publish currently requires --github".into()));
    }
    let repository = text(ctx, "repo")?
        .map(str::to_owned)
        .or_else(|| {
            in_actions()
                .then(|| std::env::var("GITHUB_REPOSITORY").ok())
                .flatten()
        })
        .ok_or_else(|| {
            Error::Invalid("publish requires --repo owner/repo outside GitHub Actions".into())
        })?;
    let mut config = crate::read_project(&ctx.project)?
        .map(|p| p.cli.github)
        .unwrap_or_default();
    if let Some(api) = text(ctx, "api")? {
        config.api = api.into();
    }
    if let Some(name) = text(ctx, "token-env")? {
        config.token_env = Some(name.into());
    }
    if config.token_env.is_none() {
        config.token_env = ["GH_TOKEN", "GITHUB_TOKEN"]
            .into_iter()
            .find(|name| std::env::var(name).is_ok_and(|v| !v.is_empty()))
            .map(str::to_owned);
    }
    let target = text(ctx, "target")?
        .map(str::to_owned)
        .or_else(|| {
            in_actions()
                .then(|| std::env::var("GITHUB_SHA").ok())
                .flatten()
        })
        .unwrap_or_else(|| "HEAD".into());
    if target.len() > 256 || target.contains(['\r', '\n', '\0']) {
        return Err(Error::Invalid("invalid publish target ref".into()));
    }
    let output = crate::project_path(
        &ctx.project,
        Path::new(text(ctx, "out")?.unwrap_or("dist/lenso")),
    )?;
    Ok(Options {
        repository: Repo::parse(&repository)?,
        config,
        target,
        draft: flag(ctx, "draft")?,
        dry_run: flag(ctx, "dry-run")?,
        output,
        cache: load.cache()?,
    })
}
fn get(
    github: &Github,
    repo: &Repo,
    parts: &[&str],
    missing: bool,
) -> Result<Option<Value>, Error> {
    github
        .fetch(
            github.url(repo, parts)?,
            "application/vnd.github+json",
            1024 * 1024,
            &LoadOptions::default(),
            missing,
        )?
        .map(|bytes| {
            serde_json::from_slice(&bytes)
                .map_err(|_| Error::Operation("GitHub returned invalid publish metadata".into()))
        })
        .transpose()
}
fn required(github: &Github, repo: &Repo, parts: &[&str]) -> Result<Value, Error> {
    get(github, repo, parts, false)?
        .ok_or_else(|| Error::Operation("GitHub publish metadata is missing".into()))
}
fn target_commit(github: &Github, repo: &Repo, target: &str) -> Result<String, Error> {
    let value = required(github, repo, &["commits", target])?;
    let commit = value["sha"]
        .as_str()
        .ok_or_else(|| Error::Invalid("publish target commit is missing".into()))?;
    crate::github::commit(commit)?;
    Ok(commit.into())
}
fn tag_commit(github: &Github, repo: &Repo, tag: &str) -> Result<Option<String>, Error> {
    let Some(value) = get(github, repo, &["git", "ref", "tags", tag], true)? else {
        return Ok(None);
    };
    let mut object = value["object"].clone();
    for _ in 0..8 {
        let sha = object["sha"]
            .as_str()
            .ok_or_else(|| Error::Invalid("GitHub tag object SHA is missing".into()))?;
        crate::github::commit(sha)?;
        match object["type"].as_str() {
            Some("commit") => return Ok(Some(sha.into())),
            Some("tag") => {
                object = required(github, repo, &["git", "tags", sha])?["object"].clone();
            }
            _ => {
                return Err(Error::Invalid(
                    "GitHub tag does not reference a commit".into(),
                ));
            }
        }
    }
    Err(Error::Invalid(
        "GitHub annotated tag depth limit exceeded".into(),
    ))
}
fn find_release(github: &Github, repo: &Repo, tag: &str) -> Result<Option<Release>, Error> {
    let mut found = None;
    for page in 1..=128 {
        let mut url = github.url(repo, &["releases"])?;
        url.query_pairs_mut()
            .append_pair("per_page", "30")
            .append_pair("page", &page.to_string());
        let bytes = github
            .fetch(
                url,
                "application/vnd.github+json",
                1024 * 1024,
                &LoadOptions::default(),
                false,
            )?
            .ok_or_else(|| Error::Operation("release list is missing".into()))?;
        let entries: Vec<Value> = serde_json::from_slice(&bytes)?;
        for entry in &entries {
            if entry["tag_name"] == tag {
                if found.is_some() {
                    return Err(Error::Invalid(
                        "multiple releases use this tag; resolve the ambiguity before publishing"
                            .into(),
                    ));
                }
                found = Some(serde_json::from_value(entry.clone())?);
            }
        }
        if entries.len() < 30 {
            return Ok(found);
        }
    }
    Err(Error::Invalid(
        "publish release pagination limit exceeded".into(),
    ))
}
fn verify_target(github: &Github, plan: &Prepared, release: &Release) -> Result<(), Error> {
    let version = semver::Version::parse(&plan.metadata.version)
        .map_err(|e| Error::Invalid(e.to_string()))?;
    if release.prerelease == version.pre.is_empty() {
        return Err(Error::Invalid(
            "release prerelease flag does not match the package version".into(),
        ));
    }
    let commit = match tag_commit(github, &plan.options.repository, &plan.tag)? {
        Some(commit) => commit,
        None if release.draft => {
            target_commit(github, &plan.options.repository, &release.target_commitish)?
        }
        None => return Err(Error::Permission("published release tag is missing".into())),
    };
    if commit != plan.commit {
        return Err(Error::Permission(
            "release target differs from the approved commit; rerun with the original --target"
                .into(),
        ));
    }
    Ok(())
}
fn verify_asset(
    github: &Github,
    repo: &Repo,
    remote: &RemoteAsset,
    asset: &Asset,
) -> Result<(), Error> {
    if remote.name != asset.name
        || remote.size != asset.bytes.len() as u64
        || remote.state != "uploaded"
    {
        return Err(Error::Permission(format!(
            "release asset {} conflicts with the prepared package; no asset was replaced",
            asset.name
        )));
    }
    let expected = format!("sha256:{}", asset.sha256);
    let matches = match &remote.digest {
        Some(value) => value == &expected,
        None => {
            digest(&github.asset(repo, remote.id, asset.bytes.len(), &LoadOptions::default())?)
                == asset.sha256
        }
    };
    if !matches {
        return Err(Error::Permission(format!(
            "release asset {} has a different digest; no asset was replaced",
            asset.name
        )));
    }
    Ok(())
}
fn verify_existing(
    github: &Github,
    repo: &Repo,
    release: &Release,
    assets: &[Asset],
) -> Result<(), Error> {
    let expected: BTreeSet<_> = assets.iter().map(|a| a.name.as_str()).collect();
    let mut seen = BTreeSet::new();
    for remote in &release.assets {
        if !seen.insert(remote.name.as_str()) || !expected.contains(remote.name.as_str()) {
            return Err(Error::Invalid(
                "release has duplicate or unexpected assets; publish uses one package per release"
                    .into(),
            ));
        }
        let asset = assets
            .iter()
            .find(|a| a.name == remote.name)
            .ok_or_else(|| Error::Invalid("unexpected release asset".into()))?;
        verify_asset(github, repo, remote, asset)?;
    }
    if !release.draft && release.assets.len() != assets.len() {
        return Err(Error::Permission(
            "a published release is missing assets; published releases are never modified".into(),
        ));
    }
    Ok(())
}
fn upload_url(
    config: &GithubConfig,
    repo: &Repo,
    release: &Release,
    name: &str,
) -> Result<reqwest::Url, Error> {
    let raw = release.upload_url.split('{').next().unwrap_or("");
    let mut url =
        reqwest::Url::parse(raw).map_err(|_| Error::Invalid("invalid GitHub upload URL".into()))?;
    let api = reqwest::Url::parse(&config.api)
        .map_err(|_| Error::Invalid("invalid GitHub API URL".into()))?;
    let public_upload = api.host_str() == Some("api.github.com")
        && url.scheme() == "https"
        && url.host_str() == Some("uploads.github.com")
        && url.port_or_known_default() == Some(443);
    let suffix = format!(
        "/repos/{}/{}/releases/{}/assets",
        repo.owner, repo.name, release.id
    );
    if (!public_upload && url.origin() != api.origin())
        || !url.path().ends_with(&suffix)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(Error::Permission(
            "GitHub upload URL is outside the approved repository/release".into(),
        ));
    }
    url.query_pairs_mut().append_pair("name", name);
    Ok(url)
}
fn prepare(ctx: &Context, options: Options) -> Result<Prepared, Error> {
    ctx.events
        .preparing("Packaging and validating release assets");
    let stage = tempfile::tempdir()?;
    let packaged = crate::package_extension(&ctx.project, text(ctx, "id")?, stage.path())?;
    let metadata: ReleaseMetadata = serde_json::from_slice(&read_bounded(
        &stage.path().join("lenso-release.json"),
        1024 * 1024,
    )?)?;
    let tag = packaged["tag"]
        .as_str()
        .ok_or_else(|| Error::Invalid("package tag is missing".into()))?
        .to_owned();
    if tag.contains(['\r', '\n']) {
        return Err(Error::Invalid(
            "release tag cannot contain line breaks".into(),
        ));
    }
    if in_actions()
        && std::env::var("GITHUB_REF_TYPE").as_deref() == Ok("tag")
        && std::env::var("GITHUB_REF_NAME").ok().as_deref() != Some(tag.as_str())
    {
        return Err(Error::Invalid(
            "Actions tag does not match the package manifest version/tag convention".into(),
        ));
    }
    let mut assets = Vec::new();
    for name in [
        &metadata.archive.name,
        &metadata.manifest.name,
        &"lenso-release.json".to_owned(),
    ] {
        let bytes = read_bounded(&stage.path().join(name), 32 * 1024 * 1024)?;
        let sha256 = digest(&bytes);
        assets.push(Asset {
            name: name.clone(),
            bytes,
            sha256,
        });
    }
    if assets[0].sha256 != metadata.archive.sha256 || assets[1].sha256 != metadata.manifest.sha256 {
        return Err(Error::Permission("prepared package digest mismatch".into()));
    }
    let archive = archive_files(&assets[0].bytes)?;
    if archive.get(&metadata.manifest.name) != Some(&assets[1].bytes) {
        return Err(Error::Permission(
            "embedded package manifest does not match the release manifest".into(),
        ));
    }
    std::fs::create_dir_all(&options.output)?;
    for asset in &assets {
        atomic_write(&options.output.join(&asset.name), &asset.bytes)?;
    }
    let mut plan = Prepared {
        options,
        metadata,
        tag,
        assets,
        commit: String::new(),
        existing: None,
    };
    validate_output_file()?;
    if plan.options.dry_run {
        return Ok(plan);
    }
    let github = Github::new(&plan.options.config)?;
    if !github.authenticated() {
        return Err(Error::Permission(
            "publishing requires --token-env NAME, GH_TOKEN or GITHUB_TOKEN".into(),
        ));
    }
    ctx.events
        .preparing("Checking GitHub target, author configuration and existing release");
    plan.commit = target_commit(&github, &plan.options.repository, &plan.options.target)?;
    if tag_commit(&github, &plan.options.repository, &plan.tag)?
        .is_some_and(|commit| commit != plan.commit)
    {
        return Err(Error::Permission(
            "release tag points to a different commit; tags are never moved".into(),
        ));
    }
    let discovery = target_commit(&github, &plan.options.repository, "HEAD")?;
    let descriptor = github.file(
        &plan.options.repository,
        &discovery,
        "lenso.registry.toml",
        &plan.options.cache,
        &LoadOptions::default(),
    )?;
    let (expected_tag, expected_archive) = publication_conventions(
        descriptor.as_deref(),
        &plan.metadata.id,
        &plan.metadata.version,
    )?;
    if plan.tag != expected_tag || plan.metadata.archive.name != expected_archive {
        return Err(Error::Invalid("package conventions are not discoverable from the repository default branch; commit lenso.registry.toml before publishing".into()));
    }
    plan.existing = find_release(&github, &plan.options.repository, &plan.tag)?;
    if let Some(release) = &plan.existing {
        verify_target(&github, &plan, release)?;
        if release.immutable && release.draft {
            return Err(Error::Permission(
                "an immutable release cannot be resumed as a draft".into(),
            ));
        }
        if !release.draft && plan.options.draft {
            return Err(Error::Permission(
                "an existing published release is never converted back to draft".into(),
            ));
        }
        verify_existing(&github, &plan.options.repository, release, &plan.assets)?;
    }
    Ok(plan)
}
impl Prepared {
    fn intent(&self) -> Value {
        json!({"repository":self.options.repository.name(),"api":self.options.config.api,"package_id":self.metadata.id,"version":self.metadata.version,"tag":self.tag,"target":self.options.target,"commit":self.commit,"draft":self.options.draft,
            "assets":self.assets.iter().map(|a|json!({"name":a.name,"size":a.bytes.len(),"sha256":a.sha256})).collect::<Vec<_>>(),"existing":self.existing.as_ref().map(|r|json!({"id":r.id,"target":r.target_commitish,"draft":r.draft,"immutable":r.immutable,"assets":r.assets}))})
    }
    fn summary(&self) -> String {
        let action = if self.options.draft {
            "prepare a draft"
        } else {
            "publish a release"
        };
        let mut text = format!(
            "GitHub: {}\nAPI: {}\nPackage: {} {}\nTag: {}\nCommit: {}\nArtifacts: {}\n",
            self.options.repository.name(),
            self.options.config.api,
            self.metadata.id,
            self.metadata.version,
            self.tag,
            self.commit,
            self.options.output.display()
        );
        for asset in &self.assets {
            text.push_str(&format!(
                "  {} · {} bytes · sha256 {}\n",
                asset.name,
                asset.bytes.len(),
                asset.sha256
            ));
        }
        text.push_str(&format!("Approve uploading verified assets to {action}?"));
        text
    }
}
fn final_release(github: &Github, plan: &Prepared, id: u64) -> Result<Release, Error> {
    let release: Release = serde_json::from_value(required(
        github,
        &plan.options.repository,
        &["releases", &id.to_string()],
    )?)?;
    if release.id != id || release.tag_name != plan.tag {
        return Err(Error::Operation(
            "GitHub release identity changed during publishing".into(),
        ));
    }
    verify_target(github, plan, &release)?;
    verify_existing(github, &plan.options.repository, &release, &plan.assets)?;
    Ok(release)
}
fn execute(ctx: &Context, plan: &Prepared) -> Result<Value, Error> {
    let _guard = plan.options.cache.lock(&format!(
        "publish:{}:{}:{}",
        plan.options.config.api,
        plan.options.repository.name(),
        plan.tag
    ))?;
    let github = Github::new(&plan.options.config)?;
    let current = find_release(&github, &plan.options.repository, &plan.tag)?;
    if current.as_ref().map(|r| (r.id, r.draft)) != plan.existing.as_ref().map(|r| (r.id, r.draft))
    {
        return Err(Error::Operation(
            "release state changed after approval; rerun publish to review it".into(),
        ));
    }
    if tag_commit(&github, &plan.options.repository, &plan.tag)?
        .is_some_and(|commit| commit != plan.commit)
    {
        return Err(Error::Permission(
            "tag commit changed after approval".into(),
        ));
    }
    let release = if let Some(release) = current {
        verify_target(&github, plan, &release)?;
        verify_existing(&github, &plan.options.repository, &release, &plan.assets)?;
        release
    } else {
        ctx.events.preparing("Creating GitHub draft release");
        let body = json!({"tag_name":plan.tag,"target_commitish":plan.commit,"name":format!("{} {}",plan.metadata.id,plan.metadata.version),"body":format!("Prebuilt Lenso extension {} {}.\n\nArchive SHA-256: `{}`",plan.metadata.id,plan.metadata.version,plan.metadata.archive.sha256),"draft":true,"prerelease":!semver::Version::parse(&plan.metadata.version).map_err(|e|Error::Invalid(e.to_string()))?.pre.is_empty()});
        let release: Release = serde_json::from_value(github.mutate(
            reqwest::Method::POST,
            github.url(&plan.options.repository, &["releases"])?,
            serde_json::to_vec(&body)?,
            "application/json",
        )?)?;
        if !release.draft || release.tag_name != plan.tag {
            return Err(Error::Operation(
                "GitHub did not create the requested draft release".into(),
            ));
        }
        release
    };
    let mut uploaded = 0;
    let operation = (|| {
        for asset in &plan.assets {
            if release.assets.iter().any(|a| a.name == asset.name) {
                continue;
            }
            if !release.draft {
                return Err(Error::Permission(
                    "published releases are never modified".into(),
                ));
            }
            ctx.events.preparing(format!("Uploading {}", asset.name));
            let url = upload_url(
                &plan.options.config,
                &plan.options.repository,
                &release,
                &asset.name,
            )?;
            let remote: RemoteAsset = serde_json::from_value(github.mutate(
                reqwest::Method::POST,
                url,
                asset.bytes.clone(),
                "application/octet-stream",
            )?)?;
            let verifier = Github::new(&plan.options.config)?;
            verify_asset(&verifier, &plan.options.repository, &remote, asset)?;
            uploaded += 1;
        }
        let verifier = Github::new(&plan.options.config)?;
        let mut ready = final_release(&verifier, plan, release.id)?;
        if ready.assets.len() != plan.assets.len() {
            return Err(Error::Operation(
                "release assets are incomplete; draft was not published".into(),
            ));
        }
        if ready.draft && !plan.options.draft {
            ctx.events
                .preparing("Publishing the verified GitHub release");
            if tag_commit(&verifier, &plan.options.repository, &plan.tag)?
                .is_some_and(|commit| commit != plan.commit)
            {
                return Err(Error::Permission(
                    "tag commit changed before release publication".into(),
                ));
            }
            ready = serde_json::from_value(verifier.mutate(
                reqwest::Method::PATCH,
                verifier.url(
                    &plan.options.repository,
                    &["releases", &ready.id.to_string()],
                )?,
                serde_json::to_vec(
                    &json!({"draft":false,"target_commitish":plan.commit,"make_latest":"false"}),
                )?,
                "application/json",
            )?)?;
            if ready.draft || ready.id != release.id || ready.tag_name != plan.tag {
                return Err(Error::Operation(
                    "GitHub did not confirm release publication".into(),
                ));
            }
        }
        let verifier = Github::new(&plan.options.config)?;
        verify_target(&verifier, plan, &ready)?;
        verify_existing(&verifier, &plan.options.repository, &ready, &plan.assets)?;
        if ready.assets.len() != plan.assets.len() {
            return Err(Error::Operation(
                "final release asset set is incomplete".into(),
            ));
        }
        let mut output = plan.intent();
        let object = output
            .as_object_mut()
            .ok_or_else(|| Error::Invalid("invalid publish output".into()))?;
        object.remove("existing");
        object.insert("release_id".into(), json!(ready.id));
        object.insert("release_url".into(), json!(ready.html_url));
        object.insert(
            "state".into(),
            json!(if ready.draft { "draft" } else { "published" }),
        );
        object.insert("uploaded".into(), json!(uploaded));
        object.insert(
            "message".into(),
            json!(format!(
                "{} {} {}: {}",
                if ready.draft {
                    "Prepared draft for"
                } else {
                    "Published"
                },
                plan.metadata.id,
                plan.metadata.version,
                ready.html_url
            )),
        );
        Ok(output)
    })();
    operation.map_err(|error|Error::Operation(format!("GitHub release {} for {} / {} is preserved; verified uploads in this attempt: {uploaded}. Rerun publish to inspect/resume it. {error}",release.id,plan.options.repository.name(),plan.tag)))
}
fn output_file() -> Option<PathBuf> {
    in_actions()
        .then(|| std::env::var_os("GITHUB_OUTPUT").map(PathBuf::from))
        .flatten()
}
fn validate_output_file() -> Result<(), Error> {
    if let Some(path) = output_file() {
        let metadata = std::fs::symlink_metadata(&path)?;
        if !path.is_absolute() || !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(Error::Permission(
                "GITHUB_OUTPUT must be an existing absolute regular file".into(),
            ));
        }
    }
    Ok(())
}
fn actions_output(output: &Value) -> Result<(), Error> {
    let Some(path) = output_file() else {
        return Ok(());
    };
    validate_output_file()?;
    let values = BTreeMap::from([
        ("release_id", output["release_id"].to_string()),
        (
            "release_url",
            output["release_url"].as_str().unwrap_or_default().into(),
        ),
        ("tag", output["tag"].as_str().unwrap_or_default().into()),
        (
            "package_id",
            output["package_id"].as_str().unwrap_or_default().into(),
        ),
        (
            "version",
            output["version"].as_str().unwrap_or_default().into(),
        ),
        ("state", output["state"].as_str().unwrap_or_default().into()),
    ]);
    let mut body = String::new();
    for (name, value) in values {
        if value.contains(['\r', '\n']) {
            return Err(Error::Invalid(
                "Actions output cannot contain line breaks".into(),
            ));
        }
        body.push_str(&format!("{name}={value}\n"));
    }
    let mut file = std::fs::OpenOptions::new().append(true).open(path)?;
    file.write_all(body.as_bytes())?;
    Ok(())
}

pub(crate) async fn run(ctx: Context, load: LoadOptions) -> Result<Outcome, Error> {
    let options = options(&ctx, &load)?;
    if load.offline && !options.dry_run {
        return Err(Error::Invalid(
            "publishing requires network access; use --dry-run with --offline".into(),
        ));
    }
    let preparing = ctx.clone();
    let plan = tokio::task::spawn_blocking(move || prepare(&preparing, options))
        .await
        .map_err(|e| Error::Operation(e.to_string()))??;
    if plan.options.dry_run {
        let mut intent = plan.intent();
        intent["state"] = json!("prepared");
        intent["message"] = json!(format!(
            "Prepared {} {} for {} / {} in {} (no GitHub requests)",
            plan.metadata.id,
            plan.metadata.version,
            plan.options.repository.name(),
            plan.tag,
            plan.options.output.display()
        ));
        return Ok(Outcome::complete(intent));
    }
    let no_change = plan.existing.as_ref().is_some_and(|release| {
        release.assets.len() == plan.assets.len() && (!release.draft || plan.options.draft)
    });
    if !no_change {
        let id = format!("publish:{}", digest(plan.intent().to_string().as_bytes()));
        match ctx.answers.get(&id) {
            None => {
                return Ok(Outcome::NeedsInput {
                    question: Question {
                        id,
                        message: plan.summary(),
                        kind: QuestionKind::Confirm,
                    },
                });
            }
            Some(Value::Bool(false)) => return Err(Error::Cancelled),
            Some(Value::Bool(true)) => {}
            _ => return Err(Error::Invalid("publish approval must be a boolean".into())),
        }
    }
    let output = tokio::task::spawn_blocking(move || execute(&ctx, &plan))
        .await
        .map_err(|e| Error::Operation(e.to_string()))??;
    actions_output(&output).map_err(|e| {
        Error::Operation(format!(
            "release {} is {}; GitHub Actions output failed: {e}",
            output["release_id"], output["state"]
        ))
    })?;
    Ok(Outcome::complete(output))
}
