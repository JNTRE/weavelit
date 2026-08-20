//! Immutable, backend-neutral Log Module configuration generation reads.

use std::{fmt, num::NonZeroU64};

use weavelit_server_database_authority::ServerDatabaseAuthority;

use crate::{
    DatabaseError, LogAssignment, LogModuleSetting, LogType, Name, StateIdentifier,
    ValidatedAuditTerminalObligationWrite,
};

/// Nonzero immutable generation version for one Log Module configuration.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LogConfigurationVersion(NonZeroU64);

impl LogConfigurationVersion {
    /// Initial version assigned to the first immutable generation.
    pub const INITIAL: Self = Self(NonZeroU64::MIN);

    /// Accepts a nonzero persisted version.
    pub const fn new(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Returns the nonzero integer representation used by persistence.
    pub const fn get(self) -> u64 {
        self.0.get()
    }

    /// Returns the next immutable generation version, or `None` at exhaustion.
    pub const fn checked_next(self) -> Option<Self> {
        match self.get().checked_add(1) {
            Some(value) => Self::new(value),
            None => None,
        }
    }
}

impl fmt::Debug for LogConfigurationVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("LogConfigurationVersion(REDACTED)")
    }
}

/// Capability for constructing trusted generation keys and persisted snapshots.
pub struct LogConfigurationGenerationPersistence {
    _private: (),
}

impl LogConfigurationGenerationPersistence {
    /// Issues generation persistence authority to Server-owned database selection.
    #[must_use]
    pub const fn from_server_authority(_authority: &ServerDatabaseAuthority) -> Self {
        Self { _private: () }
    }

    /// Binds one application-owned configuration identity to an exact version.
    #[must_use]
    pub const fn key(
        &self,
        configuration: StateIdentifier,
        version: LogConfigurationVersion,
    ) -> LogConfigurationGenerationKey {
        LogConfigurationGenerationKey {
            configuration,
            version,
        }
    }

    /// Constructs one immutable snapshot after a backend decodes bounded fields.
    pub fn generation(
        &self,
        key: LogConfigurationGenerationKey,
        module: Name,
        name: Name,
        enabled: bool,
        settings: Vec<LogModuleSetting>,
        log_types: Vec<LogType>,
    ) -> Result<LogConfigurationGeneration, LogConfigurationGenerationError> {
        if !settings.windows(2).all(|pair| pair[0].key < pair[1].key)
            || !log_types.windows(2).all(|pair| pair[0] < pair[1])
        {
            return Err(LogConfigurationGenerationError::InvalidOrdering);
        }
        Ok(LogConfigurationGeneration {
            key,
            module,
            name,
            enabled,
            settings: settings.into_boxed_slice(),
            log_types: log_types.into_boxed_slice(),
        })
    }
}

impl fmt::Debug for LogConfigurationGenerationPersistence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("LogConfigurationGenerationPersistence(REDACTED)")
    }
}

/// Exact internal key of one immutable configuration generation.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LogConfigurationGenerationKey {
    configuration: StateIdentifier,
    version: LogConfigurationVersion,
}

impl LogConfigurationGenerationKey {
    /// Returns the application-owned configuration identity.
    pub const fn configuration(&self) -> StateIdentifier {
        self.configuration
    }

    /// Returns the exact immutable generation version.
    pub const fn version(&self) -> LogConfigurationVersion {
        self.version
    }
}

impl fmt::Debug for LogConfigurationGenerationKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("LogConfigurationGenerationKey(REDACTED)")
    }
}

/// Immutable non-secret snapshot used to reconstruct one committed destination.
#[derive(Clone, Eq, PartialEq)]
pub struct LogConfigurationGeneration {
    key: LogConfigurationGenerationKey,
    module: Name,
    name: Name,
    enabled: bool,
    settings: Box<[LogModuleSetting]>,
    log_types: Box<[LogType]>,
}

