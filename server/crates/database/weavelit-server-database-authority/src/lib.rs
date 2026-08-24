//! Server-owned capability key for persisted Application Database decoding.
//!
//! Rust has no cross-crate friend visibility, so the database contract cannot
//! expose decoder issuance to lifecycle selection while hiding it from an
//! arbitrary backend implementor. Possessing a [`ServerDatabaseAuthority`] is
//! the capability, and obtaining one requires an explicit dependency on this
//! unpublished package that remains visible in manifest review.

#![forbid(unsafe_code)]

/// Capability proving the holder is Server-owned database selection authority.
///
/// This type carries no data; its construction is the privilege.
#[derive(Debug)]
pub struct ServerDatabaseAuthority {
    _private: (),
}

impl ServerDatabaseAuthority {
    /// Creates the capability held by lifecycle database selection.
    #[must_use]
    pub const fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for ServerDatabaseAuthority {
    fn default() -> Self {
        Self::new()
    }
}
