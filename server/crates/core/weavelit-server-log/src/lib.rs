#![forbid(unsafe_code)]

//! Typed Server-owned dispatch contract for complete, pre-redacted log records.

use std::{
    error::Error as StdError,
    fmt,
    path::{Path, PathBuf},
};

use weavelit_server_log_authority::ServerLogAuthority;

const MAX_LOG_MODULES: usize = 64;
const MAX_IDENTIFIER_LENGTH: usize = 64;
const RECORD_ID_LENGTH: usize = 16;

/// Most non-secret settings one configured destination may receive.
pub const MAX_DESTINATION_SETTINGS: usize = 64;

/// Maximum UTF-8 bytes in one destination setting key.
pub const MAX_DESTINATION_SETTING_KEY_BYTES: usize = 256;

/// Maximum UTF-8 bytes in one destination setting value.
pub const MAX_DESTINATION_SETTING_VALUE_BYTES: usize = 4 * 1024;
const MAX_CORRELATION_ID_BYTES: usize = 64;
const MAX_SYSTEM_CLASSIFICATION_BYTES: usize = 128;
const MAX_SYSTEM_DETAIL_BYTES: usize = 4 * 1024;
const MAX_AUDIT_CLASSIFICATION_BYTES: usize = 128;
const MAX_AUDIT_PRINCIPAL_BYTES: usize = 256;
const MAX_AUDIT_RESPONSIBLE_OWNER_BYTES: usize = 256;
const MAX_AUDIT_ACTION_BYTES: usize = 128;
const MAX_AUDIT_TARGET_BYTES: usize = 1024;
const MAX_AUDIT_DETAIL_BYTES: usize = 4 * 1024;
const MAX_RECORD_PAYLOAD_BYTES: usize = 8 * 1024;

/// Stable type of a complete record assigned to a Log Module.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum LogRecordType {
    /// Operational diagnostic record constructed and redacted by Observability.
    System,
    /// Accountability record constructed and redacted by Audit.
    Audit,
}

/// Validated capabilities declared by a compiled-in Log Module.
#[derive(Clone, Eq, PartialEq)]
pub struct LogCapabilities(Box<[LogRecordType]>);

impl LogCapabilities {
    /// Validates declared record-type capabilities.
    pub fn new(mut record_types: Vec<LogRecordType>) -> Result<Self, LogCatalogError> {
        if record_types.is_empty() {
            return Err(LogCatalogError::EmptyCapabilities);
        }
        record_types.sort_unstable();
        if record_types.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(LogCatalogError::DuplicateCapability);
        }
        Ok(Self(record_types.into_boxed_slice()))
    }

    /// Returns whether this declaration accepts the record type.
    pub fn supports(&self, record_type: LogRecordType) -> bool {
        self.0.binary_search(&record_type).is_ok()
    }

    /// Returns declared types in canonical order.
    pub fn record_types(&self) -> &[LogRecordType] {
        &self.0
    }
}

impl fmt::Debug for LogCapabilities {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("LogCapabilities")
            .field(&self.0)
            .finish()
    }
}

/// Stable opaque identifier for one Server-generated immutable record.
#[derive(Eq, Hash, PartialEq)]
pub struct RecordId([u8; RECORD_ID_LENGTH]);

impl fmt::Debug for RecordId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RecordId(REDACTED)")
    }
}

impl RecordId {
    fn duplicate(&self) -> Self {
        Self(self.0)
    }

    /// Returns the stable opaque bytes issued by the Server for persistence matching.
    pub const fn as_bytes(&self) -> &[u8; RECORD_ID_LENGTH] {
        &self.0
    }
}

/// Opaque capability linking a terminal Audit record to its precise Attempt.
#[derive(Eq, PartialEq)]
pub struct AttemptRecordId {
    record_id: [u8; RECORD_ID_LENGTH],
    event_time: EventTime,
    correlation_id: CorrelationId,
}

impl fmt::Debug for AttemptRecordId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AttemptRecordId(REDACTED)")
    }
}

impl AttemptRecordId {
    /// Returns the opaque Attempt record identifier for persistence.
    pub const fn as_bytes(&self) -> &[u8; RECORD_ID_LENGTH] {
        &self.record_id
    }
}

/// Server-internal issuer for opaque record identifiers.
pub struct TrustedRecordIssuer {
    _private: (),
}

impl TrustedRecordIssuer {
    /// Creates the Server-owned issuer used by Audit and Observability.
    #[allow(dead_code)]
    pub(crate) const fn new() -> Self {
        Self { _private: () }
    }

    /// Creates the issuer for a holder of Server-owned logging authority.
    #[must_use]
    pub const fn from_server_authority(_authority: &ServerLogAuthority) -> Self {
        Self::new()
    }

    /// Issues an identifier from Server-generated entropy.
    pub fn issue(&self, entropy: [u8; RECORD_ID_LENGTH]) -> Result<RecordId, RecordError> {
        if entropy == [0; RECORD_ID_LENGTH] {
            return Err(RecordError::InvalidRecordIdentifier);
        }
        Ok(RecordId(entropy))
    }
}

/// Immutable event time supplied by the owning Server record constructor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventTime(u64);

impl EventTime {
    /// Creates a UTC Unix timestamp in milliseconds.
    pub const fn from_unix_milliseconds(value: u64) -> Self {
        Self(value)
    }

    /// Returns the UTC Unix timestamp in milliseconds.
    pub const fn unix_milliseconds(self) -> u64 {
        self.0
    }
}

/// Opaque correlation identifier already classified by the owning Server component.
#[derive(Clone, Eq, PartialEq)]
pub struct CorrelationId(Box<str>);

impl CorrelationId {
    /// Creates a non-empty bounded correlation identifier.
    pub fn new(value: impl Into<Box<str>>) -> Result<Self, RecordError> {
        let value = value.into();
        if !is_nonempty_within_bytes(&value, MAX_CORRELATION_ID_BYTES) {
            return Err(RecordError::InvalidCorrelationIdentifier);
        }
        Ok(Self(value))
    }

    fn is_valid(&self) -> bool {
        is_nonempty_within_bytes(&self.0, MAX_CORRELATION_ID_BYTES)
    }
}

impl fmt::Debug for CorrelationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CorrelationId(REDACTED)")
    }
}

impl CorrelationId {
    /// Returns the pre-classified correlation identifier for persistence.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Server-owned result classification included with every complete record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogResult {
    /// The recorded activity completed successfully.
    Success,
    /// The recorded activity failed with a redacted classification in its record body.
    Failure,
}

/// Closed phase, Attempt linkage, and outcome contract for an Audit Log record.
#[derive(Debug, Eq, PartialEq)]
pub enum AuditRecordPhase {
    /// Accepted intent recorded before a consequential mutation.
    Attempt,
    /// Authoritative outcome recorded after the mutation decision.
    #[non_exhaustive]
    Completion {
        attempt_record_id: AttemptRecordId,
        result: LogResult,
    },
    /// Authoritative correction to prior Audit evidence.
    #[non_exhaustive]
    Correction {
        attempt_record_id: AttemptRecordId,
        result: LogResult,
    },
}

impl AuditRecordPhase {
    /// Returns the canonical phase literal persisted by a destination.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Attempt => "attempt",
            Self::Completion { .. } => "completion",
            Self::Correction { .. } => "correction",
        }
    }

    /// Returns an outcome only for completion and correction records.
    pub const fn result(&self) -> Option<LogResult> {
        match self {
            Self::Attempt => None,
            Self::Completion { result, .. } | Self::Correction { result, .. } => Some(*result),
        }
    }

    /// Returns the precise Attempt link required by terminal records.
    pub const fn attempt_record_id(&self) -> Option<&AttemptRecordId> {
        match self {
            Self::Attempt => None,
            Self::Completion {
                attempt_record_id, ..
            }
            | Self::Correction {
                attempt_record_id, ..
            } => Some(attempt_record_id),
        }
    }
}

/// Closed catalog of System Log classifications selected by Observability.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SystemLogClassification {
    LifecycleStartup,
    LifecycleInit,
    LifecycleRestore,
    OperationalState,
    ConfigurationChange,
    AuthenticationFailure,
    AuthorizationDenial,
    DependencyFailure,
    DependencyAuditLogUnavailable,
    ProviderFailure,
    InternalError,
}

impl SystemLogClassification {
    /// Returns the canonical literal persisted by a destination.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LifecycleStartup => "lifecycle.startup",
            Self::LifecycleInit => "lifecycle.init",
            Self::LifecycleRestore => "lifecycle.restore",
            Self::OperationalState => "operational.state",
            Self::ConfigurationChange => "configuration.change",
            Self::AuthenticationFailure => "authentication.failure",
            Self::AuthorizationDenial => "authorization.denial",
            Self::DependencyFailure => "dependency.failure",
            Self::DependencyAuditLogUnavailable => "dependency.audit-log-unavailable",
            Self::ProviderFailure => "provider.failure",
            Self::InternalError => "internal.error",
        }
    }
}

/// Closed catalog of Audit Log classifications selected by Audit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuditLogClassification {
    LifecycleBackupCreated,
    AuthenticationUserCreated,
    AuthenticationUserDisabled,
    AuthenticationPasswordChanged,
    AuthenticationPasswordResetStarted,
    AuthenticationMfaEnrolled,
    AuthenticationMfaReset,
    AuthenticationMfaRequirementChanged,
    AuthenticationMfaModuleEnablementChanged,
    AuthenticationSessionRevoked,
    AuthorizationGroupCreated,
    AuthorizationGroupMembershipChanged,
    AuthorizationGroupGrantChanged,
    AuthorizationGroupGrantRemovalDenied,
    AuthorizationAutomationScopeChanged,
    DependencyLogModuleConfigurationChanged,
    DependencyServiceConnectionChanged,
    ProviderOperationStarted,
    ProviderOperationCompleted,
    InternalServerConfigurationChanged,
    InternalUserStatusChanged,
    InternalLogPolicyChanged,
}

impl AuditLogClassification {
    /// Returns the canonical literal persisted by a destination.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LifecycleBackupCreated => "lifecycle.backup.created",
            Self::AuthenticationUserCreated => "authentication.user.created",
            Self::AuthenticationUserDisabled => "authentication.user.disabled",
            Self::AuthenticationPasswordChanged => "authentication.password.changed",
            Self::AuthenticationPasswordResetStarted => "authentication.password-reset.started",
            Self::AuthenticationMfaEnrolled => "authentication.mfa.enrolled",
            Self::AuthenticationMfaReset => "authentication.mfa.reset",
            Self::AuthenticationMfaRequirementChanged => "authentication.mfa-requirement.changed",
            Self::AuthenticationMfaModuleEnablementChanged => {
                "authentication.mfa-module-enablement.changed"
            }
            Self::AuthenticationSessionRevoked => "authentication.session.revoked",
            Self::AuthorizationGroupCreated => "authorization.group.created",
            Self::AuthorizationGroupMembershipChanged => "authorization.group-membership.changed",
            Self::AuthorizationGroupGrantChanged => "authorization.group-grant.changed",
            Self::AuthorizationGroupGrantRemovalDenied => {
                "authorization.group-grant.removal-denied"
            }
            Self::AuthorizationAutomationScopeChanged => "authorization.automation-scope.changed",
            Self::DependencyLogModuleConfigurationChanged => {
                "dependency.log-module-configuration.changed"
            }
            Self::DependencyServiceConnectionChanged => "dependency.service-connection.changed",
            Self::ProviderOperationStarted => "provider.operation.started",
            Self::ProviderOperationCompleted => "provider.operation.completed",
            Self::InternalServerConfigurationChanged => "internal.server-configuration.changed",
            Self::InternalUserStatusChanged => "internal.user-status.changed",
            Self::InternalLogPolicyChanged => "internal.log-policy.changed",
        }
    }
}

/// The accountable kind of an Audit principal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuditPrincipalType {
    Human,
    Automation,
}

impl AuditPrincipalType {
    /// Returns the canonical literal persisted by a destination.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Human => "human",
            Self::Automation => "automation",
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
enum AuditPrincipalKind {
    Human,
    Automation { responsible_owner: Box<str> },
}

/// Typed accountable principal for an Audit Log record.
#[derive(Clone, Eq, PartialEq)]
pub struct AuditPrincipal {
    principal: Box<str>,
    kind: AuditPrincipalKind,
}

impl AuditPrincipal {
    /// Creates a bounded human principal, which intentionally has no Responsible Owner.
    pub fn human(principal: impl Into<Box<str>>) -> Result<Self, RecordError> {
        let principal = principal.into();
        if !is_nonempty_within_bytes(&principal, MAX_AUDIT_PRINCIPAL_BYTES) {
            return Err(RecordError::InvalidAuditPrincipal);
        }
        Ok(Self {
            principal,
            kind: AuditPrincipalKind::Human,
        })
    }

