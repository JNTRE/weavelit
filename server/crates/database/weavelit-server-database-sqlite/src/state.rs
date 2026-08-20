use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension, Row, params};
use weavelit_server_database::{
    ACCOUNT_PUBLIC_IDENTIFIER_LENGTH, Account, AccountAdministrationProjection,
    AccountAdministrationStore, AccountAuditReference, AccountPasswordVerifier,
    AccountPublicIdentifier, AccountPublicIdentifierPersistence, AccountPublicIdentity,
    ApplicationState, ApplicationStateInput, AuditReferenceIdentifier, AuditReferencePersistence,
    BoundedText, COMPONENT_ENABLED_VALUE, CompletionObligation, ComponentEnablement, ComponentKind,
    ConfigurationEntry, DatabaseError, Group, GroupAuditReference, GroupGrant, GroupGrantRecord,
    GroupMembership, HumanAuthorizationSnapshot, LogAssignment, LogConfigurationAuditReference,
    LogModuleConfiguration, LogModuleSetting, LogType, MfaFactor, PasswordVerifier,
    ProtectedSecret, ProtectedValue, RecoveryPublicKey, STATE_IDENTIFIER_LENGTH, ServiceConnection,
    StateIdentifier, WorkflowKind,
};

use crate::SqliteDatabase;
use crate::error::{ErrorContext, map_sqlite_error};

const CONFIGURATION_QUERY: &str = "SELECT component, setting_key, setting_value \
     FROM weavelit_configuration ORDER BY component, setting_key";
const PROTECTED_SECRET_QUERY: &str = "SELECT component, secret_key, protected_value \
     FROM weavelit_protected_secret ORDER BY component, secret_key";
const ACCOUNT_QUERY: &str = "SELECT account_id, username, display_name, active, mfa_required \
     FROM weavelit_account ORDER BY account_id";
const ACCOUNT_PUBLIC_IDENTITY_QUERY: &str = "SELECT account_id, public_identifier \
    FROM weavelit_account_public_identity ORDER BY account_id";
const ACCOUNT_PUBLIC_IDENTITY_LOOKUP: &str = "SELECT account_id \
    FROM weavelit_account_public_identity WHERE public_identifier = ?1";
const ACCOUNT_ADMINISTRATION_LIST_QUERY: &str = "SELECT identity.public_identifier, \
    account.username, account.display_name, account.active, account.mfa_required \
    FROM weavelit_account AS account \
    JOIN weavelit_account_public_identity AS identity ON identity.account_id = account.account_id \
    ORDER BY account.username";
const ACCOUNT_ADMINISTRATION_LOOKUP_QUERY: &str = "SELECT identity.public_identifier, \
    account.username, account.display_name, account.active, account.mfa_required \
    FROM weavelit_account_public_identity AS identity \
    JOIN weavelit_account AS account ON account.account_id = identity.account_id \
    WHERE identity.public_identifier = ?1";
const ACCOUNT_PUBLIC_IDENTITY_COVERAGE_QUERY: &str = "SELECT \
    EXISTS(SELECT 1 FROM weavelit_account AS account \
        LEFT JOIN weavelit_account_public_identity AS identity \
        ON identity.account_id = account.account_id WHERE identity.account_id IS NULL) \
    OR EXISTS(SELECT 1 FROM weavelit_account_public_identity AS identity \
        LEFT JOIN weavelit_account AS account \
        ON account.account_id = identity.account_id WHERE account.account_id IS NULL)";
const ACCOUNT_PUBLIC_IDENTIFIER_VALUES_QUERY: &str = "SELECT public_identifier \
    FROM weavelit_account_public_identity ORDER BY account_id";
const ACCOUNT_AUDIT_REFERENCE_QUERY: &str = "SELECT account_id, audit_reference \
    FROM weavelit_account_audit_reference ORDER BY account_id";
const ACCOUNT_AUDIT_REFERENCE_LOOKUP: &str = "SELECT audit_reference \
    FROM weavelit_account_audit_reference WHERE account_id = ?1";
const PASSWORD_VERIFIER_QUERY: &str = "SELECT account_id, encoded_verifier \
     FROM weavelit_password_verifier ORDER BY account_id";
const GROUP_QUERY: &str =
    "SELECT group_id, name, description FROM weavelit_group ORDER BY group_id";
const GROUP_AUDIT_REFERENCE_QUERY: &str = "SELECT group_id, audit_reference \
    FROM weavelit_group_audit_reference ORDER BY group_id";
const GROUP_AUDIT_REFERENCE_LOOKUP: &str = "SELECT audit_reference \
    FROM weavelit_group_audit_reference WHERE group_id = ?1";
const GROUP_MEMBERSHIP_QUERY: &str = "SELECT group_id, account_id \
     FROM weavelit_group_membership ORDER BY group_id, account_id";
const GROUP_GRANT_QUERY: &str = "SELECT group_id, grant_kind, grant_value \
     FROM weavelit_group_grant ORDER BY group_id, grant_kind, grant_value";
