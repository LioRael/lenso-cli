//! Resolve module instances and typed capability slots without performing application I/O.

use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// A catalog is supplied by an extension from its own verified distribution source.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    /// Composition schema version, currently 1.
    pub schema_version: u32,
    /// Local module identities, independent of executable extension package IDs.
    pub modules: BTreeMap<String, Module>,
}

/// Reusable behavior with typed configuration and capability requirements.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Module {
    /// Module version within this immutable catalog snapshot.
    pub version: Version,
    /// Capabilities implemented by this module.
    #[serde(default)]
    pub provides: BTreeMap<String, Provision>,
    /// Consumer-owned capability slots.
    #[serde(default)]
    pub slots: BTreeMap<String, Slot>,
    /// Configuration fields; defaults are explicit catalog decisions.
    #[serde(default)]
    pub options: BTreeMap<String, OptionSpec>,
    /// Supported project context values such as language and target runtime.
    #[serde(default)]
    pub supports: BTreeMap<String, Vec<String>>,
}

/// A contract implementation and its advertised structural properties.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provision {
    /// Capability contract version, distinct from the module's version.
    pub version: Version,
    /// Exact properties required by consumers, for example a storage dialect.
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
}

/// A dependency on behavior rather than a particular implementation package.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Slot {
    /// Stable capability contract identity.
    pub contract: String,
    /// Compatible capability contract versions.
    pub version: VersionReq,
    /// Required provider properties.
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
    /// Allow explicit disabling; this never makes `--yes` enable a feature.
    #[serde(default)]
    pub optional: bool,
    /// Activate only when all local configuration fields equal these values.
    #[serde(default)]
    pub when: BTreeMap<String, Value>,
}

/// Supported configuration types; secret references contain names, never secret values.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum OptionKind {
    /// A nonempty string.
    Text,
    /// A boolean domain choice, not authorization consent.
    Boolean,
    /// A signed integer with optional inclusive limits.
    Integer {
        /// Inclusive lower bound.
        min: Option<i64>,
        /// Inclusive upper bound.
        max: Option<i64>,
    },
    /// One exact enumerated value.
    Select {
        /// Allowed values.
        values: Vec<String>,
    },
    /// An object of the form `{"env":"VARIABLE_NAME"}`.
    Environment,
}

/// An authored question and optional explicit default.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OptionSpec {
    /// Human-readable question text.
    pub prompt: String,
    /// Expected value type.
    pub kind: OptionKind,
    /// An explicit authored default; missing defaults require input.
    #[serde(default)]
    pub default: Option<Value>,
}

impl OptionSpec {
    /// Validate values without reading environment variables or coercing JSON inputs.
    pub fn accepts(&self, value: &Value) -> bool {
        match &self.kind {
            OptionKind::Text => value.as_str().is_some_and(|text| !text.trim().is_empty()),
            OptionKind::Boolean => value.is_boolean(),
            OptionKind::Integer { min, max } => value.as_i64().is_some_and(|number| {
                min.is_none_or(|min| number >= min) && max.is_none_or(|max| number <= max)
            }),
            OptionKind::Select { values } => value
                .as_str()
                .is_some_and(|value| values.iter().any(|allowed| allowed == value)),
            OptionKind::Environment => value.as_object().is_some_and(|object| {
                object.len() == 1
                    && object
                        .get("env")
                        .and_then(Value::as_str)
                        .is_some_and(environment_name)
            }),
        }
    }
}

/// A provider choice creates a separate instance unless sharing is explicit.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Binding {
    /// Instantiate a module at the consuming slot's address.
    Module {
        /// Catalog module identity.
        module: String,
    },
    /// Share a named root instance explicitly.
    Instance {
        /// Root instance identity.
        instance: String,
    },
    /// Disable an optional slot.
    Disabled,
}

/// User intent; bindings and configuration are scoped to stable instance addresses.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Request {
    /// Root instance name to catalog module identity.
    pub instances: BTreeMap<String, String>,
    /// Instance-scoped field values, including automatically created children.
    pub configuration: BTreeMap<String, BTreeMap<String, Value>>,
    /// Slot addresses such as `auth.store`, never a global capability singleton.
    pub bindings: BTreeMap<String, Binding>,
    /// Explicit project facts used to filter supported implementations.
    pub context: BTreeMap<String, String>,
}

