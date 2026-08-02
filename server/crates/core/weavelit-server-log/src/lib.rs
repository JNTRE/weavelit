#![forbid(unsafe_code)]

//! Typed Server-owned dispatch contract for complete, pre-redacted log records.

use std::{error::Error as StdError, fmt};

const MAX_LOG_MODULES: usize = 64;
const MAX_IDENTIFIER_LENGTH: usize = 64;
const RECORD_ID_LENGTH: usize = 16;

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
#[derive(Clone, Eq, Hash, PartialEq)]
pub struct RecordId([u8; RECORD_ID_LENGTH]);

impl fmt::Debug for RecordId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RecordId(REDACTED)")
    }
}

/// Server-internal issuer for opaque record identifiers.
pub struct TrustedRecordIssuer {
    _private: (),
}

impl TrustedRecordIssuer {
    /// Creates the Server-owned issuer used by Audit and Observability.
    pub const fn new() -> Self {
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

impl Default for TrustedRecordIssuer {
    fn default() -> Self {
        Self::new()
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
        if value.is_empty() || value.len() > MAX_IDENTIFIER_LENGTH {
            return Err(RecordError::InvalidCorrelationIdentifier);
        }
        Ok(Self(value))
    }
}

impl fmt::Debug for CorrelationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CorrelationId(REDACTED)")
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
        if classification.is_empty() || detail.is_empty() {
            return Err(RecordError::IncompleteRecord);
        }
        Ok(Self {
            classification,
            detail,
        })
    }
}

impl fmt::Debug for SystemLogBody {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SystemLogBody(REDACTED)")
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
        if principal.is_empty() || action.is_empty() || target.is_empty() || detail.is_empty() {
            return Err(RecordError::IncompleteRecord);
        }
        Ok(Self {
            principal,
            action,
            target,
            detail,
        })
    }
}

impl fmt::Debug for AuditLogBody {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AuditLogBody(REDACTED)")
    }
}

/// Complete immutable record dispatched to a Log Module.
#[derive(Clone, Eq, PartialEq)]
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
    pub const fn system(
        record_id: RecordId,
        event_time: EventTime,
        result: LogResult,
        correlation_id: CorrelationId,
        body: SystemLogBody,
    ) -> Self {
        Self::System {
            record_id,
            event_time,
            result,
            correlation_id,
            body,
        }
    }

    /// Constructs a complete pre-redacted Audit Log record.
    pub const fn audit(
        record_id: RecordId,
        event_time: EventTime,
        result: LogResult,
        correlation_id: CorrelationId,
        body: AuditLogBody,
    ) -> Self {
        Self::Audit {
            record_id,
            event_time,
            result,
            correlation_id,
            body,
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
}

impl fmt::Debug for CompleteLogRecord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CompleteLogRecord(REDACTED)")
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
    _private: (),
}

impl TrustedLogModuleContext {
    /// Creates the Server-owned runtime context.
    pub const fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for TrustedLogModuleContext {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for TrustedLogModuleContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TrustedLogModuleContext")
            .finish_non_exhaustive()
    }
}

/// Synchronous durable acknowledgement returned by a destination.
#[derive(Clone, Eq, PartialEq)]
pub struct DurableAcknowledgement {
    record_id: RecordId,
    record_type: LogRecordType,
}