const MFA_FACTOR_QUERY: &str = "SELECT factor_id, account_id, module, protected_factor_data \
     FROM weavelit_mfa_factor ORDER BY factor_id";
const SERVICE_CONNECTION_QUERY: &str = "SELECT connection_id, service_module, name, protected_credential \
     FROM weavelit_service_connection ORDER BY connection_id";
const RECOVERY_PUBLIC_KEY_QUERY: &str = "SELECT public_key \
     FROM weavelit_recovery_public_key ORDER BY singleton LIMIT 2";
const LOG_MODULE_CONFIGURATION_QUERY: &str = "SELECT configuration_id, module, name, enabled \
     FROM weavelit_log_module_configuration ORDER BY configuration_id";
const LOG_CONFIGURATION_AUDIT_REFERENCE_QUERY: &str = "SELECT configuration_id, audit_reference \
    FROM weavelit_log_configuration_audit_reference ORDER BY configuration_id";
const LOG_CONFIGURATION_AUDIT_REFERENCE_LOOKUP: &str = "SELECT audit_reference \
    FROM weavelit_log_configuration_audit_reference WHERE configuration_id = ?1";
const LOG_MODULE_SETTING_QUERY: &str = "SELECT configuration_id, setting_key, setting_value \
     FROM weavelit_log_module_setting ORDER BY configuration_id, setting_key";
const LOG_ASSIGNMENT_QUERY: &str = "SELECT log_type, configuration_id \
     FROM weavelit_log_assignment ORDER BY log_type";
const COMPLETION_OBLIGATION_QUERY: &str = "SELECT record_id, workflow_kind, classification, \
     correlation_identifier, event_time_milliseconds, detail, acknowledged \
     FROM weavelit_completion_obligation ORDER BY singleton LIMIT 2";
/// Reads one account's active flag without touching any other account column.
const ACCOUNT_ACTIVE_QUERY: &str = "SELECT active FROM weavelit_account WHERE account_id = ?1";
/// Joins the distinct grants of every Group the account belongs to.
///
/// The projection selects only the grant kind and value, so the conferring
/// Group's identifier, name, and description never leave the database.
const HUMAN_AUTHORIZATION_GRANT_QUERY: &str = "SELECT DISTINCT conferred.grant_kind, \
     conferred.grant_value FROM weavelit_group_grant AS conferred \
     JOIN weavelit_group_membership AS membership \
     ON membership.group_id = conferred.group_id \
     WHERE membership.account_id = ?1 \
     ORDER BY conferred.grant_kind, conferred.grant_value";
/// Reads only the enablement entries that disable a component.
///
/// The predicate is written as "not exactly the enabled value" rather than as
/// an equality against a disabled value, so an unrecognized or corrupted flag
/// disables the component instead of leaving it reachable. Only the owning
/// component and the enablement key are selected; no other setting, and no
/// setting value, leaves the database through this read.
const DISABLED_COMPONENT_QUERY: &str = "SELECT component, setting_key \
     FROM weavelit_configuration \
     WHERE setting_key IN (?1, ?2, ?3, ?4) AND setting_value <> ?5 \
     ORDER BY setting_key, component";

type ConfigurationRow = (String, String, String);
type ProtectedSecretRow = (String, String, Vec<u8>);
type AccountRow = (Vec<u8>, String, Option<String>, i64, i64);
type AccountPublicIdentityRow = (Vec<u8>, Vec<u8>);
type AccountAdministrationRow = (Vec<u8>, String, Option<String>, i64, i64);
type AuditReferenceRow = (Vec<u8>, String);
type PasswordVerifierRow = (Vec<u8>, String);
type GroupRow = (Vec<u8>, String, Option<String>);
type GroupMembershipRow = (Vec<u8>, Vec<u8>);
type GroupGrantRow = (Vec<u8>, String, String);
type MfaFactorRow = (Vec<u8>, Vec<u8>, String, Vec<u8>);
type ServiceConnectionRow = (Vec<u8>, String, String, Vec<u8>);
type LogModuleConfigurationRow = (Vec<u8>, String, String, i64);
type LogModuleSettingRow = (Vec<u8>, String, String);
type LogAssignmentRow = (String, Vec<u8>);
type CompletionObligationRow = (Vec<u8>, String, String, String, i64, String, i64);
type HumanAuthorizationGrantRow = (String, String);
type DisabledComponentRow = (String, String);