    /// Creates a bounded automation principal with its required Responsible Owner.
    pub fn automation(
        principal: impl Into<Box<str>>,
        responsible_owner: impl Into<Box<str>>,
    ) -> Result<Self, RecordError> {
        let principal = principal.into();
        let responsible_owner = responsible_owner.into();
        if !is_nonempty_within_bytes(&principal, MAX_AUDIT_PRINCIPAL_BYTES)
            || !is_nonempty_within_bytes(&responsible_owner, MAX_AUDIT_RESPONSIBLE_OWNER_BYTES)
        {
            return Err(RecordError::InvalidAuditPrincipal);
        }
        Ok(Self {
            principal,
            kind: AuditPrincipalKind::Automation { responsible_owner },
        })
    }

    fn is_valid(&self) -> bool {
        is_nonempty_within_bytes(&self.principal, MAX_AUDIT_PRINCIPAL_BYTES)
            && match &self.kind {
                AuditPrincipalKind::Human => true,
                AuditPrincipalKind::Automation { responsible_owner } => {
                    is_nonempty_within_bytes(responsible_owner, MAX_AUDIT_RESPONSIBLE_OWNER_BYTES)
                }
            }
    }

    /// Returns the bounded accountable principal identifier.
    pub fn as_str(&self) -> &str {
        &self.principal
    }

    /// Returns whether this principal is human or automation.
    pub const fn principal_type(&self) -> AuditPrincipalType {
        match self.kind {
            AuditPrincipalKind::Human => AuditPrincipalType::Human,
            AuditPrincipalKind::Automation { .. } => AuditPrincipalType::Automation,
        }
    }

    /// Returns the required Responsible Owner for automation principals only.
    pub fn responsible_owner(&self) -> Option<&str> {
        match &self.kind {
            AuditPrincipalKind::Human => None,
            AuditPrincipalKind::Automation { responsible_owner } => Some(responsible_owner),
        }
    }
}

impl fmt::Debug for AuditPrincipal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AuditPrincipal(REDACTED)")
    }
}

/// Typed pre-redacted System Log body owned by Observability.
#[derive(Clone, Eq, PartialEq)]
pub struct SystemLogBody {
    classification: SystemLogClassification,
    detail: Box<str>,
}

impl SystemLogBody {
    /// Creates a complete pre-redacted System Log body.
    pub fn new(
        classification: SystemLogClassification,
        detail: impl Into<Box<str>>,
    ) -> Result<Self, RecordError> {
        let detail = detail.into();
        if !is_nonempty_within_bytes(&detail, MAX_SYSTEM_DETAIL_BYTES) {
            return Err(RecordError::InvalidSystemLogBody);
        }
        Ok(Self {
            classification,
            detail,
        })
    }

    fn is_valid(&self) -> bool {
        is_nonempty_within_bytes(
            self.classification.as_str(),
            MAX_SYSTEM_CLASSIFICATION_BYTES,
        ) && is_nonempty_within_bytes(&self.detail, MAX_SYSTEM_DETAIL_BYTES)
    }
}

impl fmt::Debug for SystemLogBody {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SystemLogBody(REDACTED)")
    }
}

impl SystemLogBody {
    /// Returns the pre-redacted operational classification.
    pub fn classification(&self) -> &str {
        self.classification.as_str()
    }

    /// Returns the pre-redacted operational detail.
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

/// Typed pre-redacted Audit Log body owned by Audit.
#[derive(Clone, Eq, PartialEq)]
pub struct AuditLogBody {
    classification: AuditLogClassification,
    principal: AuditPrincipal,
    action: Box<str>,
    target: Box<str>,
    detail: Box<str>,
}

impl AuditLogBody {
    /// Creates a complete pre-redacted Audit Log body.
    pub fn new(
        classification: AuditLogClassification,
        principal: AuditPrincipal,
        action: impl Into<Box<str>>,
        target: impl Into<Box<str>>,
        detail: impl Into<Box<str>>,
    ) -> Result<Self, RecordError> {
        let action = action.into();
        let target = target.into();
        let detail = detail.into();
        if !is_nonempty_within_bytes(&action, MAX_AUDIT_ACTION_BYTES)
            || !is_nonempty_within_bytes(&target, MAX_AUDIT_TARGET_BYTES)
            || !is_nonempty_within_bytes(&detail, MAX_AUDIT_DETAIL_BYTES)
        {
            return Err(RecordError::InvalidAuditLogBody);
        }
        Ok(Self {
            classification,
            principal,
            action,
            target,
            detail,
        })
    }

    fn is_valid(&self) -> bool {
        is_nonempty_within_bytes(self.classification.as_str(), MAX_AUDIT_CLASSIFICATION_BYTES)
            && self.principal.is_valid()
            && is_nonempty_within_bytes(&self.action, MAX_AUDIT_ACTION_BYTES)
            && is_nonempty_within_bytes(&self.target, MAX_AUDIT_TARGET_BYTES)
            && is_nonempty_within_bytes(&self.detail, MAX_AUDIT_DETAIL_BYTES)
    }
}

impl fmt::Debug for AuditLogBody {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AuditLogBody(REDACTED)")
    }
}

impl AuditLogBody {
    /// Returns the pre-redacted accountability classification.
    pub fn classification(&self) -> &str {
        self.classification.as_str()
    }

    /// Returns the pre-redacted accountable principal.
    pub fn principal(&self) -> &str {
        self.principal.as_str()
    }

    /// Returns the accountable principal type.
    pub const fn principal_type(&self) -> AuditPrincipalType {
        self.principal.principal_type()
    }

    /// Returns the required Responsible Owner for automation principals only.
    pub fn responsible_owner(&self) -> Option<&str> {
        self.principal.responsible_owner()
    }

    /// Returns the pre-redacted accountable action.
    pub fn action(&self) -> &str {
        &self.action
    }

    /// Returns the pre-redacted accountable target.
    pub fn target(&self) -> &str {
        &self.target
    }

    /// Returns the pre-redacted accountability detail.
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

/// Complete immutable record dispatched to a Log Module.
#[derive(Eq, PartialEq)]
pub enum CompleteLogRecord {
    /// A System Log record constructed by Observability.
    System {
        record_id: RecordId,
        event_time: EventTime,
        result: LogResult,
        correlation_id: CorrelationId,
        body: SystemLogBody,
    },
    /// An Audit Log record constructed by Audit.
    #[non_exhaustive]
    Audit {
        record_id: RecordId,
        event_time: EventTime,
        phase: AuditRecordPhase,
        correlation_id: CorrelationId,
        body: AuditLogBody,
    },
}

impl CompleteLogRecord {
    /// Constructs a complete pre-redacted System Log record.
    pub fn system(
        record_id: RecordId,
        event_time: EventTime,
        result: LogResult,
        correlation_id: CorrelationId,
        body: SystemLogBody,
    ) -> Result<Self, RecordError> {
        if record_payload_bytes(&correlation_id, &[body.classification(), body.detail()])
            > MAX_RECORD_PAYLOAD_BYTES
        {
            return Err(RecordError::RecordSizeLimitExceeded);
        }
        if !correlation_id.is_valid() {
            return Err(RecordError::InvalidCorrelationIdentifier);
        }
        if !body.is_valid() {
            return Err(RecordError::InvalidSystemLogBody);
        }
        Ok(Self::System {
            record_id,
            event_time,
            result,
            correlation_id,
            body,
        })
    }

    /// Constructs a complete pre-redacted Audit Attempt record.
    pub fn audit_attempt(
        record_id: RecordId,
        event_time: EventTime,
        correlation_id: CorrelationId,
        body: AuditLogBody,
    ) -> Result<Self, RecordError> {
        Self::audit(
            record_id,
            event_time,
            AuditRecordPhase::Attempt,
            correlation_id,
            body,
        )
    }

    /// Constructs a complete pre-redacted Audit Completion linked to its Attempt.
    pub fn audit_completion(
        record_id: RecordId,
        event_time: EventTime,
        attempt_record_id: AttemptRecordId,
        result: LogResult,
        correlation_id: CorrelationId,
        body: AuditLogBody,
    ) -> Result<Self, RecordError> {
        Self::validate_attempt_link(&record_id, event_time, &attempt_record_id, &correlation_id)?;
        Self::audit(
            record_id,
            event_time,
            AuditRecordPhase::Completion {
                attempt_record_id,
                result,
            },
            correlation_id,
            body,
        )
    }

    /// Constructs a complete pre-redacted Audit Correction linked to its Attempt.
    pub fn audit_correction(
        record_id: RecordId,
        event_time: EventTime,
        attempt_record_id: AttemptRecordId,
        result: LogResult,
        correlation_id: CorrelationId,
        body: AuditLogBody,
    ) -> Result<Self, RecordError> {
        Self::validate_attempt_link(&record_id, event_time, &attempt_record_id, &correlation_id)?;
        Self::audit(
            record_id,
            event_time,
            AuditRecordPhase::Correction {
                attempt_record_id,
                result,
            },
            correlation_id,
            body,
        )
    }

    fn validate_attempt_link(
        record_id: &RecordId,
        event_time: EventTime,
        attempt_record_id: &AttemptRecordId,
        correlation_id: &CorrelationId,
    ) -> Result<(), RecordError> {
        if attempt_record_id.correlation_id != *correlation_id {
            return Err(RecordError::MismatchedAttemptCorrelation);
        }
        if attempt_record_id.record_id == *record_id.as_bytes()
            || attempt_record_id.event_time.unix_milliseconds() > event_time.unix_milliseconds()
        {
            return Err(RecordError::InvalidAttemptLink);
        }
        Ok(())
    }

    fn audit(
        record_id: RecordId,
        event_time: EventTime,
        phase: AuditRecordPhase,
        correlation_id: CorrelationId,
        body: AuditLogBody,
    ) -> Result<Self, RecordError> {
        if record_payload_bytes(
            &correlation_id,
            &[
                body.classification(),
                body.principal(),
                body.principal_type().as_str(),
                body.responsible_owner().unwrap_or_default(),
                body.action(),
                body.target(),
                body.detail(),
            ],
        ) > MAX_RECORD_PAYLOAD_BYTES
        {
            return Err(RecordError::RecordSizeLimitExceeded);
        }
        if !correlation_id.is_valid() {
            return Err(RecordError::InvalidCorrelationIdentifier);
        }
        if !body.is_valid() {
            return Err(RecordError::InvalidAuditLogBody);
        }
        Ok(Self::Audit {
            record_id,
            event_time,
            phase,
            correlation_id,
            body,
        })
    }

    /// Mints the typed linkage capability only for an Audit Attempt.
    pub fn attempt_record_id(&self) -> Option<AttemptRecordId> {
        match self {
            Self::Audit {
                record_id,
                event_time,
                phase: AuditRecordPhase::Attempt,
                correlation_id,
                ..
            } => Some(AttemptRecordId {
                record_id: *record_id.as_bytes(),
                event_time: *event_time,
                correlation_id: correlation_id.clone(),
            }),
            Self::System { .. }
            | Self::Audit {
                phase: AuditRecordPhase::Completion { .. } | AuditRecordPhase::Correction { .. },
                ..
            } => None,
        }
    }

    /// Returns the immutable opaque record identifier.
    pub const fn record_id(&self) -> &RecordId {
        match self {
            Self::System { record_id, .. } | Self::Audit { record_id, .. } => record_id,
        }
    }

    /// Returns the record type required from its destination.
    pub const fn record_type(&self) -> LogRecordType {
        match self {
            Self::System { .. } => LogRecordType::System,
            Self::Audit { .. } => LogRecordType::Audit,
        }
    }

    /// Returns a complete read-only typed view for durable destination persistence.
    pub const fn persistence_view(&self) -> LogRecordPersistenceView<'_> {
        match self {
            Self::System {
                record_id,
                event_time,
                result,
                correlation_id,
                body,
            } => LogRecordPersistenceView::System(SystemLogPersistenceView {
                record_id,
                event_time: *event_time,
                result: *result,
                correlation_id,
                body,
            }),
            Self::Audit {
                record_id,
                event_time,
                phase,
                correlation_id,
                body,
            } => LogRecordPersistenceView::Audit(AuditLogPersistenceView {
                record_id,
                event_time: *event_time,
                phase,
                correlation_id,
                body,
            }),
        }
    }
}

impl fmt::Debug for CompleteLogRecord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CompleteLogRecord(REDACTED)")
    }
}

/// Read-only typed persistence projection of a complete immutable record.
pub enum LogRecordPersistenceView<'a> {
    /// A System Log record constructed by Observability.
    System(SystemLogPersistenceView<'a>),
    /// An Audit Log record constructed by Audit.
    Audit(AuditLogPersistenceView<'a>),
}

/// Read-only persistence projection of a complete System Log record.
pub struct SystemLogPersistenceView<'a> {
    record_id: &'a RecordId,
    event_time: EventTime,
    result: LogResult,
    correlation_id: &'a CorrelationId,
    body: &'a SystemLogBody,
}

