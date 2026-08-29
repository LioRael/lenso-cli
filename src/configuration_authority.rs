use std::{
    error::Error,
    fmt,
    fmt::Write as _,
    path::{Path, PathBuf},
    str,
    str::FromStr,
    sync::{Arc, Mutex},
};

use anyhow::{Context, bail};
use lenso_app_plan::authoring::{
    HostCatalog, PluginInstanceId, PluginRootInstance, PluginRootResolutionError,
    PluginRootSnapshot, ResolvedApp, resolve_plugin_root,
};
use sha2::{Digest, Sha256};

use super::{
    MAX_CONFIGURATION_BYTES, PLUGIN_ROOT, PluginRootAuthoringState, atomic_write,
    inspect_plugin_root, load_host_catalog, lock_plugin_root, snapshot_plugin_root,
    validate_existing_plugin_id, validate_instance_filename,
};

const PROPOSAL_SCHEMA: &str = "lenso.plugin-configuration-proposal.v1";
const PUBLICATION_SCHEMA: &str = "lenso.plugin-configuration-publication.v1";

/// Stable provenance for the authority that owns Plugin configuration publication.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PluginConfigurationAuthoritySource {
    kind: String,
    reference: String,
}

impl PluginConfigurationAuthoritySource {
    /// Creates one Host-trusted authority identity.
    pub fn new(kind: impl Into<String>, reference: impl Into<String>) -> anyhow::Result<Self> {
        let kind = kind.into();
        let reference = reference.into();
        if kind.is_empty()
            || kind.len() > 64
            || !kind.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'_' | b'-' | b'.')
            })
        {
            bail!("Plugin configuration authority kind is invalid");
        }
        if reference.is_empty() || reference.len() > 256 || reference.chars().any(char::is_control)
        {
            bail!("Plugin configuration authority reference is invalid");
        }
        Ok(Self { kind, reference })
    }

    pub fn kind(&self) -> &str {
        &self.kind
    }

    pub fn reference(&self) -> &str {
        &self.reference
    }
}

/// Host-side port for inspecting, proposing, and publishing Plugin configuration.
///
/// Implementations own authoring storage and compare-and-swap publication. They
/// do not own App Generation staging, routing, or Kernel execution.
pub trait PluginConfigurationAuthority: fmt::Debug + Send + Sync {
    fn source(&self) -> PluginConfigurationAuthoritySource;

    fn inspect(&self) -> anyhow::Result<PluginRootAuthoringState>;

    fn propose(
        &self,
        expected_revision: &PluginRootRevision,
        plugin_id: &str,
        instance: &str,
        bytes: &[u8],
    ) -> anyhow::Result<PluginConfigurationProposal>;

    fn publish(
        &self,
        proposal: &PluginConfigurationProposal,
    ) -> anyhow::Result<PluginConfigurationPublication>;
}

/// Default configuration authority backed by one visible App Plugin Root.
#[derive(Clone, Debug)]
pub struct LocalPluginRootAuthority {
    root: PathBuf,
    access: Arc<Mutex<()>>,
}

impl LocalPluginRootAuthority {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            access: Arc::new(Mutex::new(())),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn lock(&self) -> anyhow::Result<std::sync::MutexGuard<'_, ()>> {
        self.access
            .lock()
            .map_err(|_| anyhow::anyhow!("Plugin configuration authority lock is poisoned"))
    }
}

impl PluginConfigurationAuthority for LocalPluginRootAuthority {
    fn source(&self) -> PluginConfigurationAuthoritySource {
        PluginConfigurationAuthoritySource {
            kind: "local_plugin_root".to_owned(),
            reference: "app".to_owned(),
        }
    }

    fn inspect(&self) -> anyhow::Result<PluginRootAuthoringState> {
        let _guard = self.lock()?;
        inspect_plugin_root(&self.root)
    }

    fn propose(
        &self,
        expected_revision: &PluginRootRevision,
        plugin_id: &str,
        instance: &str,
        bytes: &[u8],
    ) -> anyhow::Result<PluginConfigurationProposal> {
        let _guard = self.lock()?;
        propose_instance_configuration(&self.root, expected_revision, plugin_id, instance, bytes)
    }

    fn publish(
        &self,
        proposal: &PluginConfigurationProposal,
    ) -> anyhow::Result<PluginConfigurationPublication> {
        let _guard = self.lock()?;
        publish_instance_configuration(&self.root, proposal)
    }
}

/// A deterministic semantic revision of one App's Plugin Root.
///
/// Formatting and comments in TOML do not change this revision. The parsed
/// desired state is the authority used for compare-and-swap publication.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct PluginRootRevision(String);