impl SqliteDatabase {
    pub(super) fn load_account_public_identity_atomic(
        &mut self,
        persistence: &AccountPublicIdentifierPersistence,
        public_identifier: AccountPublicIdentifier,
    ) -> Result<Option<AccountPublicIdentity>, DatabaseError> {
        let persisted = persistence.encode(&public_identifier);
        let account = self
            .connection
            .query_row(
                ACCOUNT_PUBLIC_IDENTITY_LOOKUP,
                params![persisted.as_slice()],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()
            .map_err(|error| map_sqlite_error(error, ErrorContext::State))?;

        account
            .map(|account| {
                Ok(AccountPublicIdentity::new(
                    identifier(&account)?,
                    public_identifier,
                ))
            })
            .transpose()
    }

    /// Reads one account's active flag and joined Group grants consistently.
    ///
    /// Both reads run inside one transaction, so a concurrent membership or
    /// grant change cannot produce a snapshot that mixes two states.
    pub(super) fn load_human_authorization_atomic(
        &mut self,
        account: StateIdentifier,
    ) -> Result<Option<HumanAuthorizationSnapshot>, DatabaseError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(|error| map_sqlite_error(error, ErrorContext::State))?;

        read_human_authorization(&transaction, account)
    }

    /// Reads which components are currently disabled.
    pub(super) fn load_component_enablement_atomic(
        &mut self,
    ) -> Result<ComponentEnablement, DatabaseError> {
        read_component_enablement(&self.connection)
    }

    pub(super) fn load_account_audit_reference_atomic(
        &mut self,
        persistence: &AuditReferencePersistence,
        account: StateIdentifier,
    ) -> Result<Option<AccountAuditReference>, DatabaseError> {
        read_audit_reference(
            &self.connection,
            persistence,
            ACCOUNT_AUDIT_REFERENCE_LOOKUP,
            account,
        )
        .map(|reference| reference.map(|value| AccountAuditReference::new(account, value)))
    }

    pub(super) fn load_group_audit_reference_atomic(
        &mut self,
        persistence: &AuditReferencePersistence,
        group: StateIdentifier,
    ) -> Result<Option<GroupAuditReference>, DatabaseError> {
        read_audit_reference(
            &self.connection,
            persistence,
            GROUP_AUDIT_REFERENCE_LOOKUP,
            group,
        )
        .map(|reference| reference.map(|value| GroupAuditReference::new(group, value)))
    }

    pub(super) fn load_log_configuration_audit_reference_atomic(
        &mut self,
        persistence: &AuditReferencePersistence,
        configuration: StateIdentifier,
    ) -> Result<Option<LogConfigurationAuditReference>, DatabaseError> {
        read_audit_reference(
            &self.connection,
            persistence,
            LOG_CONFIGURATION_AUDIT_REFERENCE_LOOKUP,
            configuration,
        )
        .map(|reference| {
            reference.map(|value| LogConfigurationAuditReference::new(configuration, value))
        })
    }
}

impl AccountAdministrationStore for SqliteDatabase {
    fn list_account_administration_projections(
        &mut self,
        persistence: &AccountPublicIdentifierPersistence,
    ) -> Result<Vec<AccountAdministrationProjection>, DatabaseError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(|error| map_sqlite_error(error, ErrorContext::State))?;
        validate_account_public_identities(&transaction, persistence)?;
        parameterized_rows::<AccountAdministrationRow>(
            &transaction,
            ACCOUNT_ADMINISTRATION_LIST_QUERY,
            &[],
            five_columns,
        )?
        .into_iter()
        .map(|row| account_administration_projection(persistence, row))
        .collect()
    }

    fn load_account_administration_projection(
        &mut self,
        persistence: &AccountPublicIdentifierPersistence,
        public_identifier: AccountPublicIdentifier,
    ) -> Result<Option<AccountAdministrationProjection>, DatabaseError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(|error| map_sqlite_error(error, ErrorContext::State))?;
        validate_account_public_identities(&transaction, persistence)?;
        let persisted = persistence.encode(&public_identifier);
        transaction
            .query_row(
                ACCOUNT_ADMINISTRATION_LOOKUP_QUERY,
                params![persisted.as_slice()],
                five_columns,
            )
            .optional()
            .map_err(|error| map_sqlite_error(error, ErrorContext::State))?
            .map(|row| account_administration_projection(persistence, row))
            .transpose()
    }
}

fn validate_account_public_identities(
    connection: &Connection,
    persistence: &AccountPublicIdentifierPersistence,
) -> Result<(), DatabaseError> {
    let incomplete: i64 = connection
        .query_row(ACCOUNT_PUBLIC_IDENTITY_COVERAGE_QUERY, [], |row| row.get(0))
        .map_err(|error| map_sqlite_error(error, ErrorContext::State))?;
    if boolean(incomplete)? {
        return Err(DatabaseError::IntegrityFailure);
    }

    let identifiers = rows::<Vec<u8>>(connection, ACCOUNT_PUBLIC_IDENTIFIER_VALUES_QUERY, |row| {
        row.get(0)
    })?
    .into_iter()
    .map(|value| account_public_identifier(persistence, &value))
    .collect::<Result<Vec<_>, DatabaseError>>()?;
    if identifiers.iter().copied().collect::<BTreeSet<_>>().len() != identifiers.len() {
        return Err(DatabaseError::IntegrityFailure);
    }

    Ok(())
}

