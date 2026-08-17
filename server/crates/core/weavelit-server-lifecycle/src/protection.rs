//! Server-owned at-rest protection for Application Database secrets.
//!
//! The deployment anchor key is the single 256-bit key that protects
//! deployment-local secrets, so sealing reuses it rather than deriving a second
//! key hierarchy. The key never leaves this crate: a caller receives a
//! seal-only capability and the sealed value's opaque bytes.

use weavelit_server_database::ProtectedValue;
use zeroize::Zeroizing;

use crate::{LifecycleError, format::PROTECTED_PLAINTEXT_LIMIT};

/// Maximum bytes accepted in one value submitted for at-rest protection.
///
/// A sealed value must fit the Application Database's protected-value bound
/// after authenticated encryption and envelope encoding, so the accepted
/// plaintext is bounded below that limit.
pub const MAX_PROTECTED_PLAINTEXT_BYTES: usize = PROTECTED_PLAINTEXT_LIMIT;

/// What a protected value holds.
///
/// The kind is bound into the sealed value as additional authenticated data, so
/// a value sealed for one purpose cannot be replayed as another.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ProtectedValueKind {
    /// A component configuration secret.
    ComponentSecret,
    /// MFA Module-owned enrolled factor data.
    MfaFactorData,
    /// A Service Connection credential.
    ServiceConnectionCredential,
}

impl ProtectedValueKind {
    /// Returns the stable label bound into the sealed value.
    ///
    /// These labels are persisted through the additional authenticated data of
    /// every sealed value, so an existing label is never renamed or reused.
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::ComponentSecret => "component-secret",
            Self::MfaFactorData => "mfa-factor-data",
            Self::ServiceConnectionCredential => "service-connection-credential",
        }
    }
}

/// Capability to protect a secret under the deployment's at-rest key.
///
/// This capability seals only. It exposes no key material and no way to recover
/// plaintext, so holding it does not grant the ability to read stored secrets.
pub trait ProtectedValueSealer {
    /// Seals one bounded plaintext for storage as an opaque protected value.
    ///
    /// Each call uses a fresh nonce, so sealing equal plaintexts twice yields
    /// distinct values.
    ///
    /// # Errors
    ///
    /// Returns [`LifecycleError::IntegrityFailure`] when the plaintext is empty,
    /// exceeds [`MAX_PROTECTED_PLAINTEXT_BYTES`], or cannot be sealed.
    fn seal(
        &self,
        kind: ProtectedValueKind,
        plaintext: &[u8],
    ) -> Result<ProtectedValue, LifecycleError>;
}

/// Capability to recover a secret this deployment previously sealed.
///
/// The recovered plaintext is returned in zeroizing form and the key stays
/// inside this crate exactly as it does for sealing, so holding this capability
/// grants no access to the key itself.
pub trait ProtectedValueOpener {
    /// Recovers one sealed value that was sealed for this exact kind.
    ///
    /// # Errors
    ///
    /// Returns [`LifecycleError::IntegrityFailure`] when the envelope is
    /// malformed, was sealed under another key, or was sealed for another
    /// kind. The failure never reports which of those it was.
    fn open(
        &self,
        kind: ProtectedValueKind,
        value: &ProtectedValue,
    ) -> Result<Zeroizing<Vec<u8>>, LifecycleError>;
}

/// Both at-rest capabilities as one shareable object.
///
/// An enrolled factor is sealed when it is confirmed and opened when a code is
/// verified, so the runtime that owns both operations holds one capability
/// rather than two that could be wired to different keys.
pub trait ProtectedValueAccess: ProtectedValueSealer + ProtectedValueOpener + Send + Sync {}

impl<T: ProtectedValueSealer + ProtectedValueOpener + Send + Sync + ?Sized> ProtectedValueAccess
    for T
{
}
