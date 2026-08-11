#![forbid(unsafe_code)]

//! The compiled-in component inventory shared by every pre-operational workflow.
//!
//! A build serves only the Client, MFA, Service, and Log Modules and named
//! Operations its binary compiles in. Init and Restore both judge submitted
//! state against that inventory, so the inventory is a neutral value both
//! workflow crates depend on rather than a type owned by either of them. The
//! runtime derives it once from the module crates themselves and supplies it as
//! an inbound value; this crate owns only its representation.

use std::collections::BTreeSet;

use weavelit_server_database::Name;

/// Compiled-in components a pre-operational workflow may reference.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AvailableComponents {
    /// Compiled-in Client Modules.
    pub client_modules: BTreeSet<Name>,
    /// Compiled-in MFA Modules.
    pub mfa_modules: BTreeSet<Name>,
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
        self.mfa_modules.contains(module)
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
    use super::AvailableComponents;
    use weavelit_server_database::Name;

    fn name(value: &str) -> Name {
        Name::new(value).expect("the test name must be accepted")
    }

    #[test]
    fn an_empty_inventory_reports_no_component_as_available() {
        let components = AvailableComponents::default();
        assert!(!components.has_client_module(&name("web-ui")));
        assert!(!components.has_mfa_module(&name("totp")));
        assert!(!components.has_service_module(&name("zendesk")));
        assert!(!components.has_log_module(&name("sqlite")));
        assert!(!components.has_operation(&name("ticket-search")));
    }

    #[test]
    fn an_inventory_reports_only_the_components_it_was_built_with() {
        let components = AvailableComponents {
            client_modules: [name("web-ui")].into_iter().collect(),
            mfa_modules: [name("totp")].into_iter().collect(),
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
}