fn account_administration_projection(
    persistence: &AccountPublicIdentifierPersistence,
    (public_identifier, username, display_name, active, mfa_required): AccountAdministrationRow,
) -> Result<AccountAdministrationProjection, DatabaseError> {
    Ok(AccountAdministrationProjection::from_persistence(
        persistence,
        account_public_identifier(persistence, &public_identifier)?,
        text(username)?,
        display_name.map(text).transpose()?,
        boolean(active)?,
        boolean(mfa_required)?,
    ))
}

fn read_audit_reference(
    connection: &Connection,
    persistence: &AuditReferencePersistence,
    query: &str,
    entity: StateIdentifier,
) -> Result<Option<AuditReferenceIdentifier>, DatabaseError> {
    let value = connection
        .query_row(query, params![entity.as_bytes().as_slice()], |row| {
            row.get::<_, String>(0)
        })
        .optional()
        .map_err(|error| map_sqlite_error(error, ErrorContext::State))?;
    value
        .map(|value| audit_reference(persistence, &value))
        .transpose()
}

fn read_component_enablement(
    connection: &Connection,
) -> Result<ComponentEnablement, DatabaseError> {
    let [client_module, service_module, operation, mfa_module] =
        ComponentKind::ALL.map(ComponentKind::enablement_key);
    let disabled = parameterized_rows::<DisabledComponentRow>(
        connection,
        DISABLED_COMPONENT_QUERY,
        params![
            client_module,
            service_module,
            operation,
            mfa_module,
            COMPONENT_ENABLED_VALUE
        ],
        two_columns,
    )?
    .into_iter()
    .map(|(component, key)| Ok((decode_component_kind(&key)?, text(component)?)))
    .collect::<Result<Vec<_>, DatabaseError>>()?;

    Ok(ComponentEnablement::new(disabled))
}

fn decode_component_kind(key: &str) -> Result<ComponentKind, DatabaseError> {
    ComponentKind::ALL
        .into_iter()
        .find(|kind| kind.enablement_key() == key)
        .ok_or(DatabaseError::IntegrityFailure)
}

fn read_human_authorization(
    connection: &Connection,
    account: StateIdentifier,
) -> Result<Option<HumanAuthorizationSnapshot>, DatabaseError> {
    let active = connection
        .query_row(
            ACCOUNT_ACTIVE_QUERY,
            params![account.as_bytes().as_slice()],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|error| map_sqlite_error(error, ErrorContext::State))?;
    // An account the database does not hold is reported as absent rather than
    // as a held account that happens to confer no grant.
    let Some(active) = active else {
        return Ok(None);
    };

    let grants = parameterized_rows::<HumanAuthorizationGrantRow>(
        connection,
        HUMAN_AUTHORIZATION_GRANT_QUERY,
        params![account.as_bytes().as_slice()],
        two_columns,
    )?
    .into_iter()
    .map(|(kind, value)| decode_grant(&kind, value))
    .collect::<Result<Vec<_>, DatabaseError>>()?;

    Ok(Some(HumanAuthorizationSnapshot::new(
        boolean(active)?,
        grants,
    )))
}