/// A pending domain decision, reusable by terminal, JSON and Agent clients.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Choice {
    /// Configuration required by one exact instance.
    Configuration {
        /// Instance address.
        instance: String,
        /// Field name.
        field: String,
        /// Type, prompt and authored default.
        option: OptionSpec,
    },
    /// A missing project context fact.
    Context {
        /// Context key.
        field: String,
        /// Accepted values.
        values: Vec<String>,
    },
    /// A capability with multiple eligible implementations or optional absence.
    Provider {
        /// Consumer slot address.
        slot: String,
        /// Required contract identity.
        contract: String,
        /// Eligible module identities.
        modules: Vec<String>,
        /// Eligible explicit root instances.
        instances: Vec<String>,
        /// Whether absence is allowed.
        optional: bool,
    },
}

/// A solved instance and its explicit runtime dependencies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instance {
    /// Selected module identity.
    pub module: String,
    /// Selected module version.
    pub version: Version,
    /// Validated values including explicit defaults and environment references.
    pub configuration: BTreeMap<String, Value>,
    /// Slot names mapped to instance addresses; `None` means inactive or disabled.
    pub bindings: BTreeMap<String, Option<String>>,
}

/// A deterministic composition artifact, ready for extension-owned source generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resolution {
    /// Composition schema version.
    pub schema_version: u32,
    /// Content digest of the module catalog, not permission to execute it.
    pub catalog_sha256: String,
    /// Explicit project context.
    pub context: BTreeMap<String, String>,
    /// Fully resolved instances.
    pub instances: BTreeMap<String, Instance>,
    /// Dependency-first order, independent of source file ordering.
    pub order: Vec<String>,
}

impl Resolution {
    /// Compute a stable digest over modules, configuration, bindings, context and ordering.
    /// # Errors
    /// Returns a serialization failure for values outside JSON's representation.
    pub fn digest(&self) -> Result<String, Error> {
        Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(self)?)))
    }
}

/// Pure evaluation either completes or requests an explicit domain decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Evaluation {
    /// An extension can now render code from this resolution.
    Complete {
        /// Resolved instances and bindings.
        resolution: Resolution,
    },
    /// The host must collect another value before code generation.
    NeedsInput {
        /// Typed pending decision.
        choice: Choice,
    },
}

/// Invalid metadata or incompatible composition, with an instance/slot path.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A definition or request violates the composition contract.
    #[error("{0}")]
    Invalid(String),
    /// An artifact could not be serialized.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

fn segment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
}
fn environment_name(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().enumerate().all(|(i, byte)| {
            byte == b'_' || byte.is_ascii_alphabetic() || i > 0 && byte.is_ascii_digit()
        })
}
fn matches(module: &Module, slot: &Slot, context: &BTreeMap<String, String>) -> bool {
    module.provides.get(&slot.contract).is_some_and(|provided| {
        slot.version.matches(&provided.version)
            && slot
                .attributes
                .iter()
                .all(|(name, value)| provided.attributes.get(name) == Some(value))
    }) && module
        .supports
        .iter()
        .all(|(name, values)| context.get(name).is_none_or(|value| values.contains(value)))
}