impl LogConfigurationGeneration {
    /// Returns the exact internal generation key.
    pub const fn key(&self) -> LogConfigurationGenerationKey {
        self.key
    }

    /// Returns the compiled-in Log Module identity committed by this generation.
    pub const fn module(&self) -> &Name {
        &self.module
    }

    /// Returns the configuration name committed by this generation.
    pub const fn name(&self) -> &Name {
        &self.name
    }

    /// Returns whether this exact generation is enabled.
    pub const fn enabled(&self) -> bool {
        self.enabled
    }

    /// Returns canonically ordered non-secret settings.
    pub const fn settings(&self) -> &[LogModuleSetting] {
        &self.settings
    }

    /// Returns canonically ordered Log Type memberships.
    pub const fn log_types(&self) -> &[LogType] {
        &self.log_types
    }

    /// Returns whether this generation is assigned to `log_type`.
    pub fn contains_log_type(&self, log_type: LogType) -> bool {
        self.log_types.binary_search(&log_type).is_ok()
    }
}

impl fmt::Debug for LogConfigurationGeneration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("LogConfigurationGeneration(REDACTED)")
    }
}

/// Read-only internal lookup of current and exact historical generations.
pub trait LogConfigurationGenerationStore {
    /// Loads the generation currently assigned to Audit Logs.
    fn load_current_audit_log_configuration_generation(
        &mut self,
        persistence: &LogConfigurationGenerationPersistence,
    ) -> Result<Option<LogConfigurationGeneration>, DatabaseError>;

    /// Loads one exact immutable historical generation.
    fn load_log_configuration_generation(
        &mut self,
        persistence: &LogConfigurationGenerationPersistence,
        key: LogConfigurationGenerationKey,
    ) -> Result<Option<LogConfigurationGeneration>, DatabaseError>;
}

/// One bounded requested Log Module configuration and assignment change.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogConfigurationMutationRequest {
    primary: StateIdentifier,
    enabled: Option<bool>,
    settings: Option<Box<[LogModuleSetting]>>,
    assignments: Box<[LogAssignment]>,
}

impl LogConfigurationMutationRequest {
    /// Creates one canonical non-empty mutation request.
    pub fn new(
        primary: StateIdentifier,
        enabled: Option<bool>,
        mut settings: Option<Vec<LogModuleSetting>>,
        mut assignments: Vec<LogAssignment>,
    ) -> Result<Self, LogConfigurationMutationError> {
        if enabled.is_none() && settings.is_none() && assignments.is_empty() {
            return Err(LogConfigurationMutationError);
        }
        if let Some(settings) = settings.as_mut() {
            settings.sort_by(|left, right| left.key.cmp(&right.key));
            if settings.windows(2).any(|pair| pair[0].key == pair[1].key) {
                return Err(LogConfigurationMutationError);
            }
        }
        assignments.sort();
        if assignments
            .windows(2)
            .any(|pair| pair[0].log_type == pair[1].log_type)
        {
            return Err(LogConfigurationMutationError);
        }
        Ok(Self {
            primary,
            enabled,
            settings: settings.map(Vec::into_boxed_slice),
            assignments: assignments.into_boxed_slice(),
        })
    }

    /// Returns the existing logical configuration anchoring this change.
    pub const fn primary(&self) -> StateIdentifier {
        self.primary
    }

    /// Returns the desired enabled state, when supplied.
    pub const fn enabled(&self) -> Option<bool> {
        self.enabled
    }

    /// Returns the complete desired settings, when supplied.
    pub fn settings(&self) -> Option<&[LogModuleSetting]> {
        self.settings.as_deref()
    }

    /// Returns the canonically ordered desired assignments.
    pub const fn assignments(&self) -> &[LogAssignment] {
        &self.assignments
    }
}