impl PluginRootRevision {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PluginRootRevision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for PluginRootRevision {
    type Err = PluginRootRevisionParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let Some(digest) = value.strip_prefix("sha256:") else {
            return Err(PluginRootRevisionParseError);
        };
        if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(PluginRootRevisionParseError);
        }
        Ok(Self(format!("sha256:{}", digest.to_ascii_lowercase())))
    }
}

/// Text did not contain one complete SHA-256 Plugin Root revision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PluginRootRevisionParseError;

impl fmt::Display for PluginRootRevisionParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Plugin Root revision must be `sha256:` followed by 64 hex digits")
    }
}

impl Error for PluginRootRevisionParseError {}

/// A proposal was published against a Plugin Root that has since changed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PluginRootRevisionConflict {
    expected: PluginRootRevision,
    current: PluginRootRevision,
}

impl PluginRootRevisionConflict {
    pub const fn expected(&self) -> &PluginRootRevision {
        &self.expected
    }

    pub const fn current(&self) -> &PluginRootRevision {
        &self.current
    }
}

impl fmt::Display for PluginRootRevisionConflict {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Plugin Root revision conflict: expected {}, current {}",
            self.expected, self.current
        )
    }
}

impl Error for PluginRootRevisionConflict {}

/// Read-only review evidence for one exact Plugin Instance configuration change.
#[derive(Clone, Debug)]
pub struct PluginConfigurationProposal {
    schema: &'static str,
    base_revision: PluginRootRevision,
    candidate_revision: PluginRootRevision,
    digest: String,
    status: PluginConfigurationProposalStatus,
    application: PluginConfigurationApplication,
    diagnostics: Vec<PluginConfigurationDiagnostic>,
    plugin_id: String,
    instance_key: String,
    toml: Vec<u8>,
}

impl PluginConfigurationProposal {
    pub const fn schema(&self) -> &str {
        self.schema
    }

    pub const fn base_revision(&self) -> &PluginRootRevision {
        &self.base_revision
    }

    pub const fn candidate_revision(&self) -> &PluginRootRevision {
        &self.candidate_revision
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }

    pub const fn status(&self) -> PluginConfigurationProposalStatus {
        self.status
    }

    pub const fn application(&self) -> PluginConfigurationApplication {
        self.application
    }

    pub fn diagnostics(&self) -> &[PluginConfigurationDiagnostic] {
        &self.diagnostics
    }

    pub fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    pub fn instance_key(&self) -> &str {
        &self.instance_key
    }
}

/// Whether a configuration proposal can proceed to publication.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PluginConfigurationProposalStatus {
    Ready,
    NeedsDecision,
    Rejected,
}

/// The Host action required after publication.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PluginConfigurationApplication {
    Noop,
    AppGeneration,
    Blocked,
}

/// One stable reason a configuration proposal cannot proceed automatically.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PluginConfigurationDiagnostic {
    code: &'static str,
    detail: String,
}

impl PluginConfigurationDiagnostic {
    pub const fn code(&self) -> &str {
        self.code
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }
}

/// Evidence returned after compare-and-swap publication succeeds.
#[derive(Clone, Debug)]
pub struct PluginConfigurationPublication {
    schema: &'static str,
    base_revision: PluginRootRevision,
    revision: PluginRootRevision,
    proposal_digest: String,
    resolved: ResolvedApp,
}

impl PluginConfigurationPublication {
    pub const fn schema(&self) -> &str {
        self.schema
    }

    pub const fn base_revision(&self) -> &PluginRootRevision {
        &self.base_revision
    }

    pub const fn revision(&self) -> &PluginRootRevision {
        &self.revision
    }

    pub fn proposal_digest(&self) -> &str {
        &self.proposal_digest
    }

    pub const fn resolved(&self) -> &ResolvedApp {
        &self.resolved
    }

    pub fn into_resolved(self) -> ResolvedApp {
        self.resolved
    }
}

/// Builds review evidence without changing the Plugin Root.
pub fn propose_instance_configuration(
    root: &Path,
    expected_revision: &PluginRootRevision,
    plugin_id: &str,
    instance: &str,
    bytes: &[u8],
) -> anyhow::Result<PluginConfigurationProposal> {
    validate_existing_plugin_id(plugin_id)?;
    validate_instance_filename(instance)?;
    let host = load_host_catalog(root)?;
    let current = snapshot_plugin_root(root)?;
    let current_revision = revision_for_snapshot(&current)?;
    ensure_revision(expected_revision, &current_revision)?;
    build_proposal(
        &host,
        &current,
        current_revision,
        plugin_id,
        instance,
        bytes,
    )
}

