//! Server-owned capability for establishing administration-session authority.
//!
//! Rust has no cross-crate friend visibility. Possessing this capability marks
//! the narrow Server-runtime dependency edge allowed to bind a validated
//! session to an administration authorization and to mint a proof only after
//! current-session MFA verification succeeds.

#![forbid(unsafe_code)]

/// Capability held only by trusted Server administration composition.
#[derive(Debug)]
pub struct ServerAdministrationAuthority {
    _private: (),
}

impl ServerAdministrationAuthority {
    /// Creates the capability held by Server runtime composition.
    #[must_use]
    pub const fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for ServerAdministrationAuthority {
    fn default() -> Self {
        Self::new()
    }
}
