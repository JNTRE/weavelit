//! Atomic construction of the deployment's complete initial application state.
//!
//! The state this module builds is the whole first state or it is nothing.
//! Every identifier, verifier, sealed secret, and assignment is assembled into
//! one candidate that the persistence contract validates as a unit, so a
//! rejected candidate leaves no partially constructed state behind and nothing
//! for a later step to complete.

use weavelit_server_authentication::PasswordVerifierFactory;
use weavelit_server_database::{
    Account, AccountAuditReference, AccountPasswordVerifier, AccountPublicIdentifier,
    AccountPublicIdentity, ApplicationState, ApplicationStateInput, AuditReferenceIdentifier,
    CompletionObligation, ComponentKind, ConfigurationEntry, ConfigurationKey, ConfigurationValue,
    CredentialRevision, Group, GroupAuditReference, GroupGrant, GroupGrantRecord, GroupMembership,
    GroupPublicIdentifier, GroupPublicIdentity, LogAssignment, LogConfigurationAuditReference,
    LogModuleConfiguration, LogType, Name, PasswordVerifier, ProtectedSecret,
    STATE_IDENTIFIER_LENGTH, StateIdentifier,
};
use weavelit_server_lifecycle::{ProtectedValueKind, ProtectedValueSealer};

use crate::{AuthorizedInit, InitCheckpoint, InitError, ValidatedRequest};

/// Name of the system-defined Group Init creates.
///
/// The Group is system-defined, so its name is fixed here rather than accepted
/// from a request.
pub const ADMINISTRATORS_GROUP_NAME: &str = "Administrators";

