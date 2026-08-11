#![forbid(unsafe_code)]

//! Server-owned local password authentication and session secret material.
//!
//! This crate owns the authentication core: the approved Argon2id profile, the
//! closed allowlist of profiles a stored verifier may be attempted against,
//! the equal-work password decision, and generation and hashing of session and
//! CSRF bearer values. It does not read or write the Application Database, does
//! not touch the transport or the listener, and belongs to no Client Module; a
//! caller supplies the stored credential as a value and persists what this
//! crate returns.

mod continuation;
mod engine;
mod error;
mod password;
mod phc;
mod profile;
mod random;
mod session;

pub use continuation::{
    CONTINUATION_ENTROPY_BYTES, CONTINUATION_TEXT_BYTES, Continuation, ContinuationDigest,
};
pub use engine::{Argon2Engine, RustCryptoArgon2};
pub use error::AuthenticationError;
pub use password::{PasswordAuthenticator, PasswordVerdict, ReplacementVerifier, StoredCredential};
pub use profile::{
    ACCEPTED_ARGON2_PROFILES, Argon2Profile, CURRENT_ARGON2_PROFILE, MAX_VERIFICATION_MEMORY_KIB,
    PasswordPolicy,
};
pub use session::{
    CsrfToken, CsrfTokenDigest, SESSION_TOKEN_ENTROPY_BYTES, SESSION_TOKEN_TEXT_BYTES,
    SessionSecrets, SessionToken, SessionTokenDigest,
};