pub(super) fn write(
    connection: &Connection,
    public_identity_persistence: &AccountPublicIdentifierPersistence,
    state: &ApplicationState,
) -> Result<(), DatabaseError> {
    for entry in state.configuration() {
        execute(
            connection,
            "INSERT INTO weavelit_configuration (component, setting_key, setting_value) \
             VALUES (?1, ?2, ?3)",
            params![
                entry.component.as_str(),
                entry.key.as_str(),
                entry.value.as_str()
            ],
        )?;
    }
    for secret in state.protected_secrets() {
        execute(
            connection,
            "INSERT INTO weavelit_protected_secret (component, secret_key, protected_value) \
             VALUES (?1, ?2, ?3)",
            params![
                secret.component.as_str(),
                secret.key.as_str(),
                secret.value.as_bytes()
            ],
        )?;
    }
    for account in state.accounts() {
        execute(
            connection,
            "INSERT INTO weavelit_account \
             (account_id, username, display_name, active, mfa_required) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                account.identifier.as_bytes().as_slice(),
                account.username.as_str(),
                account.display_name.as_ref().map(BoundedText::as_str),
                i64::from(account.active),
                i64::from(account.mfa_required),
            ],
        )?;
    }
    for identity in state.account_public_identities() {
        let public_identifier = public_identity_persistence.encode(&identity.public_identifier());
        execute(
            connection,
            "INSERT INTO weavelit_account_public_identity \
             (account_id, public_identifier) VALUES (?1, ?2)",
            params![
                identity.account().as_bytes().as_slice(),
                public_identifier.as_slice(),
            ],
        )?;
    }
    for verifier in state.password_verifiers() {
        execute(
            connection,
            "INSERT INTO weavelit_password_verifier (account_id, encoded_verifier) \
             VALUES (?1, ?2)",
            params![
                verifier.account.as_bytes().as_slice(),
                verifier.verifier.as_str()
            ],
        )?;
    }
    for group in state.groups() {
        execute(
            connection,
            "INSERT INTO weavelit_group (group_id, name, description) VALUES (?1, ?2, ?3)",
            params![
                group.identifier.as_bytes().as_slice(),
                group.name.as_str(),
                group.description.as_ref().map(BoundedText::as_str),
            ],
        )?;
    }
    for reference in state.account_audit_references() {
        execute(
            connection,
            "INSERT INTO weavelit_account_audit_reference \
             (account_id, audit_reference) VALUES (?1, ?2)",
            params![
                reference.account().as_bytes().as_slice(),
                reference.audit_reference().to_string()
            ],
        )?;
    }
    for reference in state.group_audit_references() {
        execute(
            connection,
            "INSERT INTO weavelit_group_audit_reference \
             (group_id, audit_reference) VALUES (?1, ?2)",
            params![
                reference.group().as_bytes().as_slice(),
                reference.audit_reference().to_string()
            ],
        )?;
    }
    for membership in state.group_memberships() {
        execute(
            connection,
            "INSERT INTO weavelit_group_membership (group_id, account_id) VALUES (?1, ?2)",
            params![
                membership.group.as_bytes().as_slice(),
                membership.account.as_bytes().as_slice()
            ],
        )?;
    }
    for record in state.group_grants() {
        let (kind, value) = encode_grant(&record.grant);
        execute(
            connection,
            "INSERT INTO weavelit_group_grant (group_id, grant_kind, grant_value) \
             VALUES (?1, ?2, ?3)",
            params![record.group.as_bytes().as_slice(), kind, value],
        )?;
    }
    for factor in state.mfa_factors() {
        execute(
            connection,
            "INSERT INTO weavelit_mfa_factor \
             (factor_id, account_id, module, protected_factor_data) VALUES (?1, ?2, ?3, ?4)",
            params![
                factor.identifier.as_bytes().as_slice(),
                factor.account.as_bytes().as_slice(),
                factor.module.as_str(),
                factor.protected_factor_data.as_bytes(),
            ],
        )?;
    }
    for connection_record in state.service_connections() {
        execute(
            connection,
            "INSERT INTO weavelit_service_connection \
             (connection_id, service_module, name, protected_credential) VALUES (?1, ?2, ?3, ?4)",
            params![
                connection_record.identifier.as_bytes().as_slice(),
                connection_record.service_module.as_str(),
                connection_record.name.as_str(),
                connection_record.protected_credential.as_bytes(),
            ],
        )?;
    }
    execute(
        connection,
        "INSERT INTO weavelit_recovery_public_key (singleton, public_key) VALUES (1, ?1)",
        params![state.recovery_public_key().as_str()],
    )?;
    for configuration in state.log_module_configurations() {
        execute(
            connection,
            "INSERT INTO weavelit_log_module_configuration \
             (configuration_id, module, name, enabled) VALUES (?1, ?2, ?3, ?4)",
            params![
                configuration.identifier.as_bytes().as_slice(),
                configuration.module.as_str(),
                configuration.name.as_str(),
                i64::from(configuration.enabled),
            ],
        )?;
        for setting in &configuration.settings {
            execute(
                connection,
                "INSERT INTO weavelit_log_module_setting \
                 (configuration_id, setting_key, setting_value) VALUES (?1, ?2, ?3)",
                params![
                    configuration.identifier.as_bytes().as_slice(),
                    setting.key.as_str(),
                    setting.value.as_str()
                ],
            )?;
        }
    }
    for reference in state.log_configuration_audit_references() {
        execute(
            connection,
            "INSERT INTO weavelit_log_configuration_audit_reference \
             (configuration_id, audit_reference) VALUES (?1, ?2)",
            params![
                reference.configuration().as_bytes().as_slice(),
                reference.audit_reference().to_string()
            ],
        )?;
    }
    for assignment in state.log_assignments() {
        execute(
            connection,
            "INSERT INTO weavelit_log_assignment (log_type, configuration_id) VALUES (?1, ?2)",
            params![
                encode_log_type(assignment.log_type),
                assignment.configuration.as_bytes().as_slice()
            ],
        )?;
    }
    let obligation = state.completion_obligation();
    execute(
        connection,
        "INSERT INTO weavelit_completion_obligation \
         (singleton, record_id, workflow_kind, classification, correlation_identifier, \
          event_time_milliseconds, detail, acknowledged) \
         VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, 0)",
        params![
            obligation.record_identifier().as_bytes().as_slice(),
            encode_workflow(obligation.workflow()),
            obligation.classification().as_str(),
            obligation.correlation_identifier().as_str(),
            obligation.event_time_milliseconds(),
            obligation.detail().as_str(),
        ],
    )
}