impl SystemLogPersistenceView<'_> {
    /// Returns the opaque Server-issued identifier.
    pub const fn record_id(&self) -> &RecordId {
        self.record_id
    }

    /// Returns the immutable event time.
    pub const fn event_time(&self) -> EventTime {
        self.event_time
    }

    /// Returns the immutable result classification.
    pub const fn result(&self) -> LogResult {
        self.result
    }

    /// Returns the immutable correlation identifier.
    pub const fn correlation_id(&self) -> &CorrelationId {
        self.correlation_id
    }

    /// Returns the complete pre-redacted System Log body.
    pub const fn body(&self) -> &SystemLogBody {
        self.body
    }
}

/// Read-only persistence projection of a complete Audit Log record.
pub struct AuditLogPersistenceView<'a> {
    record_id: &'a RecordId,
    event_time: EventTime,
    phase: &'a AuditRecordPhase,
    correlation_id: &'a CorrelationId,
    body: &'a AuditLogBody,
}

impl AuditLogPersistenceView<'_> {
    /// Returns the opaque Server-issued identifier.
    pub const fn record_id(&self) -> &RecordId {
        self.record_id
    }

    /// Returns the immutable event time.
    pub const fn event_time(&self) -> EventTime {
        self.event_time
    }

    /// Returns the immutable Audit phase, result, and terminal Attempt link.
    pub const fn phase(&self) -> &AuditRecordPhase {
        self.phase
    }

    /// Returns the immutable correlation identifier.
    pub const fn correlation_id(&self) -> &CorrelationId {
        self.correlation_id
    }

    /// Returns the complete pre-redacted Audit Log body.
    pub const fn body(&self) -> &AuditLogBody {
        self.body
    }
}

/// Stable validated identifier for one compiled-in Log Module.
#[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
pub struct LogModuleIdentifier(Box<str>);

impl LogModuleIdentifier {
    /// Validates a lowercase kebab-case module identifier.
    pub fn new(value: impl Into<Box<str>>) -> Result<Self, LogCatalogError> {
        let value = value.into();
        if !is_valid_identifier(&value) {
            return Err(LogCatalogError::InvalidModuleIdentifier);
        }
        Ok(Self(value))
    }

    /// Returns the canonical identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for LogModuleIdentifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("LogModuleIdentifier(REDACTED)")
    }
}

/// The non-secret configuration one destination is opened against.
///
/// A destination is configured by the deployment's committed Log Module
/// configuration rather than by anything it reads itself, so this is the only
/// way settings reach a factory. Keys are unique and every key and value is
/// bounded before a module sees them, so a module cannot be handed an
/// unbounded or ambiguous configuration.
///
/// Secret settings are deliberately absent: they are sealed application state
/// and are never carried through this contract.
#[derive(Clone, Default, Eq, PartialEq)]
pub struct DestinationSettings(Box<[(Box<str>, Box<str>)]>);

impl DestinationSettings {
    /// Validates and orders the non-secret settings of one configuration.
    ///
    /// # Errors
    ///
    /// Returns [`LogConfigurationError::SettingsInvalid`] when the collection
    /// exceeds [`MAX_DESTINATION_SETTINGS`], a key or value is empty or past
    /// its bound, or two entries share a key.
    pub fn new(settings: Vec<(String, String)>) -> Result<Self, LogConfigurationError> {
        if settings.len() > MAX_DESTINATION_SETTINGS {
            return Err(LogConfigurationError::SettingsInvalid);
        }
        let mut entries: Vec<(Box<str>, Box<str>)> = Vec::with_capacity(settings.len());
        for (key, value) in settings {
            if !is_nonempty_within_bytes(&key, MAX_DESTINATION_SETTING_KEY_BYTES)
                || !is_nonempty_within_bytes(&value, MAX_DESTINATION_SETTING_VALUE_BYTES)
            {
                return Err(LogConfigurationError::SettingsInvalid);
            }
            if entries.iter().any(|(held, _)| held.as_ref() == key) {
                return Err(LogConfigurationError::SettingsInvalid);
            }
            entries.push((key.into_boxed_str(), value.into_boxed_str()));
        }
        entries.sort_by(|left, right| left.0.cmp(&right.0));
        Ok(Self(entries.into_boxed_slice()))
    }

    /// Returns whether the configuration declared no setting at all.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns how many settings the configuration declared.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns the value declared for `key`, if any.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(held, _)| held.as_ref() == key)
            .map(|(_, value)| value.as_ref())
    }

    /// Returns every declared key in canonical order.
    pub fn keys(&self) -> impl ExactSizeIterator<Item = &str> {
        self.0.iter().map(|(key, _)| key.as_ref())
    }
}

impl fmt::Debug for DestinationSettings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DestinationSettings")
            .field("setting_count", &self.0.len())
            .finish()
    }
}

/// The non-secret setting keys one Log Module defines.
///
/// A module declares this once, on its factory. The catalog carries that one
/// declaration on the module's validated declaration, so a committed
/// configuration can be judged against what the module accepts without opening
/// a destination, and the module's own factory refuses the settings it is
/// handed against the same declaration rather than a rule restated elsewhere.
#[derive(Clone, Default, Eq, PartialEq)]
pub struct LogSettingsContract(Box<[Box<str>]>);

impl LogSettingsContract {
    /// Declares a module that defines no setting at all.
    #[must_use]
    pub fn none() -> Self {
        Self(Box::default())
    }

    /// Validates and orders the setting keys a module defines.
    ///
    /// # Errors
    ///
    /// Returns [`LogCatalogError::InvalidSettingsDeclaration`] when the
    /// declaration exceeds [`MAX_DESTINATION_SETTINGS`], a key is empty or past
    /// [`MAX_DESTINATION_SETTING_KEY_BYTES`], or a key is declared twice.
    pub fn new(keys: Vec<String>) -> Result<Self, LogCatalogError> {
        if keys.len() > MAX_DESTINATION_SETTINGS {
            return Err(LogCatalogError::InvalidSettingsDeclaration);
        }
        let mut declared: Vec<Box<str>> = Vec::with_capacity(keys.len());
        for key in keys {
            if !is_nonempty_within_bytes(&key, MAX_DESTINATION_SETTING_KEY_BYTES)
                || declared.iter().any(|held| held.as_ref() == key)
            {
                return Err(LogCatalogError::InvalidSettingsDeclaration);
            }
            declared.push(key.into_boxed_str());
        }
        declared.sort();
        Ok(Self(declared.into_boxed_slice()))
    }

    /// Returns whether `key` is a setting this module defines.
    #[must_use]
    pub fn defines(&self, key: &str) -> bool {
        self.0
            .binary_search_by(|held| held.as_ref().cmp(key))
            .is_ok()
    }

    /// Returns whether every setting in `settings` is one this module defines.
    ///
    /// The comparison is pure: it opens no destination and creates no local
    /// state, so a configuration can be judged before anything durable exists.
    #[must_use]
    pub fn accepts(&self, settings: &DestinationSettings) -> bool {
        settings.keys().all(|key| self.defines(key))
    }

    /// Returns every declared key in canonical order.
    pub fn keys(&self) -> impl ExactSizeIterator<Item = &str> {
        self.0.iter().map(|key| key.as_ref())
    }
}

impl fmt::Debug for LogSettingsContract {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LogSettingsContract")
            .field("declared_key_count", &self.0.len())
            .finish()
    }
}

/// Trusted context supplied by Server runtime composition to a module factory.
pub struct TrustedLogModuleContext {
    local_root: PathBuf,
    deployment_identity: [u8; RECORD_ID_LENGTH],
    settings: DestinationSettings,
}

impl TrustedLogModuleContext {
    /// Creates the Server-owned runtime context with deployment-bound local inputs.
    #[allow(dead_code)]
    pub(crate) fn new(local_root: PathBuf, deployment_identity: [u8; RECORD_ID_LENGTH]) -> Self {
        Self {
            local_root,
            deployment_identity,
            settings: DestinationSettings::default(),
        }
    }

    /// Adds the committed configuration's non-secret settings to the context.
    #[must_use]
    pub fn with_settings(mut self, settings: DestinationSettings) -> Self {
        self.settings = settings;
        self
    }

    /// Creates the context for a holder of Server-owned logging authority.
    #[must_use]
    pub fn from_server_authority(
        _authority: &ServerLogAuthority,
        local_root: PathBuf,
        deployment_identity: [u8; RECORD_ID_LENGTH],
    ) -> Self {
        Self::new(local_root, deployment_identity)
    }

    /// Returns the Server-supplied local root without deriving a destination path.
    pub fn local_root(&self) -> &Path {
        &self.local_root
    }

    /// Returns the Server-supplied deployment identity for destination binding.
    pub const fn deployment_identity(&self) -> &[u8; RECORD_ID_LENGTH] {
        &self.deployment_identity
    }

    /// Returns the committed configuration's non-secret settings.
    pub const fn settings(&self) -> &DestinationSettings {
        &self.settings
    }
}

impl fmt::Debug for TrustedLogModuleContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TrustedLogModuleContext")
            .finish_non_exhaustive()
    }
}

/// Read-only trusted inputs available while a Log Module factory opens its destination.
pub struct LogModuleFactoryContext<'a> {
    local_root: &'a Path,
    deployment_identity: &'a [u8; RECORD_ID_LENGTH],
    settings: &'a DestinationSettings,
}

impl<'a> LogModuleFactoryContext<'a> {
    fn from_trusted(context: &'a TrustedLogModuleContext) -> Self {
        Self {
            local_root: context.local_root(),
            deployment_identity: context.deployment_identity(),
            settings: context.settings(),
        }
    }

    /// Returns the Server-supplied local root without deriving a destination path.
    pub const fn local_root(&self) -> &'a Path {
        self.local_root
    }

    /// Returns the Server-supplied deployment identity for destination binding.
    pub const fn deployment_identity(&self) -> &'a [u8; RECORD_ID_LENGTH] {
        self.deployment_identity
    }

    /// Returns the committed configuration's non-secret settings.
    ///
    /// A module must reject a setting it does not define: an unconfigured or
    /// misconfigured assignment is refused rather than silently ignored.
    pub const fn settings(&self) -> &'a DestinationSettings {
        self.settings
    }
}

impl fmt::Debug for LogModuleFactoryContext<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LogModuleFactoryContext")
            .finish_non_exhaustive()
    }
}

/// Synchronous durable acknowledgement returned by a destination.
#[derive(Eq, PartialEq)]
pub struct DurableAcknowledgement {
    record_id: RecordId,
    record_type: LogRecordType,
}

impl DurableAcknowledgement {
    /// Acknowledges exactly the record that was durably committed or matched.
    fn for_record(record: &CompleteLogRecord) -> Self {
        Self {
            record_id: record.record_id().duplicate(),
            record_type: record.record_type(),
        }
    }

    fn matches(&self, record: &CompleteLogRecord) -> bool {
        self.record_id == *record.record_id() && self.record_type == record.record_type()
    }
}

impl fmt::Debug for DurableAcknowledgement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DurableAcknowledgement(REDACTED)")
    }
}

/// Destination contract for capability validation and durable record handling only.
pub trait LogDestination: Send + Sync {
    /// Durably commits the record and returns its exact acknowledgement capability.
    fn deliver(
        &self,
        record: &CompleteLogRecord,
        acknowledgement: DurableAcknowledgement,
    ) -> Result<DurableAcknowledgement, LogDestinationError>;

    /// Proves the destination can complete its commit path for `record_type`.
    ///
    /// Init runs this before it commits application state, so an assignment
    /// whose configured storage interface could not durably accept the assigned
    /// log type is refused while the deployment can still be corrected rather
    /// than after the record it was supposed to carry already exists. The check
    /// must exercise the same commit path delivery uses and must leave no
    /// record behind.
    ///
    /// # Errors
    ///
    /// Returns a [`LogDestinationError`] when the commit path is unreachable
    /// for `record_type`.
    fn preflight(&self, record_type: LogRecordType) -> Result<(), LogDestinationError>;
}

/// Factory for one runtime-supplied compiled-in Log Module destination.
pub trait LogDestinationFactory: Send + Sync {
    /// Declares the non-secret settings this module's destinations accept.
    ///
    /// The declaration is required rather than defaulted, so a Log Module
    /// cannot be implemented without deciding which settings it defines. The
    /// catalog publishes this one declaration, and [`Self::create`] refuses the
    /// settings it is handed against the same declaration, so a configuration
    /// can be judged without opening anything and cannot be judged by a rule
    /// the module does not enforce.
    fn accepted_settings(&self) -> LogSettingsContract;