impl DurableAcknowledgement {
    /// Acknowledges exactly the record that was durably committed or matched.
    pub fn for_record(record: &CompleteLogRecord) -> Self {
        Self {
            record_id: record.record_id().clone(),
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
    /// Durably commits the record or acknowledges an exact prior record with its ID and type.
    fn deliver(
        &self,
        record: &CompleteLogRecord,
    ) -> Result<DurableAcknowledgement, LogDestinationError>;
}

/// Factory for one runtime-supplied compiled-in Log Module destination.
pub trait LogDestinationFactory: Send + Sync {
    /// Creates a destination using only trusted runtime inputs.
    fn create(
        &self,
        context: &TrustedLogModuleContext,
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
        let destination = entry
            .factory
            .create(context)
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
        let acknowledgement = self
            .destination
            .deliver(record)
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
    /// A required complete-record field was absent.
    IncompleteRecord,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct ReplayDestination {
        records: Arc<Mutex<Vec<CompleteLogRecord>>>,
    }

    impl ReplayDestination {
        fn new(records: Arc<Mutex<Vec<CompleteLogRecord>>>) -> Self {
            Self { records }
        }
    }

    impl LogDestination for ReplayDestination {
        fn deliver(
            &self,
            record: &CompleteLogRecord,
        ) -> Result<DurableAcknowledgement, LogDestinationError> {
            let mut records = self
                .records
                .lock()
                .expect("test record lock must not poison");
            if let Some(existing) = records
                .iter()
                .find(|existing| existing.record_id() == record.record_id())
            {
                if existing != record {
                    return Err(LogDestinationError::IntegrityFailure);
                }
                return Ok(DurableAcknowledgement::for_record(record));
            }
            records.push(record.clone());
            Ok(DurableAcknowledgement::for_record(record))
        }
    }

    struct ReplayFactory {
        records: Arc<Mutex<Vec<CompleteLogRecord>>>,
    }

    impl LogDestinationFactory for ReplayFactory {
        fn create(
            &self,
            _context: &TrustedLogModuleContext,
        ) -> Result<Box<dyn LogDestination>, LogDestinationError> {
            Ok(Box::new(ReplayDestination::new(Arc::clone(&self.records))))
        }
    }

    fn system_record(record_id: RecordId, detail: &str) -> CompleteLogRecord {
        CompleteLogRecord::system(
            record_id,
            EventTime::from_unix_milliseconds(1_725_000_000_000),
            LogResult::Success,
            CorrelationId::new("correlation-1").expect("valid correlation identifier"),
            SystemLogBody::new("lifecycle-complete", detail).expect("complete system body"),
        )
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
    }

    fn catalog(
        capabilities: LogCapabilities,
        records: Arc<Mutex<Vec<CompleteLogRecord>>>,
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
        let records = Arc::new(Mutex::new(Vec::new()));
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
        let records = Arc::new(Mutex::new(Vec::new()));
        let catalog = catalog(
            LogCapabilities::new(vec![LogRecordType::System, LogRecordType::Audit])
                .expect("valid capability declaration"),
            Arc::clone(&records),
        );
        let identifier = LogModuleIdentifier::new("sqlite").expect("valid module identifier");
        let destination = catalog
            .create_destination(&identifier, &TrustedLogModuleContext::new())
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
            &[record]
        );
    }

    #[test]
    fn configured_destination_rejects_unsupported_record_type_before_delivery() {
        let records = Arc::new(Mutex::new(Vec::new()));
        let catalog = catalog(
            LogCapabilities::new(vec![LogRecordType::System])
                .expect("valid capability declaration"),
            Arc::clone(&records),
        );
        let identifier = LogModuleIdentifier::new("sqlite").expect("valid module identifier");
        let destination = catalog
            .create_destination(&identifier, &TrustedLogModuleContext::new())
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
        let records = Arc::new(Mutex::new(Vec::new()));
        let catalog = catalog(
            LogCapabilities::new(vec![LogRecordType::System])
                .expect("valid capability declaration"),
            Arc::clone(&records),
        );
        let identifier = LogModuleIdentifier::new("sqlite").expect("valid module identifier");
        let destination = catalog
            .create_destination(&identifier, &TrustedLogModuleContext::new())
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
        let records = Arc::new(Mutex::new(Vec::new()));
        let catalog = catalog(
            LogCapabilities::new(vec![LogRecordType::System])
                .expect("valid capability declaration"),
            records,
        );
        let identifier = LogModuleIdentifier::new("sqlite").expect("valid module identifier");
        let destination = catalog
            .create_destination(&identifier, &TrustedLogModuleContext::new())
            .expect("registered destination is configured");
        let issuer = TrustedRecordIssuer::new();
        let record_id = issuer
            .issue([4; RECORD_ID_LENGTH])
            .expect("valid record ID");
        let original = system_record(record_id.clone(), "complete");
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
        let records = Arc::new(Mutex::new(Vec::new()));
        let catalog = catalog(
            LogCapabilities::new(vec![LogRecordType::System, LogRecordType::Audit])
                .expect("valid capability declaration"),
            records,
        );
        let identifier = LogModuleIdentifier::new("sqlite").expect("valid module identifier");
        let destination = catalog
            .create_destination(&identifier, &TrustedLogModuleContext::new())
            .expect("registered destination is configured");
        let issuer = TrustedRecordIssuer::new();
        let record_id = issuer
            .issue([5; RECORD_ID_LENGTH])
            .expect("valid record ID");
        let system_record = system_record(record_id.clone(), "complete");
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
}