impl Catalog {
    /// Validate all definitions, including currently unselected modules.
    /// # Errors
    /// Rejects malformed definitions and unsupported schema or graph limits.
    pub fn validate(&self) -> Result<(), Error> {
        if self.schema_version != 1 || self.modules.is_empty() || self.modules.len() > 128 {
            return Err(Error::Invalid(
                "composition catalog requires schema 1 and 1-128 modules".into(),
            ));
        }
        for (id, module) in &self.modules {
            if !segment(id)
                || module.options.len() > 64
                || module.slots.len() > 64
                || module.supports.len() > 32
                || module.provides.len() > 64
            {
                return Err(Error::Invalid(format!("invalid or oversized module {id}")));
            }
            for (field, option) in &module.options {
                if !segment(field)
                    || option.prompt.is_empty()
                    || option
                        .default
                        .as_ref()
                        .is_some_and(|value| !option.accepts(value))
                {
                    return Err(Error::Invalid(format!("invalid option {id}.{field}")));
                }
                match &option.kind {
                    OptionKind::Select { values }
                        if values.is_empty()
                            || values.len() > 128
                            || values.iter().any(String::is_empty)
                            || values.iter().collect::<BTreeSet<_>>().len() != values.len() =>
                    {
                        return Err(Error::Invalid(format!("invalid choices for {id}.{field}")));
                    }
                    OptionKind::Integer {
                        min: Some(min),
                        max: Some(max),
                    } if min > max => {
                        return Err(Error::Invalid(format!(
                            "invalid integer bounds for {id}.{field}"
                        )));
                    }
                    _ => {}
                }
            }
            for (slot, requirement) in &module.slots {
                if !segment(slot)
                    || requirement.contract.is_empty()
                    || requirement.contract.len() > 128
                {
                    return Err(Error::Invalid(format!(
                        "invalid capability slot {id}.{slot}"
                    )));
                }
                for (field, value) in &requirement.when {
                    if module
                        .options
                        .get(field)
                        .is_none_or(|option| !option.accepts(value))
                    {
                        return Err(Error::Invalid(format!(
                            "invalid activation condition for {id}.{slot}: {field}"
                        )));
                    }
                }
            }
            if module.supports.iter().any(|(key, values)| {
                !segment(key)
                    || values.is_empty()
                    || values.len() > 128
                    || values.iter().any(String::is_empty)
                    || values.iter().collect::<BTreeSet<_>>().len() != values.len()
            }) {
                return Err(Error::Invalid(format!("invalid context support in {id}")));
            }
        }
        Ok(())
    }

    /// Resolve typed capabilities and instances without fetching, rendering or writing files.
    /// # Errors
    /// Rejects incompatible bindings, unknown configuration, cycles and bounded-graph violations.
    pub fn evaluate(&self, request: &Request) -> Result<Evaluation, Error> {
        self.validate()?;
        if serde_json::to_vec(self)?.len() > 1024 * 1024
            || serde_json::to_vec(request)?.len() > 1024 * 1024
        {
            return Err(Error::Invalid(
                "composition metadata or request exceeds 1 MiB".into(),
            ));
        }
        if request.instances.is_empty()
            || request.instances.len() > 32
            || request.configuration.len() > 128
            || request.bindings.len() > 256
            || request.context.len() > 32
            || request.instances.keys().any(|id| !segment(id))
        {
            return Err(Error::Invalid(
                "composition requires 1-32 named root instances and bounded input".into(),
            ));
        }
        let mut state = State {
            catalog: self,
            request,
            instances: BTreeMap::new(),
            order: Vec::new(),
            active: Vec::new(),
            used_bindings: BTreeSet::new(),
            inactive: BTreeSet::new(),
        };
        for (id, module) in &request.instances {
            if let Some(choice) = state.visit(id, module)? {
                return Ok(Evaluation::NeedsInput { choice });
            }
        }
        if let Some(unused) = request
            .bindings
            .keys()
            .find(|key| !state.used_bindings.contains(*key) && !state.is_inactive(key))
        {
            return Err(Error::Invalid(format!(
                "unused or inactive binding {unused}"
            )));
        }
        if let Some(unused) = request
            .configuration
            .keys()
            .find(|key| !state.instances.contains_key(*key) && !state.is_inactive(key))
        {
            return Err(Error::Invalid(format!(
                "configuration targets unknown or inactive instance {unused}"
            )));
        }
        Ok(Evaluation::Complete {
            resolution: Resolution {
                schema_version: 1,
                catalog_sha256: format!("{:x}", Sha256::digest(serde_json::to_vec(self)?)),
                context: request.context.clone(),
                instances: state.instances,
                order: state.order,
            },
        })
    }
}

