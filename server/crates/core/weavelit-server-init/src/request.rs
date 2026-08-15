//! The normalized `InitializeServer` request and its Server semantic validation.
//!
//! The request describes the first local Human User, the initial Log Module
//! configurations, the explicit System Log and Audit Log assignments, and the
//! recovery-key proof. It deliberately cannot describe anything else: there is
//! no field for a client-defined Group, a grant, an MFA factor, a Service
//! Connection, or the selected database connection configuration, so those
//! cannot be smuggled into the first state by a request.

use weavelit_server_components::AvailableComponents;
use weavelit_server_database::{ConfigurationKey, LogModuleSetting, Name};
use weavelit_server_recovery_key::RecoveryProof;

use crate::{InitialPassword, InitialSecret, RequestError};

/// Maximum initial Log Module configurations accepted in one request.
pub const MAX_LOG_MODULE_CONFIGURATIONS: usize = 16;

/// Maximum settings accepted in one initial Log Module configuration.
pub const MAX_LOG_MODULE_SETTINGS: usize = 64;

/// Maximum protected settings accepted in one initial Log Module configuration.
pub const MAX_PROTECTED_LOG_MODULE_SETTINGS: usize = 16;

/// The first local Human User this deployment is initialized with.
#[derive(Clone, Debug)]
pub struct InitialAdministrator {
    /// Username of the first local Human User.
    pub username: Name,
    /// Optional display name of the first local Human User.
    pub display_name: Option<Name>,
    /// Submitted password the account's verifier is created from.
    pub password: InitialPassword,
}

/// One submitted Log Module setting whose value requires at-rest protection.
#[derive(Clone, Debug)]
pub struct InitialProtectedSetting {
    /// Setting key unique within the configuration.
    pub key: ConfigurationKey,
    /// Submitted secret value sealed before it is stored.
    pub value: InitialSecret,
}

/// One initial Log Module configuration.
#[derive(Clone, Debug)]
pub struct InitialLogModuleConfiguration {
    /// Log Module identifier, which must be compiled into this build.
    pub module: Name,
    /// Configuration name unique across the request.
    pub name: Name,
    /// Whether the configuration is enabled.
    pub enabled: bool,
    /// Non-secret settings for the configuration.
    pub settings: Vec<LogModuleSetting>,
    /// Secret settings sealed before they are stored.
    pub protected_settings: Vec<InitialProtectedSetting>,
}

/// The normalized request that initializes a Server.
#[derive(Clone, Debug)]
pub struct InitializeServer {
    /// The first local Human User.
    pub administrator: InitialAdministrator,
    /// Initial Log Module configurations.
    pub log_module_configurations: Vec<InitialLogModuleConfiguration>,
    /// Configuration name explicitly assigned to the System Log.
    pub system_log: Name,
    /// Configuration name explicitly assigned to the Audit Log.
    pub audit_log: Name,
    /// Proof that the requesting client retained the delivered recovery key.
    pub recovery_key_proof: Option<RecoveryProof>,
}

/// A request that passed Server semantic validation.
///
/// Its constructor is private to this crate and its only producer is
/// [`validate_request`], so a state builder cannot be handed a request that was
/// never validated. The borrow keeps the submitted secrets in the caller's
/// request rather than copying them into a second live value.
pub struct ValidatedRequest<'request> {
    request: &'request InitializeServer,
}

impl<'request> ValidatedRequest<'request> {
    /// Returns the validated request.
    #[must_use]
    pub const fn request(&self) -> &'request InitializeServer {
        self.request
    }
}

impl std::fmt::Debug for ValidatedRequest<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ValidatedRequest(REDACTED)")
    }
}

/// Validates one submitted request against Server semantics and this build.
///
/// Validation is complete before anything is created, so a request that names a
/// Log Module this build does not carry, repeats a configuration name, carries a
/// setting the named Log Module does not accept, or assigns a log type to a
/// disabled configuration is rejected while rejecting is still free.
///
/// # Errors
///
/// Returns [`RequestError`] describing the first violated rule. The variant is
/// for in-workspace attribution only and reaches a client solely as
/// [`crate::InitError::InitializationFailed`].
pub fn validate_request<'request>(
    request: &'request InitializeServer,
    components: &AvailableComponents,
) -> Result<ValidatedRequest<'request>, RequestError> {
    let configurations = &request.log_module_configurations;
    if configurations.is_empty() || configurations.len() > MAX_LOG_MODULE_CONFIGURATIONS {
        return Err(RequestError::CollectionOutOfBounds);
    }

    // A protected setting is stored against the owning Log Module component, so
    // two configurations of one module cannot both claim the same key. The
    // collision is rejected here rather than resolved by inventing a per
    // configuration component name that nothing else in the state model uses.
    let mut protected_keys: Vec<(&Name, &ConfigurationKey)> = Vec::new();

    for (index, configuration) in configurations.iter().enumerate() {
        if !components.has_log_module(&configuration.module) {
            return Err(RequestError::ComponentUnavailable);
        }
        if configurations
            .iter()
            .take(index)
            .any(|earlier| earlier.name == configuration.name)
        {
            return Err(RequestError::DuplicateEntry);
        }

        if configuration.settings.len() > MAX_LOG_MODULE_SETTINGS
            || configuration.protected_settings.len() > MAX_PROTECTED_LOG_MODULE_SETTINGS
        {
            return Err(RequestError::CollectionOutOfBounds);
        }
        if has_duplicate(configuration.settings.iter().map(|setting| &setting.key)) {
            return Err(RequestError::DuplicateEntry);
        }

        for setting in &configuration.protected_settings {
            let entry = (&configuration.module, &setting.key);
            if protected_keys.contains(&entry) {
                return Err(RequestError::DuplicateEntry);
            }
            protected_keys.push(entry);
        }

        // The module's own declaration, carried on the inventory, is the only
        // authority for which settings it accepts, so a configuration the named
        // module would refuse to open is caught while the pending delivery is
        // still claimable rather than at the finalization preflight, which has
        // no retry path short of redeployment. The comparison is pure: it reads
        // declared keys, opens no destination, and creates nothing. Secret
        // settings are deliberately outside the declaration and are never
        // carried to a module through it, so they are not judged here.
        let servable = components
            .log_settings_format(&configuration.module)
            .is_some_and(|format| {
                configuration
                    .settings
                    .iter()
                    .all(|setting| format.accepts(setting.key.as_str()))
            });
        if !servable {
            return Err(RequestError::SettingUnsupported);
        }
    }

    if request.system_log == request.audit_log {
        // Two log types may share a Log Module, but each is an independent
        // assignment: one configuration serving both would make the Audit Log's
        // retention and integrity inseparable from the System Log's.
        return Err(RequestError::DuplicateEntry);
    }

    for assignment in [&request.system_log, &request.audit_log] {
        let configuration = configurations
            .iter()
            .find(|configuration| &configuration.name == assignment)
            .ok_or(RequestError::UnresolvedAssignment)?;
        if !configuration.enabled {
            return Err(RequestError::DisabledAssignment);
        }
    }

    Ok(ValidatedRequest { request })
}

fn has_duplicate<'key>(keys: impl Iterator<Item = &'key ConfigurationKey>) -> bool {
    let keys: Vec<&ConfigurationKey> = keys.collect();
    keys.iter()
        .enumerate()
        .any(|(index, key)| keys.iter().take(index).any(|earlier| earlier == key))
}