    /// Creates a destination using read-only Server-owned factory inputs.
    fn create(
        &self,
        context: &LogModuleFactoryContext<'_>,
    ) -> Result<Box<dyn LogDestination>, LogDestinationError>;
}

/// Unvalidated runtime registration for one compiled-in Log Module.
pub struct LogModuleRegistration {
    identifier: Box<str>,
    capabilities: LogCapabilities,
    factory: Box<dyn LogDestinationFactory>,
}

impl LogModuleRegistration {
    /// Creates a registration for catalog validation.
    pub fn new(
        identifier: impl Into<Box<str>>,
        capabilities: LogCapabilities,
        factory: Box<dyn LogDestinationFactory>,
    ) -> Self {
        Self {
            identifier: identifier.into(),
            capabilities,
            factory,
        }
    }
}

impl fmt::Debug for LogModuleRegistration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LogModuleRegistration")
            .finish_non_exhaustive()
    }
}

/// Validated compiled-in Log Module declaration.
#[derive(Clone, Eq, PartialEq)]
pub struct LogModuleDeclaration {
    identifier: LogModuleIdentifier,
    capabilities: LogCapabilities,
    accepted_settings: LogSettingsContract,
}

impl LogModuleDeclaration {
    /// Returns the stable module identifier.
    pub const fn identifier(&self) -> &LogModuleIdentifier {
        &self.identifier
    }

    /// Returns the destination's validated capability declaration.
    pub const fn capabilities(&self) -> &LogCapabilities {
        &self.capabilities
    }

    /// Returns the settings the module's factory declared it accepts.
    ///
    /// This is the module's own declaration, carried here so a caller can judge
    /// a configuration against it without creating a destination.
    pub const fn accepted_settings(&self) -> &LogSettingsContract {
        &self.accepted_settings
    }
}

impl fmt::Debug for LogModuleDeclaration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LogModuleDeclaration")
            .field("capabilities", &self.capabilities)
            .field("accepted_settings", &self.accepted_settings)
            .finish_non_exhaustive()
    }
}

struct LogModuleEntry {
    declaration: LogModuleDeclaration,
    factory: Box<dyn LogDestinationFactory>,
}

/// Validated catalog of compiled-in Log Module registrations.
pub struct LogModuleCatalog(Box<[LogModuleEntry]>);

impl LogModuleCatalog {
    /// Validates compiled-in registrations and creates the Server catalog.
    pub fn new(registrations: Vec<LogModuleRegistration>) -> Result<Self, LogCatalogError> {
        if registrations.is_empty() {
            return Err(LogCatalogError::EmptyCatalog);
        }
        if registrations.len() > MAX_LOG_MODULES {
            return Err(LogCatalogError::TooManyModules);
        }

        let mut entries = Vec::with_capacity(registrations.len());
        for registration in registrations {
            let identifier = LogModuleIdentifier::new(registration.identifier)?;
            if entries
                .iter()
                .any(|entry: &LogModuleEntry| entry.declaration.identifier == identifier)
            {
                return Err(LogCatalogError::DuplicateModuleIdentifier);
            }
            let accepted_settings = registration.factory.accepted_settings();
            entries.push(LogModuleEntry {
                declaration: LogModuleDeclaration {
                    identifier,
                    capabilities: registration.capabilities,
                    accepted_settings,
                },
                factory: registration.factory,
            });
        }
        entries.sort_by(|left, right| {
            left.declaration
                .identifier
                .cmp(&right.declaration.identifier)
        });
        Ok(Self(entries.into_boxed_slice()))
    }

    /// Iterates over validated declarations in canonical identifier order.
    pub fn declarations(&self) -> impl ExactSizeIterator<Item = &LogModuleDeclaration> {
        self.0.iter().map(|entry| &entry.declaration)
    }

    /// Returns one validated declaration.
    pub fn declaration(&self, identifier: &LogModuleIdentifier) -> Option<&LogModuleDeclaration> {
        self.entry(identifier).map(|entry| &entry.declaration)
    }

    /// Creates the configured destination selected by trusted runtime composition.
    pub fn create_destination(
        &self,
        identifier: &LogModuleIdentifier,
        context: &TrustedLogModuleContext,
    ) -> Result<ConfiguredLogDestination, LogConfigurationError> {
        let entry = self
            .entry(identifier)
            .ok_or(LogConfigurationError::UnknownModule)?;
        let factory_context = LogModuleFactoryContext::from_trusted(context);
        let destination = entry
            .factory
            .create(&factory_context)
            .map_err(LogConfigurationError::Destination)?;
        Ok(ConfiguredLogDestination {
            capabilities: entry.declaration.capabilities.clone(),
            destination,
        })
    }

    fn entry(&self, identifier: &LogModuleIdentifier) -> Option<&LogModuleEntry> {
        self.0
            .binary_search_by(|entry| entry.declaration.identifier.cmp(identifier))
            .ok()
            .map(|index| &self.0[index])
    }
}

impl fmt::Debug for LogModuleCatalog {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LogModuleCatalog")
            .field("module_count", &self.0.len())
            .finish()
    }
}

/// A destination selected from the validated compiled-in catalog.
pub struct ConfiguredLogDestination {
    capabilities: LogCapabilities,
    destination: Box<dyn LogDestination>,
}

impl ConfiguredLogDestination {
    /// Proves the destination can durably accept `record_type` before it is used.
    ///
    /// The declared capability is checked first, so an assignment to a module
    /// that does not serve the assigned log type is refused without reaching
    /// the module at all.
    ///
    /// # Errors
    ///
    /// Returns [`LogDeliveryError::CapabilityUnavailable`] when the module does
    /// not declare `record_type`, or [`LogDeliveryError::Destination`] when the
    /// module could not prove its commit path.
    pub fn preflight(&self, record_type: LogRecordType) -> Result<(), LogDeliveryError> {
        if !self.capabilities.supports(record_type) {
            return Err(LogDeliveryError::CapabilityUnavailable);
        }
        self.destination
            .preflight(record_type)
            .map_err(LogDeliveryError::Destination)
    }

    /// Synchronously delivers one complete record and requires durable acknowledgement.
    pub fn deliver(&self, record: &CompleteLogRecord) -> Result<(), LogDeliveryError> {
        if !self.capabilities.supports(record.record_type()) {
            return Err(LogDeliveryError::CapabilityUnavailable);
        }
        let acknowledgement = DurableAcknowledgement::for_record(record);
        let acknowledgement = self
            .destination
            .deliver(record, acknowledgement)
            .map_err(LogDeliveryError::Destination)?;
        if !acknowledgement.matches(record) {
            return Err(LogDeliveryError::IntegrityFailure);
        }
        Ok(())
    }
}

impl fmt::Debug for ConfiguredLogDestination {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConfiguredLogDestination")
            .field("capabilities", &self.capabilities)
            .finish_non_exhaustive()
    }
}

/// Invalid complete-record construction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordError {
    /// The identifier did not contain usable Server-generated entropy.
    InvalidRecordIdentifier,
    /// The correlation identifier was empty or exceeded its fixed bound.
    InvalidCorrelationIdentifier,
    /// A terminal Audit record did not reuse its Attempt's correlation identifier.
    MismatchedAttemptCorrelation,
    /// A terminal Audit record reused its Attempt identifier or preceded its Attempt time.
    InvalidAttemptLink,
    /// A System Log field was empty or exceeded its fixed bound.
    InvalidSystemLogBody,
    /// An Audit principal was empty, too long, or lacked its required owner.
    InvalidAuditPrincipal,
    /// An Audit Log field was empty or exceeded its fixed bound.
    InvalidAuditLogBody,
    /// The correlation identifier and record body exceeded their combined fixed bound.
    RecordSizeLimitExceeded,
}

impl fmt::Display for RecordError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("log record is invalid")
    }
}

impl StdError for RecordError {}

/// Invalid Log Module catalog input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogCatalogError {
    /// No module was registered.
    EmptyCatalog,
    /// The catalog exceeded its fixed registration bound.
    TooManyModules,
    /// A module identifier was invalid.
    InvalidModuleIdentifier,
    /// More than one registration used the same identifier.
    DuplicateModuleIdentifier,
    /// A module did not declare any record type.
    EmptyCapabilities,
    /// A module declared one record type more than once.
    DuplicateCapability,
    /// A module's accepted-settings declaration was unbounded or ambiguous.
    InvalidSettingsDeclaration,
}

impl fmt::Display for LogCatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("log module catalog is invalid")
    }
}

impl StdError for LogCatalogError {}

/// Stable payload-free failure returned by a Log Module destination.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum LogDestinationError {
    /// Destination configuration is invalid.
    ConfigurationInvalid,
    /// The destination cannot currently accept delivery.
    Unavailable,
    /// The destination detected a conflicting record identifier or immutable content.
    IntegrityFailure,
}

impl fmt::Display for LogDestinationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::ConfigurationInvalid => "log destination configuration is invalid",
            Self::Unavailable => "log destination is unavailable",
            Self::IntegrityFailure => "log destination integrity validation failed",
        };
        formatter.write_str(message)
    }
}

impl StdError for LogDestinationError {}

/// Stable payload-free failure returned while creating a configured destination.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogConfigurationError {
    /// The selected module is not in the compiled-in catalog.
    UnknownModule,
    /// The configuration's non-secret settings were unbounded or ambiguous.
    SettingsInvalid,
    /// The selected module rejected trusted runtime configuration.
    Destination(LogDestinationError),
}

impl fmt::Display for LogConfigurationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownModule => formatter.write_str("log module is not registered"),
            Self::SettingsInvalid => {
                formatter.write_str("log module configuration settings are invalid")
            }
            Self::Destination(error) => error.fmt(formatter),
        }
    }
}

impl StdError for LogConfigurationError {}

/// Stable payload-free failure returned while dispatching a complete record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogDeliveryError {
    /// The configured destination cannot accept the record type.
    CapabilityUnavailable,
    /// The destination did not produce a valid acknowledgement for the record.
    IntegrityFailure,
    /// The destination could not complete synchronous durable delivery.
    Destination(LogDestinationError),
}

impl fmt::Display for LogDeliveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CapabilityUnavailable => {
                formatter.write_str("log destination cannot accept this record type")
            }
            Self::IntegrityFailure => {
                formatter.write_str("log delivery integrity validation failed")
            }
            Self::Destination(error) => error.fmt(formatter),
        }
    }
}

impl StdError for LogDeliveryError {}

fn is_valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_IDENTIFIER_LENGTH
        && !value.starts_with('-')
        && !value.ends_with('-')
        && !value.as_bytes().windows(2).any(|pair| pair == b"--")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn is_nonempty_within_bytes(value: &str, maximum_bytes: usize) -> bool {
    !value.is_empty() && value.len() <= maximum_bytes
}