pub(super) fn read(
    connection: &Connection,
    public_identity_persistence: &AccountPublicIdentifierPersistence,
    audit_reference_persistence: &AuditReferencePersistence,
) -> Result<(ApplicationState, bool), DatabaseError> {
    let configuration = rows::<ConfigurationRow>(connection, CONFIGURATION_QUERY, three_columns)?
        .into_iter()
        .map(|(component, key, value)| {
            Ok(ConfigurationEntry {
                component: text(component)?,
                key: text(key)?,
                value: text(value)?,
            })
        })
        .collect::<Result<Vec<_>, DatabaseError>>()?;

    let protected_secrets =
        rows::<ProtectedSecretRow>(connection, PROTECTED_SECRET_QUERY, three_columns)?
            .into_iter()
            .map(|(component, key, value)| {
                Ok(ProtectedSecret {
                    component: text(component)?,
                    key: text(key)?,
                    value: protected(value)?,
                })
            })
            .collect::<Result<Vec<_>, DatabaseError>>()?;

    let accounts = rows::<AccountRow>(connection, ACCOUNT_QUERY, five_columns)?
        .into_iter()
        .map(
            |(account_id, username, display_name, active, mfa_required)| {
                Ok(Account {
                    identifier: identifier(&account_id)?,
                    username: text(username)?,
                    display_name: display_name.map(text).transpose()?,
                    active: boolean(active)?,
                    mfa_required: boolean(mfa_required)?,
                })
            },
        )
        .collect::<Result<Vec<_>, DatabaseError>>()?;

    let account_public_identities =
        rows::<AccountPublicIdentityRow>(connection, ACCOUNT_PUBLIC_IDENTITY_QUERY, two_columns)?
            .into_iter()
            .map(|(account_id, public_identifier)| {
                Ok(AccountPublicIdentity::new(
                    identifier(&account_id)?,
                    account_public_identifier(public_identity_persistence, &public_identifier)?,
                ))
            })
            .collect::<Result<Vec<_>, DatabaseError>>()?;

    let account_audit_references =
        rows::<AuditReferenceRow>(connection, ACCOUNT_AUDIT_REFERENCE_QUERY, two_columns)?
            .into_iter()
            .map(|(account_id, value)| {
                Ok(AccountAuditReference::new(
                    identifier(&account_id)?,
                    audit_reference(audit_reference_persistence, &value)?,
                ))
            })
            .collect::<Result<Vec<_>, DatabaseError>>()?;

    let password_verifiers =
        rows::<PasswordVerifierRow>(connection, PASSWORD_VERIFIER_QUERY, two_columns)?
            .into_iter()
            .map(|(account_id, encoded_verifier)| {
                Ok(AccountPasswordVerifier {
                    account: identifier(&account_id)?,
                    verifier: PasswordVerifier::new(encoded_verifier)
                        .map_err(|_| DatabaseError::IntegrityFailure)?,
                })
            })
            .collect::<Result<Vec<_>, DatabaseError>>()?;

    let groups = rows::<GroupRow>(connection, GROUP_QUERY, three_columns)?
        .into_iter()
        .map(|(group_id, name, description)| {
            Ok(Group {
                identifier: identifier(&group_id)?,
                name: text(name)?,
                description: description.map(text).transpose()?,
            })
        })
        .collect::<Result<Vec<_>, DatabaseError>>()?;

    let group_audit_references =
        rows::<AuditReferenceRow>(connection, GROUP_AUDIT_REFERENCE_QUERY, two_columns)?
            .into_iter()
            .map(|(group_id, value)| {
                Ok(GroupAuditReference::new(
                    identifier(&group_id)?,
                    audit_reference(audit_reference_persistence, &value)?,
                ))
            })
            .collect::<Result<Vec<_>, DatabaseError>>()?;

    let group_memberships =
        rows::<GroupMembershipRow>(connection, GROUP_MEMBERSHIP_QUERY, two_columns)?
            .into_iter()
            .map(|(group_id, account_id)| {
                Ok(GroupMembership {
                    group: identifier(&group_id)?,
                    account: identifier(&account_id)?,
                })
            })
            .collect::<Result<Vec<_>, DatabaseError>>()?;

    let group_grants = rows::<GroupGrantRow>(connection, GROUP_GRANT_QUERY, three_columns)?
        .into_iter()
        .map(|(group_id, kind, value)| {
            Ok(GroupGrantRecord {
                group: identifier(&group_id)?,
                grant: decode_grant(&kind, value)?,
            })
        })
        .collect::<Result<Vec<_>, DatabaseError>>()?;

    let mfa_factors = rows::<MfaFactorRow>(connection, MFA_FACTOR_QUERY, four_columns)?
        .into_iter()
        .map(|(factor_id, account_id, module, factor_data)| {
            Ok(MfaFactor {
                identifier: identifier(&factor_id)?,
                account: identifier(&account_id)?,
                module: text(module)?,
                protected_factor_data: protected(factor_data)?,
            })
        })
        .collect::<Result<Vec<_>, DatabaseError>>()?;

    let service_connections =
        rows::<ServiceConnectionRow>(connection, SERVICE_CONNECTION_QUERY, four_columns)?
            .into_iter()
            .map(|(connection_id, service_module, name, credential)| {
                Ok(ServiceConnection {
                    identifier: identifier(&connection_id)?,
                    service_module: text(service_module)?,
                    name: text(name)?,
                    protected_credential: protected(credential)?,
                })
            })
            .collect::<Result<Vec<_>, DatabaseError>>()?;

    let recovery_keys = rows::<String>(connection, RECOVERY_PUBLIC_KEY_QUERY, |row| row.get(0))?;
    let [recovery_public_key] = recovery_keys.as_slice() else {
        return Err(DatabaseError::IntegrityFailure);
    };
    let recovery_public_key = RecoveryPublicKey::new(recovery_public_key.as_str())
        .map_err(|_| DatabaseError::IntegrityFailure)?;

    let settings =
        rows::<LogModuleSettingRow>(connection, LOG_MODULE_SETTING_QUERY, three_columns)?;
    let log_module_configurations = rows::<LogModuleConfigurationRow>(
        connection,
        LOG_MODULE_CONFIGURATION_QUERY,
        four_columns,
    )?
    .into_iter()
    .map(|(configuration_id, module, name, enabled)| {
        let owned_settings = settings
            .iter()
            .filter(|(owner, _, _)| owner == &configuration_id)
            .map(|(_, key, value)| {
                Ok(LogModuleSetting {
                    key: text(key.clone())?,
                    value: text(value.clone())?,
                })
            })
            .collect::<Result<Vec<_>, DatabaseError>>()?;
        Ok(LogModuleConfiguration {
            identifier: identifier(&configuration_id)?,
            module: text(module)?,
            name: text(name)?,
            enabled: boolean(enabled)?,
            settings: owned_settings,
        })
    })
    .collect::<Result<Vec<_>, DatabaseError>>()?;

    let log_configuration_audit_references = rows::<AuditReferenceRow>(
        connection,
        LOG_CONFIGURATION_AUDIT_REFERENCE_QUERY,
        two_columns,
    )?
    .into_iter()
    .map(|(configuration_id, value)| {
        Ok(LogConfigurationAuditReference::new(
            identifier(&configuration_id)?,
            audit_reference(audit_reference_persistence, &value)?,
        ))
    })
    .collect::<Result<Vec<_>, DatabaseError>>()?;

    let log_assignments = rows::<LogAssignmentRow>(connection, LOG_ASSIGNMENT_QUERY, two_columns)?
        .into_iter()
        .map(|(log_type, configuration_id)| {
            Ok(LogAssignment {
                log_type: decode_log_type(&log_type)?,
                configuration: identifier(&configuration_id)?,
            })
        })
        .collect::<Result<Vec<_>, DatabaseError>>()?;

    let obligations =
        rows::<CompletionObligationRow>(connection, COMPLETION_OBLIGATION_QUERY, |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
            ))
        })?;
    let [obligation] = obligations.as_slice() else {
        return Err(DatabaseError::IntegrityFailure);
    };
    let (record_id, workflow, classification, correlation, event_time, detail, acknowledged) =
        obligation;
    let completion_obligation = CompletionObligation::new(
        identifier(record_id)?,
        decode_workflow(workflow)?,
        text(classification.clone())?,
        text(correlation.clone())?,
        *event_time,
        text(detail.clone())?,
    )
    .map_err(|_| DatabaseError::IntegrityFailure)?;

    let state = ApplicationState::new(ApplicationStateInput {
        configuration,
        protected_secrets,
        accounts,
        account_public_identities,
        account_audit_references,
        password_verifiers,
        groups,
        group_audit_references,
        group_memberships,
        group_grants,
        mfa_factors,
        service_connections,
        recovery_public_key,
        log_module_configurations,
        log_configuration_audit_references,
        log_assignments,
        completion_obligation,
    })
    .map_err(|_| DatabaseError::IntegrityFailure)?;

    Ok((state, boolean(*acknowledged)?))
}