/// Builds the complete initial application state for a validated request.
///
/// The [`AuthorizedInit`] borrow is what allows the submitted password and
/// protected settings to be read at all, so this function cannot be reached
/// before the lifecycle authority answered.
///
/// # Errors
///
/// Returns [`InitError::InitializationFailed`] when randomness is unavailable,
/// a verifier cannot be created, a secret cannot be sealed, or the assembled
/// candidate does not satisfy the persistence contract.
pub(crate) fn build_initial_state(
    authorized: &AuthorizedInit,
    validated: &ValidatedRequest<'_>,
    checkpoint: &InitCheckpoint,
    administration_client_module: &Name,
    verifier_factory: &PasswordVerifierFactory,
    sealer: &dyn ProtectedValueSealer,
    completion_obligation: CompletionObligation,
) -> Result<ApplicationState, InitError> {
    let request = validated.request();

    let account_identifier = state_identifier()?;
    let group_identifier = state_identifier()?;
    let account_public_identifier =
        AccountPublicIdentifier::generate().map_err(|_| InitError::InitializationFailed)?;
    let group_public_identifier =
        GroupPublicIdentifier::generate().map_err(|_| InitError::InitializationFailed)?;
    let account_audit_reference =
        AuditReferenceIdentifier::generate().map_err(|_| InitError::InitializationFailed)?;
    let group_audit_reference =
        AuditReferenceIdentifier::generate().map_err(|_| InitError::InitializationFailed)?;

    let account = Account {
        identifier: account_identifier,
        username: request.administrator.username.clone(),
        display_name: request.administrator.display_name.clone(),
        active: true,
        // The first local Human User is created without an enrolled factor, so
        // requiring one here would lock the deployment out of its own first
        // sign-in. Enrollment is a later, separately authorized act.
        mfa_required: false,
        credential_revision: CredentialRevision::INITIAL,
        must_change_password: false,
        temporary_credential_expiration: None,
    };

    let created = verifier_factory
        .create(request.administrator.password.expose(authorized))
        .map_err(|_| InitError::InitializationFailed)?;
    let verifier = PasswordVerifier::new(created.into_string())
        .map_err(|_| InitError::InitializationFailed)?;

    let group = Group {
        identifier: group_identifier,
        name: Name::new(ADMINISTRATORS_GROUP_NAME).map_err(|_| InitError::InitializationFailed)?,
        description: None,
    };

    // Exactly the two grants the system-defined Group carries. There is no
    // named Operation grant, and a request has no field that could add one.
    let group_grants = vec![
        GroupGrantRecord {
            group: group_identifier,
            grant: GroupGrant::ClientModule(administration_client_module.clone()),
        },
        GroupGrantRecord {
            group: group_identifier,
            grant: GroupGrant::ServerAdministration,
        },
    ];

    let mut log_module_configurations = Vec::with_capacity(request.log_module_configurations.len());
    let mut log_configuration_audit_references =
        Vec::with_capacity(request.log_module_configurations.len());
    let mut protected_secrets = Vec::new();
    for configuration in &request.log_module_configurations {
        let identifier = state_identifier()?;
        let audit_reference =
            AuditReferenceIdentifier::generate().map_err(|_| InitError::InitializationFailed)?;
        for setting in &configuration.protected_settings {
            protected_secrets.push(ProtectedSecret {
                component: configuration.module.clone(),
                key: setting.key.clone(),
                value: sealer.seal(
                    ProtectedValueKind::ComponentSecret,
                    setting.value.expose(authorized),
                )?,
            });
        }
        log_module_configurations.push(LogModuleConfiguration {
            identifier,
            module: configuration.module.clone(),
            name: configuration.name.clone(),
            enabled: configuration.enabled,
            settings: configuration.settings.clone(),
        });
        log_configuration_audit_references.push(LogConfigurationAuditReference::new(
            identifier,
            audit_reference,
        ));
    }

    let log_assignments = vec![
        LogAssignment {
            log_type: LogType::System,
            configuration: resolve(&log_module_configurations, &request.system_log)?,
        },
        LogAssignment {
            log_type: LogType::Audit,
            configuration: resolve(&log_module_configurations, &request.audit_log)?,
        },
    ];

    ApplicationState::new(ApplicationStateInput {
        configuration: vec![ConfigurationEntry {
            component: Name::new("totp").map_err(|_| InitError::InitializationFailed)?,
            key: ConfigurationKey::new(ComponentKind::MfaModule.enablement_key())
                .map_err(|_| InitError::InitializationFailed)?,
            value: ConfigurationValue::new("false").map_err(|_| InitError::InitializationFailed)?,
        }],
        protected_secrets,
        accounts: vec![account],
        account_public_identities: vec![AccountPublicIdentity::new(
            account_identifier,
            account_public_identifier,
        )],
        account_audit_references: vec![AccountAuditReference::new(
            account_identifier,
            account_audit_reference,
        )],
        password_verifiers: vec![AccountPasswordVerifier {
            account: account_identifier,
            verifier,
        }],
        groups: vec![group],
        group_public_identities: vec![GroupPublicIdentity::new(
            group_identifier,
            group_public_identifier,
        )],
        group_audit_references: vec![GroupAuditReference::new(
            group_identifier,
            group_audit_reference,
        )],
        group_memberships: vec![GroupMembership {
            group: group_identifier,
            account: account_identifier,
        }],
        group_grants,
        // Init creates the first user without an enrolled MFA factor and
        // without any Service Connection.
        mfa_factors: Vec::new(),
        service_connections: Vec::new(),
        recovery_public_key: checkpoint.recovery_public_key().clone(),
        log_module_configurations,
        log_configuration_audit_references,
        log_assignments,
        completion_obligation,
    })
    .map_err(|_| InitError::InitializationFailed)
}

/// Resolves an assignment's configuration name to its generated identifier.
fn resolve(
    configurations: &[LogModuleConfiguration],
    name: &Name,
) -> Result<StateIdentifier, InitError> {
    configurations
        .iter()
        .find(|configuration| &configuration.name == name)
        .map(|configuration| configuration.identifier)
        .ok_or(InitError::InitializationFailed)
}

/// Generates one state identifier from operating-system randomness.
fn state_identifier() -> Result<StateIdentifier, InitError> {
    let mut bytes = [0_u8; STATE_IDENTIFIER_LENGTH];
    getrandom::fill(&mut bytes).map_err(|_| InitError::InitializationFailed)?;
    StateIdentifier::from_bytes(bytes).map_err(|_| InitError::InitializationFailed)
}