fn record_payload_bytes(correlation_id: &CorrelationId, body_fields: &[&str]) -> usize {
    body_fields
        .iter()
        .fold(correlation_id.as_str().len(), |size, field| {
            size.saturating_add(field.len())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Debug, Eq, PartialEq)]
    enum PersistedRecord {
        System {
            record_id: [u8; RECORD_ID_LENGTH],
            event_time: u64,
            result: LogResult,
            correlation_id: Box<str>,
            classification: Box<str>,
            detail: Box<str>,
        },
        Audit {
            record_id: [u8; RECORD_ID_LENGTH],
            event_time: u64,
            phase: Box<str>,
            result: Option<LogResult>,
            attempt_record_id: Option<[u8; RECORD_ID_LENGTH]>,
            correlation_id: Box<str>,
            classification: Box<str>,
            principal: Box<str>,
            principal_type: Box<str>,
            responsible_owner: Option<Box<str>>,
            action: Box<str>,
            target: Box<str>,
            detail: Box<str>,
        },
    }

    impl PersistedRecord {
        fn record_id(&self) -> &[u8; RECORD_ID_LENGTH] {
            match self {
                Self::System { record_id, .. } | Self::Audit { record_id, .. } => record_id,
            }
        }

        fn from_record(record: &CompleteLogRecord) -> Self {
            match record.persistence_view() {
                LogRecordPersistenceView::System(record) => Self::System {
                    record_id: *record.record_id().as_bytes(),
                    event_time: record.event_time().unix_milliseconds(),
                    result: record.result(),
                    correlation_id: record.correlation_id().as_str().into(),
                    classification: record.body().classification().into(),
                    detail: record.body().detail().into(),
                },
                LogRecordPersistenceView::Audit(record) => Self::Audit {
                    record_id: *record.record_id().as_bytes(),
                    event_time: record.event_time().unix_milliseconds(),
                    phase: record.phase().as_str().into(),
                    result: record.phase().result(),
                    attempt_record_id: record
                        .phase()
                        .attempt_record_id()
                        .map(|record_id| *record_id.as_bytes()),
                    correlation_id: record.correlation_id().as_str().into(),
                    classification: record.body().classification().into(),
                    principal: record.body().principal().into(),
                    principal_type: record.body().principal_type().as_str().into(),
                    responsible_owner: record.body().responsible_owner().map(Into::into),
                    action: record.body().action().into(),
                    target: record.body().target().into(),
                    detail: record.body().detail().into(),
                },
            }
        }
    }

    #[derive(Clone)]
    struct ReplayDestination {
        records: Arc<Mutex<Vec<PersistedRecord>>>,
    }

    impl ReplayDestination {
        fn new(records: Arc<Mutex<Vec<PersistedRecord>>>) -> Self {
            Self { records }
        }
    }

    impl LogDestination for ReplayDestination {
        fn deliver(
            &self,
            record: &CompleteLogRecord,
            acknowledgement: DurableAcknowledgement,
        ) -> Result<DurableAcknowledgement, LogDestinationError> {
            let mut records = self
                .records
                .lock()
                .expect("test record lock must not poison");
            let persisted_record = PersistedRecord::from_record(record);
            if let Some(existing) = records
                .iter()
                .find(|existing| existing.record_id() == persisted_record.record_id())
            {
                if existing != &persisted_record {
                    return Err(LogDestinationError::IntegrityFailure);
                }
                return Ok(acknowledgement);
            }
            records.push(persisted_record);
            Ok(acknowledgement)
        }

        fn preflight(&self, _record_type: LogRecordType) -> Result<(), LogDestinationError> {
            let _records = self
                .records
                .lock()
                .expect("test record lock must not poison");
            Ok(())
        }
    }

    struct ReplayFactory {
        records: Arc<Mutex<Vec<PersistedRecord>>>,
    }

    impl LogDestinationFactory for ReplayFactory {
        fn accepted_settings(&self) -> LogSettingsContract {
            LogSettingsContract::none()
        }

        fn create(
            &self,
            _context: &LogModuleFactoryContext<'_>,
        ) -> Result<Box<dyn LogDestination>, LogDestinationError> {
            Ok(Box::new(ReplayDestination::new(Arc::clone(&self.records))))
        }
    }

    type ObservedFactoryContext = (PathBuf, [u8; RECORD_ID_LENGTH]);

    struct ContextForwardingFactory {
        observed_context: Arc<Mutex<Option<ObservedFactoryContext>>>,
    }

    impl LogDestinationFactory for ContextForwardingFactory {
        fn accepted_settings(&self) -> LogSettingsContract {
            LogSettingsContract::none()
        }

        fn create(
            &self,
            context: &LogModuleFactoryContext<'_>,
        ) -> Result<Box<dyn LogDestination>, LogDestinationError> {
            *self
                .observed_context
                .lock()
                .expect("test context lock must not poison") = Some((
                context.local_root().to_path_buf(),
                *context.deployment_identity(),
            ));
            Ok(Box::new(ReplayDestination::new(Arc::new(Mutex::new(
                Vec::new(),
            )))))
        }
    }

    struct MismatchedAcknowledgementDestination;

    impl LogDestination for MismatchedAcknowledgementDestination {
        fn deliver(
            &self,
            record: &CompleteLogRecord,
            _acknowledgement: DurableAcknowledgement,
        ) -> Result<DurableAcknowledgement, LogDestinationError> {
            let record_type = match record.record_type() {
                LogRecordType::System => LogRecordType::Audit,
                LogRecordType::Audit => LogRecordType::System,
            };
            Ok(DurableAcknowledgement {
                record_id: record.record_id().duplicate(),
                record_type,
            })
        }

        fn preflight(&self, _record_type: LogRecordType) -> Result<(), LogDestinationError> {
            Ok(())
        }
    }

    struct MismatchedAcknowledgementFactory;

    impl LogDestinationFactory for MismatchedAcknowledgementFactory {
        fn accepted_settings(&self) -> LogSettingsContract {
            LogSettingsContract::none()
        }

        fn create(
            &self,
            _context: &LogModuleFactoryContext<'_>,
        ) -> Result<Box<dyn LogDestination>, LogDestinationError> {
            Ok(Box::new(MismatchedAcknowledgementDestination))
        }
    }

    struct UnprovableDestination;

    impl LogDestination for UnprovableDestination {
        fn deliver(
            &self,
            _record: &CompleteLogRecord,
            acknowledgement: DurableAcknowledgement,
        ) -> Result<DurableAcknowledgement, LogDestinationError> {
            Ok(acknowledgement)
        }

        fn preflight(&self, _record_type: LogRecordType) -> Result<(), LogDestinationError> {
            Err(LogDestinationError::Unavailable)
        }
    }

    /// A module that accepts only the settings it defines.
    struct SettingsBoundFactory {
        observed: Arc<Mutex<Vec<(String, String)>>>,
    }

    impl SettingsBoundFactory {
        /// The one declaration the catalog publishes and `create` refuses against.
        fn accepted_settings() -> LogSettingsContract {
            LogSettingsContract::new(vec!["retention_days".to_owned()])
                .expect("the test settings declaration is valid")
        }
    }

    impl LogDestinationFactory for SettingsBoundFactory {
        fn accepted_settings(&self) -> LogSettingsContract {
            Self::accepted_settings()
        }

        fn create(
            &self,
            context: &LogModuleFactoryContext<'_>,
        ) -> Result<Box<dyn LogDestination>, LogDestinationError> {
            let settings = context.settings();
            if !Self::accepted_settings().accepts(settings) {
                return Err(LogDestinationError::ConfigurationInvalid);
            }
            *self.observed.lock().expect("test lock must not poison") = settings
                .keys()
                .map(|key| {
                    (
                        key.to_owned(),
                        settings.get(key).expect("declared key resolves").to_owned(),
                    )
                })
                .collect();
            Ok(Box::new(ReplayDestination::new(Arc::new(Mutex::new(
                Vec::new(),
            )))))
        }
    }

    fn trusted_context() -> TrustedLogModuleContext {
        TrustedLogModuleContext::new(PathBuf::from("/srv/weavelit"), [9; RECORD_ID_LENGTH])
    }

    fn system_record(record_id: RecordId, detail: &str) -> CompleteLogRecord {
        CompleteLogRecord::system(
            record_id,
            EventTime::from_unix_milliseconds(1_725_000_000_000),
            LogResult::Success,
            CorrelationId::new("correlation-1").expect("valid correlation identifier"),
            SystemLogBody::new(SystemLogClassification::LifecycleStartup, detail)
                .expect("complete system body"),
        )
        .expect("complete system record")
    }

    fn audit_record(record_id: RecordId) -> CompleteLogRecord {
        let issuer = TrustedRecordIssuer::new();
        let attempt = CompleteLogRecord::audit_attempt(
            issuer.issue([0xa0; RECORD_ID_LENGTH]).unwrap(),
            EventTime::from_unix_milliseconds(1_724_999_999_999),
            CorrelationId::new("correlation-1").unwrap(),
            AuditLogBody::new(
                AuditLogClassification::LifecycleBackupCreated,
                AuditPrincipal::human("administrator").unwrap(),
                "init",
                "deployment",
                "attempt",
            )
            .unwrap(),
        )
        .unwrap();
        CompleteLogRecord::audit_completion(
            record_id,
            EventTime::from_unix_milliseconds(1_725_000_000_000),
            attempt.attempt_record_id().unwrap(),
            LogResult::Success,
            CorrelationId::new("correlation-1").expect("valid correlation identifier"),
            AuditLogBody::new(
                AuditLogClassification::LifecycleBackupCreated,
                AuditPrincipal::human("administrator").expect("complete human principal"),
                "init",
                "deployment",
                "complete",
            )
            .expect("complete audit body"),
        )
        .expect("complete audit record")
    }

    fn utf8_overflow(maximum_bytes: usize) -> String {
        format!("{}€", "x".repeat(maximum_bytes - 2))
    }

    fn assert_rejection_is_payload_free(error: RecordError, rejected: &str) {
        if !rejected.is_empty() {
            assert!(!error.to_string().contains(rejected));
            assert!(!format!("{error:?}").contains(rejected));
        }
    }

    #[test]
    fn classification_catalogs_project_every_registered_canonical_literal() {
        for (classification, literal) in [
            (
                SystemLogClassification::LifecycleStartup,
                "lifecycle.startup",
            ),
            (SystemLogClassification::LifecycleInit, "lifecycle.init"),
            (
                SystemLogClassification::LifecycleRestore,
                "lifecycle.restore",
            ),
            (
                SystemLogClassification::OperationalState,
                "operational.state",
            ),
            (
                SystemLogClassification::ConfigurationChange,
                "configuration.change",
            ),
            (
                SystemLogClassification::AuthenticationFailure,
                "authentication.failure",
            ),
            (
                SystemLogClassification::AuthorizationDenial,
                "authorization.denial",
            ),
            (
                SystemLogClassification::DependencyFailure,
                "dependency.failure",
            ),
            (
                SystemLogClassification::DependencyAuditLogUnavailable,
                "dependency.audit-log-unavailable",
            ),
            (SystemLogClassification::ProviderFailure, "provider.failure"),
            (SystemLogClassification::InternalError, "internal.error"),
        ] {
            assert_eq!(classification.as_str(), literal);
            assert_eq!(
                SystemLogBody::new(classification, "detail")
                    .expect("registered System classifications construct")
                    .classification(),
                literal
            );
        }

        for (classification, literal) in [
            (
                AuditLogClassification::LifecycleBackupCreated,
                "lifecycle.backup.created",
            ),
            (
                AuditLogClassification::AuthenticationUserCreated,
                "authentication.user.created",
            ),
            (
                AuditLogClassification::AuthenticationUserDisabled,
                "authentication.user.disabled",
            ),
            (
                AuditLogClassification::AuthenticationPasswordChanged,
                "authentication.password.changed",
            ),
            (
                AuditLogClassification::AuthenticationPasswordResetStarted,
                "authentication.password-reset.started",
            ),
            (
                AuditLogClassification::AuthenticationMfaEnrolled,
                "authentication.mfa.enrolled",
            ),
            (
                AuditLogClassification::AuthenticationMfaReset,
                "authentication.mfa.reset",
            ),
            (
                AuditLogClassification::AuthenticationMfaRequirementChanged,
                "authentication.mfa-requirement.changed",
            ),
            (
                AuditLogClassification::AuthenticationMfaModuleEnablementChanged,
                "authentication.mfa-module-enablement.changed",
            ),
            (
                AuditLogClassification::AuthenticationSessionRevoked,
                "authentication.session.revoked",
            ),
            (
                AuditLogClassification::AuthorizationGroupCreated,
                "authorization.group.created",
            ),
            (
                AuditLogClassification::AuthorizationGroupMembershipChanged,
                "authorization.group-membership.changed",
            ),
            (
                AuditLogClassification::AuthorizationGroupGrantChanged,
                "authorization.group-grant.changed",
            ),
            (
                AuditLogClassification::AuthorizationGroupGrantRemovalDenied,
                "authorization.group-grant.removal-denied",
            ),
            (
                AuditLogClassification::AuthorizationAutomationScopeChanged,
                "authorization.automation-scope.changed",
            ),
            (
                AuditLogClassification::DependencyLogModuleConfigurationChanged,
                "dependency.log-module-configuration.changed",
            ),
            (
                AuditLogClassification::DependencyServiceConnectionChanged,
                "dependency.service-connection.changed",
            ),
            (
                AuditLogClassification::ProviderOperationStarted,
                "provider.operation.started",
            ),
            (
                AuditLogClassification::ProviderOperationCompleted,
                "provider.operation.completed",
            ),
            (
                AuditLogClassification::InternalServerConfigurationChanged,
                "internal.server-configuration.changed",
            ),
            (
                AuditLogClassification::InternalUserStatusChanged,
                "internal.user-status.changed",
            ),
            (
                AuditLogClassification::InternalLogPolicyChanged,
                "internal.log-policy.changed",
            ),
        ] {
            assert_eq!(classification.as_str(), literal);
            assert_eq!(
                AuditLogBody::new(
                    classification,
                    AuditPrincipal::human("administrator").expect("bounded human principal"),
                    "action",
                    "target",
                    "detail",
                )
                .expect("registered Audit classifications construct")
                .classification(),
                literal
            );
        }
    }

    #[test]
    fn audit_principals_enforce_owner_shape_and_redact_rejections() {
        let human = AuditPrincipal::human("administrator").expect("bounded human principal");
        assert_eq!(human.principal_type(), AuditPrincipalType::Human);
        assert_eq!(human.responsible_owner(), None);

        let automation = AuditPrincipal::automation("restore-worker", "administrator")
            .expect("bounded automation principal");
        assert_eq!(automation.principal_type(), AuditPrincipalType::Automation);
        assert_eq!(automation.responsible_owner(), Some("administrator"));
        for sensitive in ["restore-worker", "administrator"] {
            assert!(!format!("{automation:?}").contains(sensitive));
        }

        for rejected in [String::new(), "p".repeat(MAX_AUDIT_PRINCIPAL_BYTES + 1)] {
            let error = AuditPrincipal::human(rejected.as_str()).unwrap_err();
            assert_eq!(error, RecordError::InvalidAuditPrincipal);
            assert_rejection_is_payload_free(error, &rejected);
        }
        for rejected in [
            String::new(),
            "o".repeat(MAX_AUDIT_RESPONSIBLE_OWNER_BYTES + 1),
        ] {
            let error = AuditPrincipal::automation("automation", rejected.as_str()).unwrap_err();
            assert_eq!(error, RecordError::InvalidAuditPrincipal);
            assert_rejection_is_payload_free(error, &rejected);
        }
    }

    #[test]
    fn audit_phases_expose_only_their_valid_result_combinations() {
        let issuer = TrustedRecordIssuer::new();
        let attempt = CompleteLogRecord::audit_attempt(
            issuer.issue([0xa1; RECORD_ID_LENGTH]).unwrap(),
            EventTime::from_unix_milliseconds(1),
            CorrelationId::new("correlation-1").unwrap(),
            AuditLogBody::new(
                AuditLogClassification::LifecycleBackupCreated,
                AuditPrincipal::human("administrator").unwrap(),
                "backup",
                "deployment",
                "attempt",
            )
            .unwrap(),
        )
        .unwrap();
        let valid = [
            (AuditRecordPhase::Attempt, "attempt", None, false),
            (
                AuditRecordPhase::Completion {
                    attempt_record_id: attempt.attempt_record_id().unwrap(),
                    result: LogResult::Success,
                },
                "completion",
                Some(LogResult::Success),
                true,
            ),
            (
                AuditRecordPhase::Completion {
                    attempt_record_id: attempt.attempt_record_id().unwrap(),
                    result: LogResult::Failure,
                },
                "completion",
                Some(LogResult::Failure),
                true,
            ),
            (
                AuditRecordPhase::Correction {
                    attempt_record_id: attempt.attempt_record_id().unwrap(),
                    result: LogResult::Success,
                },
                "correction",
                Some(LogResult::Success),
                true,
            ),
            (
                AuditRecordPhase::Correction {
                    attempt_record_id: attempt.attempt_record_id().unwrap(),
                    result: LogResult::Failure,
                },
                "correction",
                Some(LogResult::Failure),
                true,
            ),
        ];

        for (phase, literal, result, has_attempt_link) in valid {
            assert_eq!(phase.as_str(), literal);
            assert_eq!(phase.result(), result);
            assert_eq!(phase.attempt_record_id().is_some(), has_attempt_link);
        }
    }

    #[test]
    fn only_attempts_mint_redacted_linkage_capabilities_for_matching_correlations() {
        let issuer = TrustedRecordIssuer::new();
        let attempt = CompleteLogRecord::audit_attempt(
            issuer.issue([0xa2; RECORD_ID_LENGTH]).unwrap(),
            EventTime::from_unix_milliseconds(1),
            CorrelationId::new("correlation-1").unwrap(),
            AuditLogBody::new(
                AuditLogClassification::LifecycleBackupCreated,
                AuditPrincipal::human("administrator").unwrap(),
                "backup",
                "deployment",
                "attempt",
            )
            .unwrap(),
        )
        .unwrap();
        let attempt_record_id = attempt.attempt_record_id().unwrap();
        assert_eq!(attempt_record_id.as_bytes(), &[0xa2; RECORD_ID_LENGTH]);
        assert_eq!(
            format!("{attempt_record_id:?}"),
            "AttemptRecordId(REDACTED)"
        );

        let error = CompleteLogRecord::audit_completion(
            issuer.issue([0xa3; RECORD_ID_LENGTH]).unwrap(),
            EventTime::from_unix_milliseconds(2),
            attempt_record_id,
            LogResult::Success,
            CorrelationId::new("different-correlation").unwrap(),
            AuditLogBody::new(
                AuditLogClassification::LifecycleBackupCreated,
                AuditPrincipal::human("administrator").unwrap(),
                "backup",
                "deployment",
                "complete",
            )
            .unwrap(),
        )
        .unwrap_err();
        assert_eq!(error, RecordError::MismatchedAttemptCorrelation);
        assert!(!format!("{error:?}").contains("correlation-1"));
        assert!(!error.to_string().contains("different-correlation"));
        assert!(attempt.attempt_record_id().is_some());

        for (record_id, event_time) in
            [([0xa2; RECORD_ID_LENGTH], 2), ([0xa4; RECORD_ID_LENGTH], 0)]
        {
            let error = CompleteLogRecord::audit_correction(
                issuer.issue(record_id).unwrap(),
                EventTime::from_unix_milliseconds(event_time),
                attempt.attempt_record_id().unwrap(),
                LogResult::Failure,
                CorrelationId::new("correlation-1").unwrap(),
                AuditLogBody::new(
                    AuditLogClassification::LifecycleBackupCreated,
                    AuditPrincipal::human("administrator").unwrap(),
                    "backup",
                    "deployment",
                    "corrected",
                )
                .unwrap(),
            )
            .unwrap_err();
            assert_eq!(error, RecordError::InvalidAttemptLink);
            assert_eq!(error.to_string(), "log record is invalid");
        }
    }

    fn cargo_json_messages(output: &std::process::Output) -> Vec<Value> {
        let stdout =
            std::str::from_utf8(&output.stdout).expect("Cargo JSON output must be valid UTF-8");
        stdout
            .lines()
            .map(|line| serde_json::from_str(line).expect("Cargo must emit JSON messages"))
            .collect()
    }

    fn assert_forbidden_fixture_rejected(
        fixture_root: &Path,
        target_root: &Path,
        binary: &str,
        expected_code: &str,
    ) {
        let forbidden = std::process::Command::new(env!("CARGO"))
            .arg("check")
            .arg("--offline")
            .arg("--quiet")
            .arg("--message-format=json")
            .arg("--manifest-path")
            .arg(fixture_root.join("forbidden-authority/Cargo.toml"))
            .arg("--bin")
            .arg(binary)
            .env("CARGO_TARGET_DIR", target_root)
            .output()
            .expect("forbidden external fixture must run");

        assert!(
            !forbidden.status.success(),
            "forbidden {binary} fixture unexpectedly compiled"
        );

        let messages = cargo_json_messages(&forbidden);
        let errors = messages
            .iter()
            .filter(|message| {
                message["reason"] == "compiler-message" && message["message"]["level"] == "error"
            })
            .collect::<Vec<_>>();
        assert_eq!(
            errors.len(),
            1,
            "forbidden {binary} fixture must emit exactly one structured compiler error"
        );

        let diagnostic = &errors[0]["message"];
        assert_eq!(
            diagnostic["code"]["code"].as_str(),
            Some(expected_code),
            "forbidden {binary} fixture emitted an unexpected rustc code"
        );
        let expected_source = format!("src/{}.rs", binary.replace('-', "_"));
        assert!(
            diagnostic["spans"].as_array().is_some_and(|spans| {
                spans
                    .iter()
                    .any(|span| span["is_primary"] == true && span["file_name"] == expected_source)
            }),
            "forbidden {binary} fixture must identify its authority-minting source span"
        );
    }

    #[test]
    fn record_constructors_accept_exact_utf8_byte_boundaries() {
        let issuer = TrustedRecordIssuer::new();
        let correlation_id = CorrelationId::new("c".repeat(MAX_CORRELATION_ID_BYTES)).unwrap();
        let system_body = SystemLogBody::new(
            SystemLogClassification::LifecycleStartup,
            "d".repeat(MAX_SYSTEM_DETAIL_BYTES),
        )
        .unwrap();
        let audit_body = AuditLogBody::new(
            AuditLogClassification::LifecycleBackupCreated,
            AuditPrincipal::human("p".repeat(MAX_AUDIT_PRINCIPAL_BYTES)).unwrap(),
            "a".repeat(MAX_AUDIT_ACTION_BYTES),
            "t".repeat(MAX_AUDIT_TARGET_BYTES),
            "d".repeat(MAX_AUDIT_DETAIL_BYTES),
        )
        .unwrap();
        let automation_body = AuditLogBody::new(
            AuditLogClassification::LifecycleBackupCreated,
            AuditPrincipal::automation(
                "p".repeat(MAX_AUDIT_PRINCIPAL_BYTES),
                "o".repeat(MAX_AUDIT_RESPONSIBLE_OWNER_BYTES),
            )
            .unwrap(),
            "a".repeat(MAX_AUDIT_ACTION_BYTES),
            "t".repeat(MAX_AUDIT_TARGET_BYTES),
            "d".repeat(MAX_AUDIT_DETAIL_BYTES),
        )
        .unwrap();

        assert!(
            CompleteLogRecord::system(
                issuer.issue([1; RECORD_ID_LENGTH]).unwrap(),
                EventTime::from_unix_milliseconds(1),
                LogResult::Success,
                correlation_id.clone(),
                system_body,
            )
            .is_ok()
        );
        let attempt = CompleteLogRecord::audit_attempt(
            issuer.issue([2; RECORD_ID_LENGTH]).unwrap(),
            EventTime::from_unix_milliseconds(1),
            correlation_id,
            audit_body,
        )
        .unwrap();
        assert!(
            CompleteLogRecord::audit_correction(
                issuer.issue([3; RECORD_ID_LENGTH]).unwrap(),
                EventTime::from_unix_milliseconds(1),
                attempt.attempt_record_id().unwrap(),
                LogResult::Failure,
                CorrelationId::new("c".repeat(MAX_CORRELATION_ID_BYTES)).unwrap(),
                automation_body,
            )
            .is_ok()
        );
    }

    #[test]
    fn record_constructors_reject_byte_and_utf8_overflows_without_payloads() {
        for rejected in [
            String::new(),
            "x".repeat(MAX_CORRELATION_ID_BYTES + 1),
            utf8_overflow(MAX_CORRELATION_ID_BYTES),
        ] {
            let error = CorrelationId::new(rejected.as_str()).unwrap_err();
            assert_eq!(error, RecordError::InvalidCorrelationIdentifier);
            assert_rejection_is_payload_free(error, &rejected);
        }

        for rejected in [
            String::new(),
            "x".repeat(MAX_SYSTEM_DETAIL_BYTES + 1),
            utf8_overflow(MAX_SYSTEM_DETAIL_BYTES),
        ] {
            let error =
                SystemLogBody::new(SystemLogClassification::LifecycleStartup, rejected.as_str())
                    .unwrap_err();
            assert_eq!(error, RecordError::InvalidSystemLogBody);
            assert_rejection_is_payload_free(error, &rejected);
        }

        for (field, maximum_bytes) in [
            ("action", MAX_AUDIT_ACTION_BYTES),
            ("target", MAX_AUDIT_TARGET_BYTES),
            ("detail", MAX_AUDIT_DETAIL_BYTES),
        ] {
            for rejected in [
                String::new(),
                "x".repeat(maximum_bytes + 1),
                utf8_overflow(maximum_bytes),
            ] {
                let (action, target, detail) = match field {
                    "action" => (rejected.as_str(), "target", "detail"),
                    "target" => ("action", rejected.as_str(), "detail"),
                    "detail" => ("action", "target", rejected.as_str()),
                    _ => unreachable!("the test enumerates every Audit body field"),
                };
                let error = AuditLogBody::new(
                    AuditLogClassification::LifecycleBackupCreated,
                    AuditPrincipal::human("principal").unwrap(),
                    action,
                    target,
                    detail,
                )
                .unwrap_err();
                assert_eq!(error, RecordError::InvalidAuditLogBody);
                assert_rejection_is_payload_free(error, &rejected);
            }
        }
    }

    #[test]
    fn complete_records_reject_aggregate_overflows_before_delivery() {
        let system_rejected = "x".repeat(MAX_RECORD_PAYLOAD_BYTES);
        let body = SystemLogBody {
            classification: SystemLogClassification::LifecycleStartup,
            detail: system_rejected.clone().into(),
        };
        let mut deliveries = 0;
        let result = (|| -> Result<(), RecordError> {
            let record = CompleteLogRecord::system(
                TrustedRecordIssuer::new().issue([1; RECORD_ID_LENGTH])?,
                EventTime::from_unix_milliseconds(1),
                LogResult::Success,
                CorrelationId::new("correlation")?,
                body,
            )?;
            let _ = record;
            deliveries += 1;
            Ok(())
        })();

        assert_eq!(result, Err(RecordError::RecordSizeLimitExceeded));
        assert_eq!(deliveries, 0);
        assert_rejection_is_payload_free(RecordError::RecordSizeLimitExceeded, &system_rejected);

        let audit_rejected = "y".repeat(MAX_RECORD_PAYLOAD_BYTES);
        let audit_body = AuditLogBody {
            classification: AuditLogClassification::LifecycleBackupCreated,
            principal: AuditPrincipal::human("principal").unwrap(),
            action: "action".into(),
            target: "target".into(),
            detail: audit_rejected.clone().into(),
        };
        let error = CompleteLogRecord::audit_attempt(
            TrustedRecordIssuer::new()
                .issue([2; RECORD_ID_LENGTH])
                .unwrap(),
            EventTime::from_unix_milliseconds(1),
            CorrelationId::new("correlation").unwrap(),
            audit_body,
        )
        .unwrap_err();
        assert_eq!(error, RecordError::RecordSizeLimitExceeded);
        assert_rejection_is_payload_free(error, &audit_rejected);
    }

    fn catalog(
        capabilities: LogCapabilities,
        records: Arc<Mutex<Vec<PersistedRecord>>>,
    ) -> LogModuleCatalog {
        LogModuleCatalog::new(vec![LogModuleRegistration::new(
            "sqlite",
            capabilities,
            Box::new(ReplayFactory { records }),
        )])
        .expect("valid log module catalog")
    }

    #[test]
    fn catalog_rejects_duplicate_module_registration() {
        let capabilities = LogCapabilities::new(vec![LogRecordType::System])
            .expect("valid capability declaration");
        let records = Arc::new(Mutex::new(Vec::<PersistedRecord>::new()));
        let registrations = vec![
            LogModuleRegistration::new(
                "sqlite",
                capabilities.clone(),
                Box::new(ReplayFactory {
                    records: Arc::clone(&records),
                }),
            ),
            LogModuleRegistration::new("sqlite", capabilities, Box::new(ReplayFactory { records })),
        ];

        assert!(matches!(
            LogModuleCatalog::new(registrations),
            Err(LogCatalogError::DuplicateModuleIdentifier)
        ));
    }

    #[test]
    fn configured_destination_dispatches_and_forwards_durable_acknowledgement() {
        let records = Arc::new(Mutex::new(Vec::<PersistedRecord>::new()));
        let catalog = catalog(
            LogCapabilities::new(vec![LogRecordType::System, LogRecordType::Audit])
                .expect("valid capability declaration"),
            Arc::clone(&records),
        );
        let identifier = LogModuleIdentifier::new("sqlite").expect("valid module identifier");
        let destination = catalog
            .create_destination(&identifier, &trusted_context())
            .expect("registered destination is configured");
        let issuer = TrustedRecordIssuer::new();
        let record = system_record(
            issuer
                .issue([1; RECORD_ID_LENGTH])
                .expect("valid record ID"),
            "complete",
        );

        assert_eq!(destination.deliver(&record), Ok(()));
        assert_eq!(
            records
                .lock()
                .expect("test record lock must not poison")
                .as_slice(),
            &[PersistedRecord::from_record(&record)]
        );
    }

    #[test]
    fn configured_destination_rejects_unsupported_record_type_before_delivery() {
        let records = Arc::new(Mutex::new(Vec::<PersistedRecord>::new()));
        let catalog = catalog(
            LogCapabilities::new(vec![LogRecordType::System])
                .expect("valid capability declaration"),
            Arc::clone(&records),
        );
        let identifier = LogModuleIdentifier::new("sqlite").expect("valid module identifier");
        let destination = catalog
            .create_destination(&identifier, &trusted_context())
            .expect("registered destination is configured");
        let issuer = TrustedRecordIssuer::new();
        let record = audit_record(
            issuer
                .issue([2; RECORD_ID_LENGTH])
                .expect("valid record ID"),
        );

        assert_eq!(
            destination.deliver(&record),
            Err(LogDeliveryError::CapabilityUnavailable)
        );
        assert!(
            records
                .lock()
                .expect("test record lock must not poison")
                .is_empty()
        );
    }

    #[test]
    fn exact_replay_is_acknowledged_without_a_second_persisted_record() {
        let records = Arc::new(Mutex::new(Vec::<PersistedRecord>::new()));
        let catalog = catalog(
            LogCapabilities::new(vec![LogRecordType::System])
                .expect("valid capability declaration"),
            Arc::clone(&records),
        );
        let identifier = LogModuleIdentifier::new("sqlite").expect("valid module identifier");
        let destination = catalog
            .create_destination(&identifier, &trusted_context())
            .expect("registered destination is configured");
        let issuer = TrustedRecordIssuer::new();
        let record = system_record(
            issuer
                .issue([3; RECORD_ID_LENGTH])
                .expect("valid record ID"),
            "complete",
        );

        assert_eq!(destination.deliver(&record), Ok(()));
        assert_eq!(destination.deliver(&record), Ok(()));
        assert_eq!(
            records
                .lock()
                .expect("test record lock must not poison")
                .len(),
            1
        );
    }

    #[test]
    fn changed_replay_for_the_same_identifier_is_an_integrity_failure() {
        let records = Arc::new(Mutex::new(Vec::<PersistedRecord>::new()));
        let catalog = catalog(
            LogCapabilities::new(vec![LogRecordType::System])
                .expect("valid capability declaration"),
            records,
        );
        let identifier = LogModuleIdentifier::new("sqlite").expect("valid module identifier");
        let destination = catalog
            .create_destination(&identifier, &trusted_context())
            .expect("registered destination is configured");
        let issuer = TrustedRecordIssuer::new();
        let record_id = issuer
            .issue([4; RECORD_ID_LENGTH])
            .expect("valid record ID");
        let original = system_record(record_id.duplicate(), "complete");
        let changed = system_record(record_id, "different-complete-record");

        assert_eq!(destination.deliver(&original), Ok(()));
        assert_eq!(
            destination.deliver(&changed),
            Err(LogDeliveryError::Destination(
                LogDestinationError::IntegrityFailure
            ))
        );
    }

    #[test]
    fn changed_audit_phase_for_the_same_identifier_is_an_integrity_failure() {
        let records = Arc::new(Mutex::new(Vec::<PersistedRecord>::new()));
        let catalog = catalog(
            LogCapabilities::new(vec![LogRecordType::Audit]).expect("valid capability declaration"),
            records,
        );
        let identifier = LogModuleIdentifier::new("sqlite").expect("valid module identifier");
        let destination = catalog
            .create_destination(&identifier, &trusted_context())
            .expect("registered destination is configured");
        let issuer = TrustedRecordIssuer::new();
        let attempt_record_id = issuer
            .issue([6; RECORD_ID_LENGTH])
            .expect("valid record ID");
        let terminal_record_id = issuer
            .issue([7; RECORD_ID_LENGTH])
            .expect("valid record ID");
        let correlation_id = CorrelationId::new("correlation-1").unwrap();
        let body = || {
            AuditLogBody::new(
                AuditLogClassification::LifecycleBackupCreated,
                AuditPrincipal::human("administrator").unwrap(),
                "backup",
                "deployment",
                "pre-redacted",
            )
            .unwrap()
        };
        let attempt = CompleteLogRecord::audit_attempt(
            attempt_record_id,
            EventTime::from_unix_milliseconds(1_725_000_000_000),
            correlation_id.clone(),
            body(),
        )
        .unwrap();
        let original = CompleteLogRecord::audit_completion(
            terminal_record_id.duplicate(),
            EventTime::from_unix_milliseconds(1_725_000_000_001),
            attempt.attempt_record_id().unwrap(),
            LogResult::Success,
            correlation_id.clone(),
            body(),
        )
        .unwrap();
        let changed = CompleteLogRecord::audit_correction(
            terminal_record_id,
            EventTime::from_unix_milliseconds(1_725_000_000_001),
            attempt.attempt_record_id().unwrap(),
            LogResult::Success,
            correlation_id,
            body(),
        )
        .unwrap();

        assert_eq!(destination.deliver(&attempt), Ok(()));
        assert_eq!(destination.deliver(&original), Ok(()));
        assert_eq!(
            destination.deliver(&changed),
            Err(LogDeliveryError::Destination(
                LogDestinationError::IntegrityFailure
            ))
        );
    }

    #[test]
    fn changed_audit_result_with_the_same_phase_is_an_integrity_failure() {
        let records = Arc::new(Mutex::new(Vec::<PersistedRecord>::new()));
        let catalog = catalog(
            LogCapabilities::new(vec![LogRecordType::Audit]).expect("valid capability declaration"),
            records,
        );
        let destination = catalog
            .create_destination(
                &LogModuleIdentifier::new("sqlite").unwrap(),
                &trusted_context(),
            )
            .unwrap();
        let issuer = TrustedRecordIssuer::new();
        let attempt = CompleteLogRecord::audit_attempt(
            issuer.issue([0xa4; RECORD_ID_LENGTH]).unwrap(),
            EventTime::from_unix_milliseconds(1),
            CorrelationId::new("correlation-1").unwrap(),
            AuditLogBody::new(
                AuditLogClassification::LifecycleBackupCreated,
                AuditPrincipal::human("administrator").unwrap(),
                "backup",
                "deployment",
                "attempt",
            )
            .unwrap(),
        )
        .unwrap();
        let body = || {
            AuditLogBody::new(
                AuditLogClassification::LifecycleBackupCreated,
                AuditPrincipal::human("administrator").unwrap(),
                "backup",
                "deployment",
                "complete",
            )
            .unwrap()
        };
        let record_id = issuer.issue([0xa5; RECORD_ID_LENGTH]).unwrap();
        let original = CompleteLogRecord::audit_completion(
            record_id.duplicate(),
            EventTime::from_unix_milliseconds(2),
            attempt.attempt_record_id().unwrap(),
            LogResult::Success,
            CorrelationId::new("correlation-1").unwrap(),
            body(),
        )
        .unwrap();
        let changed = CompleteLogRecord::audit_completion(
            record_id,
            EventTime::from_unix_milliseconds(2),
            attempt.attempt_record_id().unwrap(),
            LogResult::Failure,
            CorrelationId::new("correlation-1").unwrap(),
            body(),
        )
        .unwrap();

        assert_eq!(destination.deliver(&original), Ok(()));
        assert_eq!(
            destination.deliver(&changed),
            Err(LogDeliveryError::Destination(
                LogDestinationError::IntegrityFailure
            ))
        );
    }

    #[test]
    fn changed_record_type_for_the_same_identifier_is_an_integrity_failure() {
        let records = Arc::new(Mutex::new(Vec::<PersistedRecord>::new()));
        let catalog = catalog(
            LogCapabilities::new(vec![LogRecordType::System, LogRecordType::Audit])
                .expect("valid capability declaration"),
            records,
        );
        let identifier = LogModuleIdentifier::new("sqlite").expect("valid module identifier");
        let destination = catalog
            .create_destination(&identifier, &trusted_context())
            .expect("registered destination is configured");
        let issuer = TrustedRecordIssuer::new();
        let record_id = issuer
            .issue([5; RECORD_ID_LENGTH])
            .expect("valid record ID");
        let system_record = system_record(record_id.duplicate(), "complete");
        let audit_record = audit_record(record_id);

        assert_eq!(destination.deliver(&system_record), Ok(()));
        assert_eq!(
            destination.deliver(&audit_record),
            Err(LogDeliveryError::Destination(
                LogDestinationError::IntegrityFailure
            ))
        );
    }

    #[test]
    fn configured_destination_rejects_an_acknowledgement_for_the_wrong_record_type() {
        let catalog = LogModuleCatalog::new(vec![LogModuleRegistration::new(
            "sqlite",
            LogCapabilities::new(vec![LogRecordType::System])
                .expect("valid capability declaration"),
            Box::new(MismatchedAcknowledgementFactory),
        )])
        .expect("valid log module catalog");
        let identifier = LogModuleIdentifier::new("sqlite").expect("valid module identifier");
        let destination = catalog
            .create_destination(&identifier, &trusted_context())
            .expect("registered destination is configured");
        let record = system_record(
            TrustedRecordIssuer::new()
                .issue([6; RECORD_ID_LENGTH])
                .expect("valid record ID"),
            "complete",
        );

        assert_eq!(
            destination.deliver(&record),
            Err(LogDeliveryError::IntegrityFailure)
        );
    }

    #[test]
    fn errors_do_not_disclose_record_or_destination_payloads() {
        let error = LogDeliveryError::Destination(LogDestinationError::IntegrityFailure);
        let record = system_record(
            TrustedRecordIssuer::new()
                .issue([6; RECORD_ID_LENGTH])
                .expect("valid record ID"),
            "password=secret path=/srv/weavelit/log.sqlite3",
        );

        let display = error.to_string();
        let debug = format!("{error:?}");
        let record_debug = format!("{record:?}");
        for secret in ["password", "secret", "/srv", "sqlite", "correlation"] {
            assert!(!display.contains(secret));
            assert!(!debug.contains(secret));
            assert!(!record_debug.contains(secret));
        }
    }

    #[test]
    fn catalog_forwards_server_supplied_local_root_and_deployment_identity() {
        let observed_context = Arc::new(Mutex::new(None));
        let catalog = LogModuleCatalog::new(vec![LogModuleRegistration::new(
            "sqlite",
            LogCapabilities::new(vec![LogRecordType::System])
                .expect("valid capability declaration"),
            Box::new(ContextForwardingFactory {
                observed_context: Arc::clone(&observed_context),
            }),
        )])
        .expect("valid log module catalog");
        let identifier = LogModuleIdentifier::new("sqlite").expect("valid module identifier");
        let context =
            TrustedLogModuleContext::new(PathBuf::from("/var/lib/weavelit"), [8; RECORD_ID_LENGTH]);

        let _destination = catalog
            .create_destination(&identifier, &context)
            .expect("registered destination is configured");

        assert_eq!(
            *observed_context
                .lock()
                .expect("test context lock must not poison"),
            Some((PathBuf::from("/var/lib/weavelit"), [8; RECORD_ID_LENGTH]))
        );
    }

    #[test]
    fn destination_settings_reject_unbounded_or_ambiguous_configuration() {
        assert_eq!(
            DestinationSettings::new(
                (0..=MAX_DESTINATION_SETTINGS)
                    .map(|index| (format!("key-{index}"), "value".to_owned()))
                    .collect()
            ),
            Err(LogConfigurationError::SettingsInvalid)
        );
        assert_eq!(
            DestinationSettings::new(vec![(String::new(), "value".to_owned())]),
            Err(LogConfigurationError::SettingsInvalid)
        );
        assert_eq!(
            DestinationSettings::new(vec![("key".to_owned(), String::new())]),
            Err(LogConfigurationError::SettingsInvalid)
        );
        assert_eq!(
            DestinationSettings::new(vec![(
                "k".repeat(MAX_DESTINATION_SETTING_KEY_BYTES + 1),
                "value".to_owned()
            )]),
            Err(LogConfigurationError::SettingsInvalid)
        );
        assert_eq!(
            DestinationSettings::new(vec![(
                "key".to_owned(),
                "v".repeat(MAX_DESTINATION_SETTING_VALUE_BYTES + 1)
            )]),
            Err(LogConfigurationError::SettingsInvalid)
        );
        assert_eq!(
            DestinationSettings::new(vec![
                ("key".to_owned(), "first".to_owned()),
                ("key".to_owned(), "second".to_owned()),
            ]),
            Err(LogConfigurationError::SettingsInvalid)
        );

        let accepted = DestinationSettings::new(vec![
            (
                "k".repeat(MAX_DESTINATION_SETTING_KEY_BYTES),
                "v".repeat(MAX_DESTINATION_SETTING_VALUE_BYTES),
            ),
            ("retention_days".to_owned(), "30".to_owned()),
        ])
        .expect("bounded settings are accepted");
        assert_eq!(accepted.len(), 2);
        assert_eq!(accepted.get("retention_days"), Some("30"));
        assert_eq!(accepted.get("absent"), None);
        assert!(DestinationSettings::default().is_empty());
    }

    #[test]
    fn a_factory_receives_the_committed_configuration_settings() {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let catalog = LogModuleCatalog::new(vec![LogModuleRegistration::new(
            "sqlite",
            LogCapabilities::new(vec![LogRecordType::System])
                .expect("valid capability declaration"),
            Box::new(SettingsBoundFactory {
                observed: Arc::clone(&observed),
            }),
        )])
        .expect("valid log module catalog");
        let identifier = LogModuleIdentifier::new("sqlite").expect("valid module identifier");

        let accepted = trusted_context().with_settings(
            DestinationSettings::new(vec![("retention_days".to_owned(), "30".to_owned())])
                .expect("bounded settings"),
        );
        let _destination = catalog
            .create_destination(&identifier, &accepted)
            .expect("a defined setting is accepted");
        assert_eq!(
            observed
                .lock()
                .expect("test lock must not poison")
                .as_slice(),
            &[("retention_days".to_owned(), "30".to_owned())]
        );

        let rejected = trusted_context().with_settings(
            DestinationSettings::new(vec![("unknown".to_owned(), "1".to_owned())])
                .expect("bounded settings"),
        );
        assert_eq!(
            catalog
                .create_destination(&identifier, &rejected)
                .expect_err("an undefined setting is refused"),
            LogConfigurationError::Destination(LogDestinationError::ConfigurationInvalid)
        );
    }

    #[test]
    fn a_settings_declaration_rejects_an_unbounded_or_ambiguous_key_set() {
        assert_eq!(
            LogSettingsContract::new(
                (0..=MAX_DESTINATION_SETTINGS)
                    .map(|index| format!("key-{index}"))
                    .collect()
            ),
            Err(LogCatalogError::InvalidSettingsDeclaration)
        );
        assert_eq!(
            LogSettingsContract::new(vec![String::new()]),
            Err(LogCatalogError::InvalidSettingsDeclaration)
        );
        assert_eq!(
            LogSettingsContract::new(vec!["k".repeat(MAX_DESTINATION_SETTING_KEY_BYTES + 1)]),
            Err(LogCatalogError::InvalidSettingsDeclaration)
        );
        assert_eq!(
            LogSettingsContract::new(vec!["key".to_owned(), "key".to_owned()]),
            Err(LogCatalogError::InvalidSettingsDeclaration)
        );

        let declared =
            LogSettingsContract::new(vec!["retention_days".to_owned(), "host".to_owned()])
                .expect("a bounded declaration is accepted");
        assert_eq!(
            declared.keys().collect::<Vec<_>>(),
            ["host", "retention_days"]
        );
        assert!(declared.defines("host"));
        assert!(!declared.defines("absent"));

        let none = LogSettingsContract::none();
        assert_eq!(none.keys().len(), 0);
        assert!(none.accepts(&DestinationSettings::default()));
        assert!(
            !none.accepts(
                &DestinationSettings::new(vec![("retention_days".to_owned(), "30".to_owned())])
                    .expect("bounded settings")
            )
        );
    }

    /// The catalog carries the module's own declaration, so a configuration can
    /// be judged against it without creating a destination.
    #[test]
    fn the_catalog_publishes_the_settings_declaration_its_factory_makes() {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let catalog = LogModuleCatalog::new(vec![LogModuleRegistration::new(
            "sqlite",
            LogCapabilities::new(vec![LogRecordType::System])
                .expect("valid capability declaration"),
            Box::new(SettingsBoundFactory {
                observed: Arc::clone(&observed),
            }),
        )])
        .expect("valid log module catalog");
        let identifier = LogModuleIdentifier::new("sqlite").expect("valid module identifier");
        let undeclared = DestinationSettings::new(vec![("unknown".to_owned(), "1".to_owned())])
            .expect("bounded settings");

        let published = catalog
            .declaration(&identifier)
            .expect("a registered module publishes its declaration")
            .accepted_settings();
        assert_eq!(published.keys().collect::<Vec<_>>(), ["retention_days"]);
        assert!(
            published.accepts(
                &DestinationSettings::new(vec![("retention_days".to_owned(), "30".to_owned())])
                    .expect("bounded settings")
            )
        );
        assert!(!published.accepts(&undeclared));

        // Judging the configuration created nothing, and the module refuses the
        // same configuration against the same declaration when it is opened.
        assert!(
            observed
                .lock()
                .expect("test lock must not poison")
                .is_empty()
        );
        assert_eq!(
            catalog
                .create_destination(&identifier, &trusted_context().with_settings(undeclared))
                .expect_err("an undeclared setting is refused"),
            LogConfigurationError::Destination(LogDestinationError::ConfigurationInvalid)
        );
    }

    #[test]
    fn preflight_refuses_an_undeclared_record_type_without_reaching_the_module() {
        let records = Arc::new(Mutex::new(Vec::<PersistedRecord>::new()));
        let catalog = catalog(
            LogCapabilities::new(vec![LogRecordType::System])
                .expect("valid capability declaration"),
            Arc::clone(&records),
        );
        let identifier = LogModuleIdentifier::new("sqlite").expect("valid module identifier");
        let destination = catalog
            .create_destination(&identifier, &trusted_context())
            .expect("registered destination is configured");

        assert_eq!(destination.preflight(LogRecordType::System), Ok(()));
        assert_eq!(
            destination.preflight(LogRecordType::Audit),
            Err(LogDeliveryError::CapabilityUnavailable)
        );
        assert!(
            records
                .lock()
                .expect("test record lock must not poison")
                .is_empty(),
            "preflight must leave no record behind"
        );
    }

    #[test]
    fn preflight_surfaces_a_destination_that_cannot_prove_its_commit_path() {
        struct UnprovableFactory;

        impl LogDestinationFactory for UnprovableFactory {
            fn accepted_settings(&self) -> LogSettingsContract {
                LogSettingsContract::none()
            }

            fn create(
                &self,
                _context: &LogModuleFactoryContext<'_>,
            ) -> Result<Box<dyn LogDestination>, LogDestinationError> {
                Ok(Box::new(UnprovableDestination))
            }
        }

        let catalog = LogModuleCatalog::new(vec![LogModuleRegistration::new(
            "sqlite",
            LogCapabilities::new(vec![LogRecordType::System, LogRecordType::Audit])
                .expect("valid capability declaration"),
            Box::new(UnprovableFactory),
        )])
        .expect("valid log module catalog");
        let identifier = LogModuleIdentifier::new("sqlite").expect("valid module identifier");
        let destination = catalog
            .create_destination(&identifier, &trusted_context())
            .expect("registered destination is configured");

        assert_eq!(
            destination.preflight(LogRecordType::System),
            Err(LogDeliveryError::Destination(
                LogDestinationError::Unavailable
            ))
        );
    }

    #[test]
    fn external_consumers_can_register_but_cannot_construct_server_authority() {
        let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let target_root = std::env::temp_dir().join(format!(
            "weavelit-server-log-fixtures-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&target_root);

        let permitted = std::process::Command::new(env!("CARGO"))
            .arg("check")
            .arg("--offline")
            .arg("--quiet")
            .arg("--manifest-path")
            .arg(fixture_root.join("permitted-module/Cargo.toml"))
            .env("CARGO_TARGET_DIR", &target_root)
            .output()
            .expect("permitted external fixture must run");
        assert!(
            permitted.status.success(),
            "permitted external fixture failed: {}",
            String::from_utf8_lossy(&permitted.stderr)
        );

        let metadata = std::process::Command::new(env!("CARGO"))
            .arg("metadata")
            .arg("--offline")
            .arg("--format-version=1")
            .arg("--manifest-path")
            .arg(fixture_root.join("removed-test-support/Cargo.toml"))
            .output()
            .expect("external fixture metadata query must run");
        assert!(
            metadata.status.success(),
            "external fixture metadata query failed: {}",
            String::from_utf8_lossy(&metadata.stderr)
        );
        let metadata: Value =
            serde_json::from_slice(&metadata.stdout).expect("Cargo metadata must be valid JSON");
        let manifest_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let manifest_path = manifest_path
            .to_str()
            .expect("log contract manifest path must be UTF-8");
        let package = metadata["packages"]
            .as_array()
            .and_then(|packages| {
                packages.iter().find(|package| {
                    package["name"] == "weavelit-server-log"
                        && package["manifest_path"].as_str() == Some(manifest_path)
                })
            })
            .expect("Cargo metadata must include the log contract package");
        let features = package["features"]
            .as_object()
            .expect("Cargo metadata must include the log contract feature map");
        assert!(
            !features.contains_key("test-support"),
            "external consumer metadata must not expose the removed test-support feature"
        );

        for (binary, expected_code) in [
            ("issuer", "E0624"),
            ("record-identity", "E0308"),
            ("attempt-record-identity", "E0308"),
            ("audit-record", "E0639"),
            ("audit-completion-phase", "E0639"),
            ("audit-correction-phase", "E0639"),
            ("context", "E0624"),
            ("acknowledgement", "E0624"),
            ("dispatch", "E0451"),
            ("catalog-dispatch", "E0308"),
            ("server-authority", "E0603"),
        ] {
            assert_forbidden_fixture_rejected(&fixture_root, &target_root, binary, expected_code);
        }
        let _ = std::fs::remove_dir_all(&target_root);
        for fixture in [
            "permitted-module",
            "forbidden-authority",
            "removed-test-support",
        ] {
            let _ = std::fs::remove_file(fixture_root.join(fixture).join("Cargo.lock"));
        }
    }
}