pub(super) fn encode_workflow(workflow: WorkflowKind) -> &'static str {
    match workflow {
        WorkflowKind::Init => "init",
        WorkflowKind::Restore => "restore",
    }
}

fn decode_workflow(workflow: &str) -> Result<WorkflowKind, DatabaseError> {
    match workflow {
        "init" => Ok(WorkflowKind::Init),
        "restore" => Ok(WorkflowKind::Restore),
        _ => Err(DatabaseError::IntegrityFailure),
    }
}

fn encode_grant(grant: &GroupGrant) -> (&'static str, &str) {
    match grant {
        GroupGrant::ClientModule(name) => ("client_module", name.as_str()),
        GroupGrant::ServiceModule(name) => ("service_module", name.as_str()),
        GroupGrant::Operation(name) => ("operation", name.as_str()),
        GroupGrant::ServerAdministration => ("server_administration", ""),
    }
}

fn decode_grant(kind: &str, value: String) -> Result<GroupGrant, DatabaseError> {
    match kind {
        "client_module" => Ok(GroupGrant::ClientModule(text(value)?)),
        "service_module" => Ok(GroupGrant::ServiceModule(text(value)?)),
        "operation" => Ok(GroupGrant::Operation(text(value)?)),
        "server_administration" if value.is_empty() => Ok(GroupGrant::ServerAdministration),
        _ => Err(DatabaseError::IntegrityFailure),
    }
}

