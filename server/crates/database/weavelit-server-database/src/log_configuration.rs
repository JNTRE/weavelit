//! Immutable, backend-neutral Log Module configuration generation reads.

use std::{fmt, num::NonZeroU64};

use weavelit_server_database_authority::ServerDatabaseAuthority;

use crate::{DatabaseError, LogModuleSetting, LogType, Name, StateIdentifier};

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