/// Publishes an exact reviewed proposal when its base revision is still current.
pub fn publish_instance_configuration(
    root: &Path,
    proposal: &PluginConfigurationProposal,
) -> anyhow::Result<PluginConfigurationPublication> {
    let _lock = lock_plugin_root(root)?;
    let host = load_host_catalog(root)?;
    let current = snapshot_plugin_root(root)?;
    let current_revision = revision_for_snapshot(&current)?;
    ensure_revision(&proposal.base_revision, &current_revision)?;

    let verified = build_proposal(
        &host,
        &current,
        current_revision,
        &proposal.plugin_id,
        &proposal.instance_key,
        &proposal.toml,
    )?;
    if proposal.candidate_revision != verified.candidate_revision
        || proposal.digest != verified.digest
    {
        bail!("Plugin configuration proposal no longer matches its reviewed candidate");
    }
    if verified.status != PluginConfigurationProposalStatus::Ready
        || verified.application == PluginConfigurationApplication::Blocked
    {
        let detail = verified
            .diagnostics
            .as_slice()
            .first()
            .map_or("candidate did not pass the Ready Gate", |diagnostic| {
                diagnostic.detail()
            });
        bail!("Plugin configuration proposal cannot be published: {detail}");
    }

    let path = root
        .join(PLUGIN_ROOT)
        .join(&proposal.plugin_id)
        .join(format!("{}.toml", proposal.instance_key));
    atomic_write(&path, &proposal.toml)?;
    let published = snapshot_plugin_root(root)?;
    let revision = revision_for_snapshot(&published)?;
    if revision != proposal.candidate_revision {
        bail!("published Plugin Root does not match the reviewed candidate revision");
    }
    let resolved = super::inspect_plugin_root(root)?.resolved().clone();
    Ok(PluginConfigurationPublication {
        schema: PUBLICATION_SCHEMA,
        base_revision: proposal.base_revision.clone(),
        revision,
        proposal_digest: proposal.digest.clone(),
        resolved,
    })
}

fn build_proposal(
    host: &HostCatalog,
    current: &PluginRootSnapshot,
    base_revision: PluginRootRevision,
    plugin_id: &str,
    instance: &str,
    bytes: &[u8],
) -> anyhow::Result<PluginConfigurationProposal> {
    let configuration = parse_configuration(bytes)?;
    let id = PluginInstanceId::new(plugin_id, instance);
    let mut instances = current
        .instances()
        .iter()
        .filter(|item| item.id() != &id)
        .cloned()
        .collect::<Vec<_>>();
    instances.push(PluginRootInstance::new(plugin_id, instance).with_configuration(configuration));
    let candidate = PluginRootSnapshot::new(
        current.releases().iter().cloned(),
        instances,
        current.disabled().iter().cloned(),
    );
    let candidate_revision = revision_for_snapshot(&candidate)?;
    let authority = serde_json::to_vec(&(host, current, &candidate))
        .context("encode Plugin configuration proposal authority")?;
    let digest = sha256_digest(&authority);
    let (status, application, diagnostics) = match resolve_plugin_root(host, &candidate) {
        Ok(_) => (
            PluginConfigurationProposalStatus::Ready,
            if candidate_revision == base_revision {
                PluginConfigurationApplication::Noop
            } else {
                PluginConfigurationApplication::AppGeneration
            },
            Vec::new(),
        ),
        Err(error) => {
            let status = if matches!(
                error,
                PluginRootResolutionError::AmbiguousSlot { .. }
                    | PluginRootResolutionError::AmbiguousCapability { .. }
            ) {
                PluginConfigurationProposalStatus::NeedsDecision
            } else {
                PluginConfigurationProposalStatus::Rejected
            };
            (
                status,
                PluginConfigurationApplication::Blocked,
                vec![PluginConfigurationDiagnostic {
                    code: resolution_error_code(&error),
                    detail: error.to_string(),
                }],
            )
        }
    };
    Ok(PluginConfigurationProposal {
        schema: PROPOSAL_SCHEMA,
        base_revision,
        candidate_revision,
        digest,
        status,
        application,
        diagnostics,
        plugin_id: plugin_id.to_owned(),
        instance_key: instance.to_owned(),
        toml: bytes.to_vec(),
    })
}