/// One affected configuration's exact pre-change and candidate generations.
#[derive(Clone, Eq, PartialEq)]
pub struct LogConfigurationMutationEntry {
    expected: LogConfigurationGeneration,
    candidate: LogConfigurationGeneration,
}

impl LogConfigurationMutationEntry {
    /// Returns the exact generation observed during preparation.
    pub const fn expected(&self) -> &LogConfigurationGeneration {
        &self.expected
    }

    /// Returns the complete immutable generation to append on commit.
    pub const fn candidate(&self) -> &LogConfigurationGeneration {
        &self.candidate
    }
}

impl fmt::Debug for LogConfigurationMutationEntry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("LogConfigurationMutationEntry(REDACTED)")
    }
}

/// Complete optimistic plan produced by one atomic preparation read.
pub struct PreparedLogConfigurationMutation {
    entries: Box<[LogConfigurationMutationEntry]>,
    expected_assignments: Box<[LogAssignment]>,
    desired_assignments: Box<[LogAssignment]>,
    resultant_generations: Box<[LogConfigurationGeneration]>,
}

impl PreparedLogConfigurationMutation {
    /// Returns affected configurations in stable identity order.
    pub const fn entries(&self) -> &[LogConfigurationMutationEntry] {
        &self.entries
    }

    /// Returns the complete assignment topology observed during preparation.
    pub const fn expected_assignments(&self) -> &[LogAssignment] {
        &self.expected_assignments
    }

    /// Returns the complete assignment topology to persist on commit.
    pub const fn desired_assignments(&self) -> &[LogAssignment] {
        &self.desired_assignments
    }

    /// Returns every resultant current generation in stable identity order.
    pub const fn resultant_generations(&self) -> &[LogConfigurationGeneration] {
        &self.resultant_generations
    }

    /// Returns one configuration's exact generation observed during preparation.
    pub fn expected_generation(
        &self,
        configuration: StateIdentifier,
    ) -> Option<&LogConfigurationGeneration> {
        self.entries
            .iter()
            .find(|entry| entry.expected.key().configuration() == configuration)
            .map(|entry| &entry.expected)
            .or_else(|| {
                self.resultant_generations
                    .iter()
                    .find(|generation| generation.key().configuration() == configuration)
            })
    }
}

impl fmt::Debug for PreparedLogConfigurationMutation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PreparedLogConfigurationMutation(REDACTED)")
    }
}

/// Server-issued authority for constructing a validated optimistic mutation plan.
pub struct LogConfigurationMutationPersistence {
    _private: (),
}

impl LogConfigurationMutationPersistence {
    /// Issues mutation persistence authority to Server-owned database selection.
    #[must_use]
    pub const fn from_server_authority(_authority: &ServerDatabaseAuthority) -> Self {
        Self { _private: () }
    }

    /// Validates exact predecessor/successor and complete-assignment invariants.
    pub fn prepare(
        &self,
        mut entries: Vec<(LogConfigurationGeneration, LogConfigurationGeneration)>,
        mut expected_assignments: Vec<LogAssignment>,
        mut desired_assignments: Vec<LogAssignment>,
        mut resultant_generations: Vec<LogConfigurationGeneration>,
    ) -> Result<PreparedLogConfigurationMutation, LogConfigurationMutationError> {
        entries.sort_by_key(|(expected, _)| expected.key().configuration());
        if entries.is_empty()
            || entries
                .windows(2)
                .any(|pair| pair[0].0.key().configuration() == pair[1].0.key().configuration())
            || entries.iter().any(|(expected, candidate)| {
                expected.key().configuration() != candidate.key().configuration()
                    || expected.key().version().checked_next() != Some(candidate.key().version())
            })
        {
            return Err(LogConfigurationMutationError);
        }
        expected_assignments.sort();
        desired_assignments.sort();
        resultant_generations.sort_by_key(|generation| generation.key().configuration());
        if !complete_assignments(&expected_assignments)
            || !complete_assignments(&desired_assignments)
            || resultant_generations.is_empty()
            || resultant_generations
                .windows(2)
                .any(|pair| pair[0].key().configuration() == pair[1].key().configuration())
            || desired_assignments.iter().any(|assignment| {
                !resultant_generations
                    .iter()
                    .any(|generation| generation.key().configuration() == assignment.configuration)
            })
        {
            return Err(LogConfigurationMutationError);
        }

        Ok(PreparedLogConfigurationMutation {
            entries: entries
                .into_iter()
                .map(|(expected, candidate)| LogConfigurationMutationEntry {
                    expected,
                    candidate,
                })
                .collect(),
            expected_assignments: expected_assignments.into_boxed_slice(),
            desired_assignments: desired_assignments.into_boxed_slice(),
            resultant_generations: resultant_generations.into_boxed_slice(),
        })
    }
}