struct State<'a> {
    catalog: &'a Catalog,
    request: &'a Request,
    instances: BTreeMap<String, Instance>,
    order: Vec<String>,
    active: Vec<String>,
    used_bindings: BTreeSet<String>,
    inactive: BTreeSet<String>,
}
impl State<'_> {
    fn is_inactive(&self, address: &str) -> bool {
        self.inactive
            .iter()
            .any(|root| address == root || address.starts_with(&format!("{root}.")))
    }
    fn visit(&mut self, id: &str, module_id: &str) -> Result<Option<Choice>, Error> {
        if self.active.iter().any(|active| active == id) {
            return Err(Error::Invalid(format!(
                "composition cycle: {} -> {id}",
                self.active.join(" -> ")
            )));
        }
        if self.instances.contains_key(id) {
            return Ok(None);
        }
        if self.active.len() >= 32 || self.instances.len() + self.active.len() >= 128 {
            return Err(Error::Invalid(format!(
                "composition instance/depth limit reached at {id}"
            )));
        }
        let module = self
            .catalog
            .modules
            .get(module_id)
            .ok_or_else(|| Error::Invalid(format!("{id}: unknown module {module_id}")))?;
        self.active.push(id.into());
        for (field, values) in &module.supports {
            match self.request.context.get(field) {
                None => {
                    return Ok(Some(Choice::Context {
                        field: field.clone(),
                        values: values.clone(),
                    }));
                }
                Some(value) if !values.contains(value) => {
                    return Err(Error::Invalid(format!(
                        "{id}: unsupported context {field}={value}"
                    )));
                }
                _ => {}
            }
        }
        let supplied = self.request.configuration.get(id);
        if let Some(unknown) = supplied.and_then(|values| {
            values
                .keys()
                .find(|field| !module.options.contains_key(*field))
        }) {
            return Err(Error::Invalid(format!(
                "unknown configuration {id}.{unknown}"
            )));
        }
        let mut configuration = BTreeMap::new();
        for (field, option) in &module.options {
            let value = supplied
                .and_then(|values| values.get(field))
                .or(option.default.as_ref());
            let Some(value) = value else {
                return Ok(Some(Choice::Configuration {
                    instance: id.into(),
                    field: field.clone(),
                    option: option.clone(),
                }));
            };
            if !option.accepts(value) {
                return Err(Error::Invalid(format!(
                    "invalid configuration {id}.{field}"
                )));
            }
            configuration.insert(field.clone(), value.clone());
        }
        let mut bindings = BTreeMap::new();
        for (slot, required) in &module.slots {
            let address = format!("{id}.{slot}");
            if !required
                .when
                .iter()
                .all(|(field, value)| configuration.get(field) == Some(value))
            {
                self.inactive.insert(address.clone());
                bindings.insert(slot.clone(), None);
                continue;
            }
            let modules = self
                .catalog
                .modules
                .iter()
                .filter(|(_, module)| matches(module, required, &self.request.context))
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();
            let instances = self
                .request
                .instances
                .iter()
                .filter(|(_, module)| {
                    self.catalog
                        .modules
                        .get(*module)
                        .is_some_and(|module| matches(module, required, &self.request.context))
                })
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();
            let explicit = self.request.bindings.get(&address);
            if explicit.is_some() {
                self.used_bindings.insert(address.clone());
            }
            let automatic = if !required.optional && modules.len() == 1 && instances.is_empty() {
                Some(Binding::Module {
                    module: modules[0].clone(),
                })
            } else {
                None
            };
            let binding = explicit.or(automatic.as_ref());
            let Some(binding) = binding else {
                if modules.is_empty() && instances.is_empty() && !required.optional {
                    return Err(Error::Invalid(format!(
                        "{address}: no provider for {} {} with {:?}",
                        required.contract, required.version, required.attributes
                    )));
                }
                return Ok(Some(Choice::Provider {
                    slot: address,
                    contract: required.contract.clone(),
                    modules,
                    instances,
                    optional: required.optional,
                }));
            };
            let target = match binding {
                Binding::Disabled if required.optional => {
                    self.inactive.insert(address.clone());
                    bindings.insert(slot.clone(), None);
                    continue;
                }
                Binding::Disabled => {
                    return Err(Error::Invalid(format!(
                        "{address}: required slot cannot be disabled"
                    )));
                }
                Binding::Module { module } if modules.contains(module) => {
                    if let Some(choice) = self.visit(&address, module)? {
                        return Ok(Some(choice));
                    }
                    address.clone()
                }
                Binding::Instance { instance } if instances.contains(instance) => {
                    let target_module = &self.request.instances[instance];
                    if let Some(choice) = self.visit(instance, target_module)? {
                        return Ok(Some(choice));
                    }
                    instance.clone()
                }
                _ => {
                    return Err(Error::Invalid(format!(
                        "{address}: binding does not satisfy {} {}",
                        required.contract, required.version
                    )));
                }
            };
            bindings.insert(slot.clone(), Some(target));
        }
        self.active.pop();
        self.instances.insert(
            id.into(),
            Instance {
                module: module_id.into(),
                version: module.version.clone(),
                configuration,
                bindings,
            },
        );
        self.order.push(id.into());
        Ok(None)
    }
}
