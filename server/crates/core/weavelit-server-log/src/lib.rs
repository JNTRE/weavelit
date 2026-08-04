#![forbid(unsafe_code)]

//! Typed Server-owned dispatch contract for complete, pre-redacted log records.

use std::{
    error::Error as StdError,
    fmt,
    path::{Path, PathBuf},
};

const MAX_LOG_MODULES: usize = 64;
const MAX_IDENTIFIER_LENGTH: usize = 64;
const RECORD_ID_LENGTH: usize = 16;
const MAX_CORRELATION_ID_BYTES: usize = 64;
const MAX_SYSTEM_CLASSIFICATION_BYTES: usize = 128;
const MAX_SYSTEM_DETAIL_BYTES: usize = 4 * 1024;
const MAX_AUDIT_PRINCIPAL_BYTES: usize = 256;
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

/// Typed pre-redacted System Log body owned by Observability.
#[derive(Clone, Eq, PartialEq)]
pub struct SystemLogBody {
    classification: Box<str>,
    detail: Box<str>,
}

impl SystemLogBody {
    /// Creates a complete pre-redacted System Log body.
    pub fn new(
        classification: impl Into<Box<str>>,
        detail: impl Into<Box<str>>,
    ) -> Result<Self, RecordError> {
        let classification = classification.into();
        let detail = detail.into();
        if !is_nonempty_within_bytes(&classification, MAX_SYSTEM_CLASSIFICATION_BYTES)
            || !is_nonempty_within_bytes(&detail, MAX_SYSTEM_DETAIL_BYTES)
        {
            return Err(RecordError::InvalidSystemLogBody);
        }
        Ok(Self {
            classification,
            detail,
        })
    }

    fn is_valid(&self) -> bool {
        is_nonempty_within_bytes(&self.classification, MAX_SYSTEM_CLASSIFICATION_BYTES)
            && is_nonempty_within_bytes(&self.detail, MAX_SYSTEM_DETAIL_BYTES)
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
        &self.classification
    }

    /// Returns the pre-redacted operational detail.
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

/// Typed pre-redacted Audit Log body owned by Audit.
#[derive(Clone, Eq, PartialEq)]
pub struct AuditLogBody {
    principal: Box<str>,
    action: Box<str>,
    target: Box<str>,
    detail: Box<str>,
}

impl AuditLogBody {
    /// Creates a complete pre-redacted Audit Log body.
    pub fn new(
        principal: impl Into<Box<str>>,
        action: impl Into<Box<str>>,
        target: impl Into<Box<str>>,
        detail: impl Into<Box<str>>,
    ) -> Result<Self, RecordError> {
        let principal = principal.into();
        let action = action.into();
        let target = target.into();
        let detail = detail.into();
        if !is_nonempty_within_bytes(&principal, MAX_AUDIT_PRINCIPAL_BYTES)
            || !is_nonempty_within_bytes(&action, MAX_AUDIT_ACTION_BYTES)
            || !is_nonempty_within_bytes(&target, MAX_AUDIT_TARGET_BYTES)
            || !is_nonempty_within_bytes(&detail, MAX_AUDIT_DETAIL_BYTES)
        {
            return Err(RecordError::InvalidAuditLogBody);
        }
        Ok(Self {
            principal,
            action,
            target,
            detail,
        })
    }

