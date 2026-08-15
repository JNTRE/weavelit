#![forbid(unsafe_code)]

//! The compiled-in component inventory shared by every pre-operational workflow.
//!
//! A build serves only the Client, MFA, Service, and Log Modules and named
//! Operations its binary compiles in. Init and Restore both judge submitted
//! state against that inventory, so the inventory is a neutral value both
//! workflow crates depend on rather than a type owned by either of them. The
//! runtime derives it once from the module crates themselves and supplies it as
//! an inbound value; this crate owns only its representation.

use std::collections::{BTreeMap, BTreeSet};

use weavelit_server_database::Name;

/// The shape of the stored factor data one MFA Module can open.
///
/// The value is declared by the module crate that supplies the module, exactly
/// as its name is, so a workflow judging stored state against the inventory
/// judges it against the format the compiled-in module actually reads rather
/// than against a length restated by the workflow.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MfaFactorFormat {
    /// Exact decrypted factor-data length, in bytes, the module can open.
    pub factor_data_bytes: usize,
}

impl MfaFactorFormat {
    /// Returns whether `factor_data` is a value the module can open.
    #[must_use]
    pub fn accepts(&self, factor_data: &[u8]) -> bool {
        factor_data.len() == self.factor_data_bytes
    }
}

/// Compiled-in components a pre-operational workflow may reference.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AvailableComponents {
    /// Compiled-in Client Modules.
    pub client_modules: BTreeSet<Name>,
    /// Compiled-in MFA Modules, each with the factor-data format it declares.
    pub mfa_modules: BTreeMap<Name, MfaFactorFormat>,
    /// Compiled-in Service Modules.
    pub service_modules: BTreeSet<Name>,
    /// Compiled-in Log Modules.
    pub log_modules: BTreeSet<Name>,
    /// Named Operations exposed by compiled-in modules.
    pub operations: BTreeSet<Name>,
}

impl AvailableComponents {
    /// Returns whether the named Client Module is compiled in.
    #[must_use]
    pub fn has_client_module(&self, module: &Name) -> bool {
        self.client_modules.contains(module)
    }

    /// Returns whether the named MFA Module is compiled in.
    #[must_use]
    pub fn has_mfa_module(&self, module: &Name) -> bool {
        self.mfa_modules.contains_key(module)
    }

    /// Returns the factor-data format the named MFA Module declares.
    #[must_use]
    pub fn mfa_factor_format(&self, module: &Name) -> Option<MfaFactorFormat> {
        self.mfa_modules.get(module).copied()
    }

    /// Returns whether the named Service Module is compiled in.
    #[must_use]
    pub fn has_service_module(&self, module: &Name) -> bool {
        self.service_modules.contains(module)
    }

    /// Returns whether the named Log Module is compiled in.
    #[must_use]
    pub fn has_log_module(&self, module: &Name) -> bool {
        self.log_modules.contains(module)
    }

    /// Returns whether the named Operation is exposed by a compiled-in module.
    #[must_use]
    pub fn has_operation(&self, operation: &Name) -> bool {
        self.operations.contains(operation)
    }
}

#[cfg(test)]
mod tests {
    use super::{AvailableComponents, MfaFactorFormat};
    use weavelit_server_database::Name;

    fn name(value: &str) -> Name {
        Name::new(value).expect("the test name must be accepted")
    }

    fn format(bytes: usize) -> MfaFactorFormat {
        MfaFactorFormat {
            factor_data_bytes: bytes,
        }
    }

    #[test]
    fn an_empty_inventory_reports_no_component_as_available() {
        let components = AvailableComponents::default();
        assert!(!components.has_client_module(&name("web-ui")));
        assert!(!components.has_mfa_module(&name("totp")));
        assert!(components.mfa_factor_format(&name("totp")).is_none());
        assert!(!components.has_service_module(&name("zendesk")));
        assert!(!components.has_log_module(&name("sqlite")));
        assert!(!components.has_operation(&name("ticket-search")));
    }

    #[test]
    fn an_inventory_reports_only_the_components_it_was_built_with() {
        let components = AvailableComponents {
            client_modules: [name("web-ui")].into_iter().collect(),
            mfa_modules: [(name("totp"), format(20))].into_iter().collect(),
            log_modules: [name("sqlite")].into_iter().collect(),
            ..AvailableComponents::default()
        };

        assert!(components.has_client_module(&name("web-ui")));
        assert!(components.has_mfa_module(&name("totp")));
        assert!(components.has_log_module(&name("sqlite")));
        assert!(!components.has_service_module(&name("zendesk")));
        assert!(!components.has_operation(&name("ticket-search")));
        assert!(!components.has_client_module(&name("cli")));
    }

    #[test]
    fn an_inventory_reports_the_factor_format_its_mfa_module_declares() {
        let components = AvailableComponents {
            mfa_modules: [(name("totp"), format(20))].into_iter().collect(),
            ..AvailableComponents::default()
        };

        let declared = components
            .mfa_factor_format(&name("totp"))
            .expect("a compiled-in MFA Module declares its factor format");
        assert!(declared.accepts(&[0; 20]));
        assert!(!declared.accepts(&[0; 19]));
        assert!(!declared.accepts(&[0; 21]));
        assert!(!declared.accepts(&[]));
        assert!(components.mfa_factor_format(&name("webauthn")).is_none());
    }
}