fn resolution_error_code(error: &PluginRootResolutionError) -> &'static str {
    match error {
        PluginRootResolutionError::AmbiguousSlot { .. } => "ambiguous_slot",
        PluginRootResolutionError::AmbiguousCapability { .. } => "ambiguous_capability",
        PluginRootResolutionError::InvalidConfiguration { .. } => "invalid_configuration",
        PluginRootResolutionError::MissingRequiredSlot(_) => "missing_required_slot",
        PluginRootResolutionError::MissingCapability { .. } => "missing_capability",
        PluginRootResolutionError::RequiredInstanceDisabled(_) => "required_instance_disabled",
        PluginRootResolutionError::UnknownPlugin(_) => "unknown_plugin",
        PluginRootResolutionError::UnknownDisabledInstance(_) => "unknown_disabled_instance",
        _ => "invalid_plugin_root",
    }
}

fn parse_configuration(bytes: &[u8]) -> anyhow::Result<serde_json::Value> {
    let byte_count = u64::try_from(bytes.len()).context("Plugin configuration is too large")?;
    if byte_count > MAX_CONFIGURATION_BYTES {
        bail!("Plugin configuration exceeds 256 KiB");
    }
    let text = str::from_utf8(bytes).context("Plugin configuration must be UTF-8 TOML")?;
    let table: toml::Table = toml::from_str(text).context("parse Plugin configuration TOML")?;
    serde_json::to_value(table).context("convert Plugin configuration to portable values")
}

fn ensure_revision(
    expected: &PluginRootRevision,
    current: &PluginRootRevision,
) -> anyhow::Result<()> {
    if expected == current {
        return Ok(());
    }
    Err(PluginRootRevisionConflict {
        expected: expected.clone(),
        current: current.clone(),
    }
    .into())
}

pub(crate) fn revision_for_snapshot(
    snapshot: &PluginRootSnapshot,
) -> anyhow::Result<PluginRootRevision> {
    let canonical =
        serde_json::to_vec(snapshot).context("encode Plugin Root revision authority")?;
    Ok(PluginRootRevision(sha256_digest(&canonical)))
}

fn sha256_digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(7 + digest.len() * 2);
    encoded.push_str("sha256:");
    for byte in digest {
        write!(&mut encoded, "{byte:02x}").expect("writing to a String cannot fail");
    }
    encoded
}

#[cfg(test)]
mod tests {
    use std::{fs, sync::Arc};

    use lenso_app_plan::authoring::{
        HostDefaultPlugin, HostPluginRelease, HostSlot, PluginDescriptor,
    };

    use super::*;
    use crate::{HOST_CATALOG, inspect_plugin_root};

