use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use lenso_cli::Error;
use reqwest::Url;

use crate::project::read_bounded;
use crate::{Cache, LoadOptions, project_path};

#[derive(Debug)]
pub(crate) enum Source {
    Local(PathBuf),
    Remote(Url),
}

impl Source {
    pub fn parse(project: &Path, source: &str) -> Result<Self, Error> {
        if source.contains("://") {
            let url =
                Url::parse(source).map_err(|e| Error::Invalid(format!("dependency URL: {e}")))?;
            let loopback = url
                .host_str()
                .and_then(|host| {
                    host.trim_matches(['[', ']'])
                        .parse::<std::net::IpAddr>()
                        .ok()
                })
                .is_some_and(|ip| ip.is_loopback());
            if (url.scheme() != "https" && !(url.scheme() == "http" && loopback))
                || !url.username().is_empty()
                || url.password().is_some()
                || url.fragment().is_some()
            {
                return Err(Error::Invalid("dependency sources require HTTPS without credentials or fragments (HTTP is allowed on loopback only)".into()));
            }
            Ok(Self::Remote(url))
        } else {
            let relative = Path::new(source);
            if relative
                .components()
                .next()
                .is_some_and(|part| part.as_os_str() == ".lenso")
            {
                return Err(Error::Invalid(
                    "package sources must live outside the disposable .lenso directory".into(),
                ));
            }
            Ok(Self::Local(project_path(project, relative)?))
        }
    }

    pub fn module(&self, project: &Path, declaration: &str, module: &str) -> Result<String, Error> {
        if module.is_empty()
            || module.contains([':', '\\'])
            || Path::new(module)
                .components()
                .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            return Err(Error::Invalid(
                "module paths must be relative without traversal".into(),
            ));
        }
        match self {
            Self::Local(_) => Ok(Path::new(declaration)
                .parent()
                .unwrap_or(Path::new(""))
                .join(module)
                .to_string_lossy()
                .into_owned()),
            Self::Remote(base) => {
                let url = base
                    .join(module)
                    .map_err(|e| Error::Invalid(format!("module URL: {e}")))?;
                let directory = base.join("./").map_err(|e| Error::Invalid(e.to_string()))?;
                if url.origin() != base.origin() || !url.path().starts_with(directory.path()) {
                    return Err(Error::Invalid(
                        "module URL escapes its source directory".into(),
                    ));
                }
                Self::parse(project, url.as_str())?;
                Ok(url.into())
            }
        }
    }

    pub fn is_toml(&self) -> bool {
        match self {
            Self::Local(path) => path.extension().is_some_and(|ext| ext == "toml"),
            Self::Remote(url) => url.path().ends_with(".toml"),
        }
    }

    pub fn manifest<T>(
        &self,
        cache: &Cache,
        options: &LoadOptions,
        parse: impl FnOnce(&[u8]) -> Result<T, Error>,
    ) -> Result<T, Error> {
        match self {
            Self::Local(path) => parse(&read_bounded(path, 1024 * 1024)?),
            Self::Remote(url) => {
                let _guard = cache.lock(url.as_str())?;
                if (options.mode != crate::LoadMode::Update || options.offline)
                    && let Some(bytes) = cache.metadata(url.as_str())?
                {
                    return parse(&bytes);
                }
                let bytes = self.bytes(1024 * 1024, options)?;
                let manifest = parse(&bytes)?;
                cache.save_metadata(url.as_str(), &bytes)?;
                Ok(manifest)
            }
        }
    }

    pub fn bytes(&self, max: usize, options: &LoadOptions) -> Result<Vec<u8>, Error> {
        match self {
            Self::Local(path) => read_bounded(path, max),
            Self::Remote(url) => {
                if options.offline {
                    return Err(Error::Operation(format!(
                        "dependency unavailable offline: {url}"
                    )));
                }
                options.events.preparing(format!("Downloading {url}"));
                let mut builder = reqwest::blocking::Client::builder();
                if url
                    .host_str()
                    .and_then(|host| {
                        host.trim_matches(['[', ']'])
                            .parse::<std::net::IpAddr>()
                            .ok()
                    })
                    .is_some_and(|ip| ip.is_loopback())
                {
                    builder = builder.no_proxy();
                }
                let client = builder
                    .timeout(Duration::from_secs(20))
                    .redirect(reqwest::redirect::Policy::none())
                    .build()
                    .map_err(|e| Error::Operation(format!("dependency HTTP client: {e}")))?;
                let response = client
                    .get(url.clone())
                    .send()
                    .map_err(|e| Error::Operation(format!("dependency GET {url}: {e}")))?;
                if !response.status().is_success() {
                    return Err(Error::Operation(format!(
                        "dependency GET {url}: HTTP {}",
                        response.status()
                    )));
                }
                let mut bytes = Vec::new();
                response.take(max as u64 + 1).read_to_end(&mut bytes)?;
                if bytes.len() > max {
                    return Err(Error::Invalid(format!(
                        "dependency exceeds size limit: {url}"
                    )));
                }
                Ok(bytes)
            }
        }
    }
}
