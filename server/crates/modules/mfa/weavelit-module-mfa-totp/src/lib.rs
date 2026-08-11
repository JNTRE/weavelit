#![forbid(unsafe_code)]

//! Compiled-in TOTP MFA Module: the RFC 6238 profile, one-time provisioning
//! data, and code verification.
//!
//! The Server owns MFA policy, module enablement, session usability, recovery,
//! and audit records. This crate owns only the TOTP method itself: the
//! approved profile, the Base32 secret and `otpauth://` URI a user is shown
//! exactly once, and the decision that a submitted code matches a secret at a
//! caller-supplied instant.
//!
//! Nothing here reads a clock. Verification takes the instant as an argument,
//! so the module's decision is reproducible and the Server owns the one place
//! time enters authentication.

mod provisioning;
mod secret;

pub use provisioning::{ProvisioningError, ProvisioningText};
pub use secret::{TimeStep, TotpSecret};

/// The canonical identifier this MFA Module is compiled in and registered under.
///
/// It is the single source of the Server's compiled-in TOTP Module inventory,
/// so a stored factor cannot be judged against a module name the runtime
/// restated by hand.
pub const MODULE_IDENTIFIER: &str = "totp";

/// The profile's hash algorithm, named as an `otpauth://` URI writes it.
pub const ALGORITHM: &str = "SHA1";

/// Decimal digits in one generated code.
pub const DIGITS: u8 = 6;

/// Seconds in one RFC 6238 time step.
///
/// Steps are counted from `T0 = 0`, so a step number is the Unix second
/// divided by this period and no deployment-specific epoch exists.
pub const STEP_SECONDS: u64 = 30;

/// Steps accepted on either side of the step a code is presented in.
pub const SKEW_STEPS: u16 = 1;

/// Bytes in a secret, being the RFC 4226 recommended 160 bits.
pub const SECRET_LENGTH: usize = 20;

/// The compiled-in TOTP MFA Module registration.
///
/// The registration restates no value of its own: every field is one of this
/// module's profile constants, so the inventory the Server registers and the
/// profile verification runs cannot drift apart.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TotpModuleRegistration {
    identifier: &'static str,
    algorithm: &'static str,
    digits: u8,
    step_seconds: u64,
    skew_steps: u16,
    secret_length: usize,
}

impl TotpModuleRegistration {
    /// Returns the module identifier factors are stored under.
    #[must_use]
    pub const fn identifier(&self) -> &'static str {
        self.identifier
    }

    /// Returns the profile's hash algorithm name.
    #[must_use]
    pub const fn algorithm(&self) -> &'static str {
        self.algorithm
    }

    /// Returns the number of digits in one code.
    #[must_use]
    pub const fn digits(&self) -> u8 {
        self.digits
    }

    /// Returns the length of one time step in seconds.
    #[must_use]
    pub const fn step_seconds(&self) -> u64 {
        self.step_seconds
    }

    /// Returns how many steps on either side of the presented step are accepted.
    #[must_use]
    pub const fn skew_steps(&self) -> u16 {
        self.skew_steps
    }

    /// Returns the required secret length in bytes.
    #[must_use]
    pub const fn secret_length(&self) -> usize {
        self.secret_length
    }
}

/// Returns the compiled-in TOTP MFA Module registration.
#[must_use]
pub const fn registration() -> TotpModuleRegistration {
    TotpModuleRegistration {
        identifier: MODULE_IDENTIFIER,
        algorithm: ALGORITHM,
        digits: DIGITS,
        step_seconds: STEP_SECONDS,
        skew_steps: SKEW_STEPS,
        secret_length: SECRET_LENGTH,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_registration_states_the_approved_rfc_6238_profile() {
        let registration = registration();

        assert_eq!(registration.identifier(), "totp");
        assert_eq!(registration.algorithm(), "SHA1");
        assert_eq!(registration.digits(), 6);
        assert_eq!(registration.step_seconds(), 30);
        assert_eq!(registration.skew_steps(), 1);
        assert_eq!(registration.secret_length(), 20);
    }
}