impl fmt::Debug for LogConfigurationMutationPersistence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("LogConfigurationMutationPersistence(REDACTED)")
    }
}

fn complete_assignments(assignments: &[LogAssignment]) -> bool {
    assignments.len() == LogType::ALL.len()
        && assignments
            .iter()
            .zip(LogType::ALL)
            .all(|(assignment, expected)| assignment.log_type == expected)
}

/// Result of preparing one configuration mutation from one consistent read.
#[derive(Debug)]
pub enum LogConfigurationPreparation {
    /// The desired state is already exact; no Audit record or write is required.
    Unchanged,
    /// One complete optimistic mutation plan is ready for validation and preflight.
    Prepared(PreparedLogConfigurationMutation),
    /// At least one affected configuration cannot allocate another generation.
    VersionExhausted,
    /// The request does not describe a valid mutation of the current topology.
    Invalid,
}

/// Exactly one terminal obligation for either possible commit decision.
pub struct LogConfigurationAuditTerminalWrites<'a> {
    applied: &'a ValidatedAuditTerminalObligationWrite,
    stale: &'a ValidatedAuditTerminalObligationWrite,
}

impl<'a> LogConfigurationAuditTerminalWrites<'a> {
    /// Binds the applied and stale terminal writes before mutation begins.
    #[must_use]
    pub const fn new(
        applied: &'a ValidatedAuditTerminalObligationWrite,
        stale: &'a ValidatedAuditTerminalObligationWrite,
    ) -> Self {
        Self { applied, stale }
    }

    /// Returns the terminal obligation for a committed state change.
    pub const fn applied(&self) -> &ValidatedAuditTerminalObligationWrite {
        self.applied
    }

    /// Returns the terminal obligation for a stale prepared plan.
    pub const fn stale(&self) -> &ValidatedAuditTerminalObligationWrite {
        self.stale
    }
}

impl fmt::Debug for LogConfigurationAuditTerminalWrites<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("LogConfigurationAuditTerminalWrites(REDACTED)")
    }
}

/// Authoritative result of committing one prepared configuration mutation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogConfigurationMutationOutcome {
    /// All candidate generations and mutable state committed atomically.
    Applied { generation_count: usize },
    /// The prepared state was stale; only the stale terminal obligation committed.
    Stale,
}

/// Backend-neutral preparation and atomic commit of Log configuration changes.
pub trait LogConfigurationMutationStore {
    /// Reads current configurations, settings, assignments, and pointers atomically.
    fn prepare_log_configuration_mutation(
        &mut self,
        generation_persistence: &LogConfigurationGenerationPersistence,
        mutation_persistence: &LogConfigurationMutationPersistence,
        request: &LogConfigurationMutationRequest,
    ) -> Result<LogConfigurationPreparation, DatabaseError>;

    /// Rechecks the complete prepared state and commits exactly one terminal outcome.
    fn commit_log_configuration_mutation(
        &mut self,
        mutation: &PreparedLogConfigurationMutation,
        audit_terminals: &LogConfigurationAuditTerminalWrites<'_>,
    ) -> Result<LogConfigurationMutationOutcome, DatabaseError>;
}