fn encode_log_type(log_type: LogType) -> &'static str {
    match log_type {
        LogType::System => "system",
        LogType::Audit => "audit",
    }
}

fn decode_log_type(log_type: &str) -> Result<LogType, DatabaseError> {
    match log_type {
        "system" => Ok(LogType::System),
        "audit" => Ok(LogType::Audit),
        _ => Err(DatabaseError::IntegrityFailure),
    }
}

fn execute(
    connection: &Connection,
    sql: &str,
    parameters: &[&dyn rusqlite::ToSql],
) -> Result<(), DatabaseError> {
    connection
        .execute(sql, parameters)
        .map(|_| ())
        .map_err(|error| map_sqlite_error(error, ErrorContext::State))
}

fn rows<R>(
    connection: &Connection,
    sql: &str,
    decode: impl FnMut(&Row<'_>) -> rusqlite::Result<R>,
) -> Result<Vec<R>, DatabaseError> {
    let mut statement = connection
        .prepare(sql)
        .map_err(|error| map_sqlite_error(error, ErrorContext::State))?;
    let decoded = statement
        .query_map([], decode)
        .map_err(|error| map_sqlite_error(error, ErrorContext::State))?;

    decoded
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| map_sqlite_error(error, ErrorContext::State))
}

fn parameterized_rows<R>(
    connection: &Connection,
    sql: &str,
    parameters: &[&dyn rusqlite::ToSql],
    decode: impl FnMut(&Row<'_>) -> rusqlite::Result<R>,
) -> Result<Vec<R>, DatabaseError> {
    let mut statement = connection
        .prepare(sql)
        .map_err(|error| map_sqlite_error(error, ErrorContext::State))?;
    let decoded = statement
        .query_map(parameters, decode)
        .map_err(|error| map_sqlite_error(error, ErrorContext::State))?;

    decoded
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| map_sqlite_error(error, ErrorContext::State))
}

fn two_columns<A: rusqlite::types::FromSql, B: rusqlite::types::FromSql>(
    row: &Row<'_>,
) -> rusqlite::Result<(A, B)> {
    Ok((row.get(0)?, row.get(1)?))
}

fn three_columns<
    A: rusqlite::types::FromSql,
    B: rusqlite::types::FromSql,
    C: rusqlite::types::FromSql,
>(
    row: &Row<'_>,
) -> rusqlite::Result<(A, B, C)> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
}

fn four_columns<
    A: rusqlite::types::FromSql,
    B: rusqlite::types::FromSql,
    C: rusqlite::types::FromSql,
    D: rusqlite::types::FromSql,
>(
    row: &Row<'_>,
) -> rusqlite::Result<(A, B, C, D)> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
}

fn five_columns<
    A: rusqlite::types::FromSql,
    B: rusqlite::types::FromSql,
    C: rusqlite::types::FromSql,
    D: rusqlite::types::FromSql,
    E: rusqlite::types::FromSql,
>(
    row: &Row<'_>,
) -> rusqlite::Result<(A, B, C, D, E)> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
    ))
}

fn identifier(bytes: &[u8]) -> Result<StateIdentifier, DatabaseError> {
    let bytes: [u8; STATE_IDENTIFIER_LENGTH] = bytes
        .try_into()
        .map_err(|_| DatabaseError::IntegrityFailure)?;
    StateIdentifier::from_bytes(bytes).map_err(|_| DatabaseError::IntegrityFailure)
}

fn audit_reference(
    persistence: &AuditReferencePersistence,
    value: &str,
) -> Result<AuditReferenceIdentifier, DatabaseError> {
    persistence
        .decode(value)
        .map_err(|_| DatabaseError::IntegrityFailure)
}

fn account_public_identifier(
    persistence: &AccountPublicIdentifierPersistence,
    value: &[u8],
) -> Result<AccountPublicIdentifier, DatabaseError> {
    let bytes: [u8; ACCOUNT_PUBLIC_IDENTIFIER_LENGTH] = value
        .try_into()
        .map_err(|_| DatabaseError::IntegrityFailure)?;
    persistence
        .decode(bytes)
        .map_err(|_| DatabaseError::IntegrityFailure)
}

fn text<const MAX_BYTES: usize>(value: String) -> Result<BoundedText<MAX_BYTES>, DatabaseError> {
    BoundedText::new(value).map_err(|_| DatabaseError::IntegrityFailure)
}

fn protected(value: Vec<u8>) -> Result<ProtectedValue, DatabaseError> {
    ProtectedValue::new(value).map_err(|_| DatabaseError::IntegrityFailure)
}

fn boolean(value: i64) -> Result<bool, DatabaseError> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(DatabaseError::IntegrityFailure),
    }
}