    fn is_valid(&self) -> bool {
        is_nonempty_within_bytes(&self.principal, MAX_AUDIT_PRINCIPAL_BYTES)
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
    /// Returns the pre-redacted accountable principal.
    pub fn principal(&self) -> &str {
        &self.principal
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
    Audit {
        record_id: RecordId,
        event_time: EventTime,
        result: LogResult,
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

    /// Constructs a complete pre-redacted Audit Log record.
    pub fn audit(
        record_id: RecordId,
        event_time: EventTime,
        result: LogResult,
        correlation_id: CorrelationId,
        body: AuditLogBody,
    ) -> Result<Self, RecordError> {
        if record_payload_bytes(
            &correlation_id,
            &[
                body.principal(),
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
            result,
            correlation_id,
            body,
        })
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
                result,
                correlation_id,
                body,
            } => LogRecordPersistenceView::Audit(AuditLogPersistenceView {
                record_id,
                event_time: *event_time,
                result: *result,
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
    result: LogResult,
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

    /// Returns the immutable result classification.
    pub const fn result(&self) -> LogResult {
        self.result
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

/// Trusted context supplied by Server runtime composition to a module factory.
pub struct TrustedLogModuleContext {
    local_root: PathBuf,
    deployment_identity: [u8; RECORD_ID_LENGTH],
}

impl TrustedLogModuleContext {
    /// Creates the Server-owned runtime context with deployment-bound local inputs.
    #[allow(dead_code)]
    pub(crate) fn new(local_root: PathBuf, deployment_identity: [u8; RECORD_ID_LENGTH]) -> Self {
        Self {
            local_root,
            deployment_identity,
        }
    }

    /// Returns the Server-supplied local root without deriving a destination path.
    pub fn local_root(&self) -> &Path {
        &self.local_root
    }

    /// Returns the Server-supplied deployment identity for destination binding.
    pub const fn deployment_identity(&self) -> &[u8; RECORD_ID_LENGTH] {
        &self.deployment_identity
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
}

impl<'a> LogModuleFactoryContext<'a> {
    fn from_trusted(context: &'a TrustedLogModuleContext) -> Self {
        Self {
            local_root: context.local_root(),
            deployment_identity: context.deployment_identity(),
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
}

/// Factory for one runtime-supplied compiled-in Log Module destination.
pub trait LogDestinationFactory: Send + Sync {
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
}

impl fmt::Debug for LogModuleDeclaration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LogModuleDeclaration")
            .field("capabilities", &self.capabilities)
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
            entries.push(LogModuleEntry {
                declaration: LogModuleDeclaration {
                    identifier,
                    capabilities: registration.capabilities,
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
    /// A System Log field was empty or exceeded its fixed bound.
    InvalidSystemLogBody,
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
    /// The selected module rejected trusted runtime configuration.
    Destination(LogDestinationError),
}

impl fmt::Display for LogConfigurationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownModule => formatter.write_str("log module is not registered"),
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
            result: LogResult,
            correlation_id: Box<str>,
            principal: Box<str>,
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
                    result: record.result(),
                    correlation_id: record.correlation_id().as_str().into(),
                    principal: record.body().principal().into(),
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
    }

    struct ReplayFactory {
        records: Arc<Mutex<Vec<PersistedRecord>>>,
    }

    impl LogDestinationFactory for ReplayFactory {
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
    }

    struct MismatchedAcknowledgementFactory;

    impl LogDestinationFactory for MismatchedAcknowledgementFactory {
        fn create(
            &self,
            _context: &LogModuleFactoryContext<'_>,
        ) -> Result<Box<dyn LogDestination>, LogDestinationError> {
            Ok(Box::new(MismatchedAcknowledgementDestination))
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
            SystemLogBody::new("lifecycle-complete", detail).expect("complete system body"),
        )
        .expect("complete system record")
    }

    fn audit_record(record_id: RecordId) -> CompleteLogRecord {
        CompleteLogRecord::audit(
            record_id,
            EventTime::from_unix_milliseconds(1_725_000_000_000),
            LogResult::Success,
            CorrelationId::new("correlation-1").expect("valid correlation identifier"),
            AuditLogBody::new("administrator", "init", "deployment", "complete")
                .expect("complete audit body"),
        )
        .expect("complete audit record")
    }

    fn utf8_overflow(maximum_bytes: usize) -> String {
        format!("{}€", "x".repeat(maximum_bytes - 2))
    }

    fn assert_rejection_is_payload_free(error: RecordError, rejected: &str) {
        assert!(!error.to_string().contains(rejected));
        assert!(!format!("{error:?}").contains(rejected));
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
            "s".repeat(MAX_SYSTEM_CLASSIFICATION_BYTES),
            "d".repeat(MAX_SYSTEM_DETAIL_BYTES),
        )
        .unwrap();
        let audit_body = AuditLogBody::new(
            "p".repeat(MAX_AUDIT_PRINCIPAL_BYTES),
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
        assert!(
            CompleteLogRecord::audit(
                issuer.issue([2; RECORD_ID_LENGTH]).unwrap(),
                EventTime::from_unix_milliseconds(1),
                LogResult::Success,
                correlation_id,
                audit_body,
            )
            .is_ok()
        );
    }

    #[test]
    fn record_constructors_reject_byte_and_utf8_overflows_without_payloads() {
        for rejected in [
            "x".repeat(MAX_CORRELATION_ID_BYTES + 1),
            utf8_overflow(MAX_CORRELATION_ID_BYTES),
        ] {
            let error = CorrelationId::new(rejected.as_str()).unwrap_err();
            assert_eq!(error, RecordError::InvalidCorrelationIdentifier);
            assert_rejection_is_payload_free(error, &rejected);
        }

        for rejected in [
            "x".repeat(MAX_SYSTEM_DETAIL_BYTES + 1),
            utf8_overflow(MAX_SYSTEM_DETAIL_BYTES),
        ] {
            let error = SystemLogBody::new("classification", rejected.as_str()).unwrap_err();
            assert_eq!(error, RecordError::InvalidSystemLogBody);
            assert_rejection_is_payload_free(error, &rejected);
        }

        for rejected in [
            "x".repeat(MAX_AUDIT_TARGET_BYTES + 1),
            utf8_overflow(MAX_AUDIT_TARGET_BYTES),
        ] {
            let error =
                AuditLogBody::new("principal", "action", rejected.as_str(), "detail").unwrap_err();
            assert_eq!(error, RecordError::InvalidAuditLogBody);
            assert_rejection_is_payload_free(error, &rejected);
        }
    }

    #[test]
    fn complete_record_rejects_an_aggregate_overflow_before_delivery() {
        let rejected = "x".repeat(MAX_RECORD_PAYLOAD_BYTES);
        let body = SystemLogBody {
            classification: "classification".into(),
            detail: rejected.clone().into(),
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
        assert_rejection_is_payload_free(RecordError::RecordSizeLimitExceeded, &rejected);
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
            ("context", "E0624"),
            ("acknowledgement", "E0624"),
            ("dispatch", "E0451"),
            ("catalog-dispatch", "E0308"),
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
