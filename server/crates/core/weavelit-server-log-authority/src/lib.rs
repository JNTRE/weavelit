//! Server-owned capability key for minting trusted logging authority.
//!
//! Rust has no cross-crate friend visibility, so the log contract cannot make
//! its authority-minting constructors reachable from Server Audit and Server
//! Observability while keeping them unreachable from a Log Module. This crate
//! supplies the missing distinction: possessing a [`ServerLogAuthority`] is the
//! capability, and obtaining one requires an explicit dependency edge on this
//! package that is visible in a manifest and reviewable in isolation.
//!
//! A Log Module depends only on the log contract, so it cannot construct this
//! value, and the log contract's compile fixtures continue to prove that every
//! authority-minting attempt from an ordinary external consumer fails to
//! compile.

#![forbid(unsafe_code)]

/// Capability proving the holder is Server-owned logging authority.
///
/// This type carries no data; its construction is the privilege.
#[derive(Debug)]
pub struct ServerLogAuthority {
    _private: (),
}

impl ServerLogAuthority {
    /// Creates the capability held by Server runtime composition.
    #[must_use]
    pub const fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for ServerLogAuthority {
    fn default() -> Self {
        Self::new()
    }
}