/// Payload-free rejection of a malformed mutation request or prepared plan.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LogConfigurationMutationError;

impl fmt::Display for LogConfigurationMutationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Log Module configuration mutation is invalid")
    }
}

impl std::error::Error for LogConfigurationMutationError {}

/// Payload-free rejection of malformed persisted generation structure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogConfigurationGenerationError {
    /// Settings or Log Type memberships are duplicated or not canonically ordered.
    InvalidOrdering,
}

impl fmt::Display for LogConfigurationGenerationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Log Module configuration generation is invalid")
    }
}

impl std::error::Error for LogConfigurationGenerationError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ConfigurationKey, ConfigurationValue};

    fn persistence() -> LogConfigurationGenerationPersistence {
        LogConfigurationGenerationPersistence::from_server_authority(&ServerDatabaseAuthority::new())
    }

    fn setting(key: &str, value: &str) -> LogModuleSetting {
        LogModuleSetting {
            key: ConfigurationKey::new(key).unwrap(),
            value: ConfigurationValue::new(value).unwrap(),
        }
    }

    #[test]
    fn versions_are_nonzero_and_begin_at_one() {
        assert_eq!(LogConfigurationVersion::INITIAL.get(), 1);
        assert_eq!(LogConfigurationVersion::new(0), None);
        assert_eq!(LogConfigurationVersion::new(7).unwrap().get(), 7);
        assert_eq!(
            LogConfigurationVersion::new(7)
                .unwrap()
                .checked_next()
                .unwrap()
                .get(),
            8
        );
        assert_eq!(
            LogConfigurationVersion::new(u64::MAX)
                .unwrap()
                .checked_next(),
            None
        );
    }

    #[test]
    fn persisted_generation_requires_canonical_unique_members() {
        let persistence = persistence();
        let key = persistence.key(
            StateIdentifier::from_bytes([1; 16]).unwrap(),
            LogConfigurationVersion::INITIAL,
        );
        let generation = persistence
            .generation(
                key,
                Name::new("sqlite").unwrap(),
                Name::new("primary").unwrap(),
                true,
                vec![setting("a", "one"), setting("b", "two")],
                vec![LogType::System, LogType::Audit],
            )
            .unwrap();
        assert_eq!(generation.key(), key);
        assert!(generation.contains_log_type(LogType::Audit));

        for error in [
            persistence.generation(
                key,
                Name::new("sqlite").unwrap(),
                Name::new("primary").unwrap(),
                true,
                vec![setting("a", "one"), setting("a", "two")],
                vec![LogType::Audit],
            ),
            persistence.generation(
                key,
                Name::new("sqlite").unwrap(),
                Name::new("primary").unwrap(),
                true,
                Vec::new(),
                vec![LogType::Audit, LogType::Audit],
            ),
        ] {
            assert_eq!(
                error.unwrap_err(),
                LogConfigurationGenerationError::InvalidOrdering
            );
        }
    }

    #[test]
    fn generation_diagnostics_expose_no_identity_or_settings() {
        let persistence = persistence();
        let key = persistence.key(
            StateIdentifier::from_bytes([0x5a; 16]).unwrap(),
            LogConfigurationVersion::new(42).unwrap(),
        );
        let generation = persistence
            .generation(
                key,
                Name::new("sensitive-module").unwrap(),
                Name::new("sensitive-configuration").unwrap(),
                true,
                vec![setting("endpoint", "sensitive-setting")],
                vec![LogType::Audit],
            )
            .unwrap();
        let rendered = format!("{key:?} {generation:?}");
        for forbidden in [
            "5a5a",
            "42",
            "sensitive-module",
            "sensitive-configuration",
            "sensitive-setting",
        ] {
            assert!(!rendered.contains(forbidden), "{rendered}");
        }
    }
}