    fn fixture_root() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join(".lenso")).unwrap();
        let descriptor = PluginDescriptor::new("example.agent", "1.0.0", "agent")
            .with_configuration_schema(serde_json::json!({
                "type": "object",
                "properties": {
                    "greeting": { "type": "string" }
                },
                "additionalProperties": false
            }));
        let host = HostCatalog::new(
            [HostSlot::one("agent")],
            [HostPluginRelease::new(descriptor)],
            [HostDefaultPlugin::new("example.agent", "default")],
        );
        fs::write(
            root.path().join(HOST_CATALOG),
            serde_json::to_vec(&host).unwrap(),
        )
        .unwrap();
        root
    }

    #[test]
    fn proposal_is_read_only_and_publication_advances_the_revision() {
        let root = fixture_root();
        let base = inspect_plugin_root(root.path()).unwrap().revision().clone();
        let proposal = propose_instance_configuration(
            root.path(),
            &base,
            "example.agent",
            "default",
            b"greeting = \"hello\"\n",
        )
        .unwrap();

        assert_eq!(proposal.status(), PluginConfigurationProposalStatus::Ready);
        assert_eq!(
            proposal.application(),
            PluginConfigurationApplication::AppGeneration
        );
        assert_eq!(proposal.base_revision(), &base);
        assert_ne!(proposal.candidate_revision(), &base);
        assert!(proposal.digest().starts_with("sha256:"));
        assert!(!configuration_path(root.path()).exists());

        let publication = publish_instance_configuration(root.path(), &proposal).unwrap();
        assert_eq!(publication.base_revision(), &base);
        assert_eq!(publication.revision(), proposal.candidate_revision());
        assert_eq!(publication.proposal_digest(), proposal.digest());
        assert_eq!(
            fs::read_to_string(configuration_path(root.path())).unwrap(),
            "greeting = \"hello\"\n"
        );
    }

    #[test]
    fn local_authority_dispatches_through_the_host_port() {
        let root = fixture_root();
        let authority: Arc<dyn PluginConfigurationAuthority> =
            Arc::new(LocalPluginRootAuthority::new(root.path()));
        let source = authority.source();
        let base = authority.inspect().unwrap().revision().clone();

        let proposal = authority
            .propose(
                &base,
                "example.agent",
                "default",
                b"greeting = \"authority\"\n",
            )
            .unwrap();
        assert!(!configuration_path(root.path()).exists());

        let publication = authority.publish(&proposal).unwrap();

        assert_eq!(source.kind(), "local_plugin_root");
        assert_eq!(source.reference(), "app");
        assert_eq!(publication.revision(), proposal.candidate_revision());
        assert_eq!(
            authority.inspect().unwrap().revision(),
            publication.revision()
        );
    }

    #[test]
    fn stale_publication_fails_with_a_typed_conflict_and_preserves_the_winner() {
        let root = fixture_root();
        let base = inspect_plugin_root(root.path()).unwrap().revision().clone();
        let first = proposal(root.path(), &base, b"greeting = \"first\"\n");
        let stale = proposal(root.path(), &base, b"greeting = \"stale\"\n");
        let first_publication = publish_instance_configuration(root.path(), &first).unwrap();

        let error = publish_instance_configuration(root.path(), &stale).unwrap_err();
        let conflict = error.downcast_ref::<PluginRootRevisionConflict>().unwrap();

        assert_eq!(conflict.expected(), &base);
        assert_eq!(conflict.current(), first_publication.revision());
        assert_eq!(
            fs::read_to_string(configuration_path(root.path())).unwrap(),
            "greeting = \"first\"\n"
        );
    }

    #[test]
    fn concurrent_publications_allow_exactly_one_winner() {
        let root = fixture_root();
        let base = inspect_plugin_root(root.path()).unwrap().revision().clone();
        let first = proposal(root.path(), &base, b"greeting = \"first\"\n");
        let second = proposal(root.path(), &base, b"greeting = \"second\"\n");
        let path = root.path().to_path_buf();
        let barrier = Arc::new(std::sync::Barrier::new(3));
        let handles = [first, second].map(|proposal| {
            let path = path.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                publish_instance_configuration(&path, &proposal)
            })
        });
        barrier.wait();
        let outcomes = handles.map(|handle| handle.join().unwrap());

        assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
        let error = outcomes.into_iter().find_map(Result::err).unwrap();
        assert!(error.downcast_ref::<PluginRootRevisionConflict>().is_some());
        let contents = fs::read_to_string(configuration_path(&path)).unwrap();
        assert!(contents == "greeting = \"first\"\n" || contents == "greeting = \"second\"\n");
    }

    #[test]
    fn rejected_proposal_keeps_structured_diagnostics_without_writing() {
        let root = fixture_root();
        let base = inspect_plugin_root(root.path()).unwrap().revision().clone();
        let proposal = proposal(root.path(), &base, b"unexpected = true\n");

        assert_eq!(
            proposal.status(),
            PluginConfigurationProposalStatus::Rejected
        );
        assert_eq!(
            proposal.application(),
            PluginConfigurationApplication::Blocked
        );
        assert_eq!(proposal.diagnostics()[0].code(), "invalid_configuration");
        assert!(!configuration_path(root.path()).exists());
        assert!(publish_instance_configuration(root.path(), &proposal).is_err());
    }

    #[test]
    fn plugin_root_revision_is_semantic_not_toml_formatting() {
        let root = fixture_root();
        let base = inspect_plugin_root(root.path()).unwrap().revision().clone();
        let proposal = proposal(root.path(), &base, b"greeting = \"hello\"\n");
        let publication = publish_instance_configuration(root.path(), &proposal).unwrap();
        fs::write(
            configuration_path(root.path()),
            b"# human note\n\ngreeting=\"hello\"\n",
        )
        .unwrap();

        let reformatted = inspect_plugin_root(root.path()).unwrap();
        assert_eq!(reformatted.revision(), publication.revision());
    }

    #[test]
    fn plugin_root_revision_round_trips_for_http_preconditions() {
        let root = fixture_root();
        let revision = inspect_plugin_root(root.path()).unwrap().revision().clone();

        assert_eq!(
            revision.as_str().parse::<PluginRootRevision>().unwrap(),
            revision
        );
        assert!("sha256:not-a-digest".parse::<PluginRootRevision>().is_err());
    }

    fn proposal(
        root: &Path,
        base: &PluginRootRevision,
        toml: &[u8],
    ) -> PluginConfigurationProposal {
        propose_instance_configuration(root, base, "example.agent", "default", toml).unwrap()
    }

    fn configuration_path(root: &Path) -> std::path::PathBuf {
        root.join("plugins/example.agent/default.toml")
    }
}
