#![forbid(unsafe_code)]

//! Operation execution boundary for the Weavelit Server.
//!
//! This crate owns the two steps that follow an authorization decision on the
//! operational request path: selecting the Service Connection an authorized
//! Operation runs against, and executing the provider for that selection.
//!
//! It exists as its own crate so the ordering it enforces is structural. It
//! depends on the authorization proof and on the Application Database's
//! Service Connection contract, and on nothing else, so no transport, Client
//! Module, or Service Module can reach provider execution without first moving
//! an authorization proof through it.

pub mod selection;

pub use selection::SelectedServiceConnection;
