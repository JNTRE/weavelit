//! Live session records held outside restorable application state.
//!
//! A session is live operational data. It is not part of [`crate::ApplicationState`],
//! so it cannot enter a checkpoint, a normalized backup, or restored state.
//! It survives an ordinary Server restart because it is durable, and it is
//! cleared inside the one atomic state replacement a Restore performs.
//!
//! Only digests are representable here. Neither of the stored digest types can
//! be built from text or from a variable-length byte sequence, so an encoded
//! session or CSRF bearer value has no constructor that reaches persistence.

use std::fmt;

use subtle::ConstantTimeEq as _;

use crate::{ContractInputError, DatabaseError, Name, StateIdentifier};

/// Bytes in a stored session or CSRF digest.
pub const SESSION_DIGEST_LENGTH: usize = 32;

/// Idle timeout applied to a session's last observed activity.
pub const SESSION_IDLE_TIMEOUT_MILLISECONDS: i64 = 30 * 60 * 1_000;

/// Absolute maximum lifetime measured from the moment a session is issued.
pub const SESSION_ABSOLUTE_LIFETIME_MILLISECONDS: i64 = 12 * 60 * 60 * 1_000;

/// Largest accepted session instant.
///
/// Bounded so that adding [`SESSION_ABSOLUTE_LIFETIME_MILLISECONDS`] to an
/// accepted instant is always representable and the absolute expiry can never
/// wrap into the past.
pub const MAX_SESSION_INSTANT_MILLISECONDS: i64 = i64::MAX - SESSION_ABSOLUTE_LIFETIME_MILLISECONDS;

macro_rules! stored_digest {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        ///
        /// The only constructor takes exactly [`SESSION_DIGEST_LENGTH`] bytes,
        /// so an encoded bearer value cannot be built into this type. The type
        /// implements neither `PartialEq` nor `Display`, so the only reachable
        /// comparison is the constant-time [`Self::matches`] and no code path
        /// can render it.
        #[derive(Clone, Copy)]
        pub struct $name([u8; SESSION_DIGEST_LENGTH]);

        impl $name {
            /// Creates a stored digest and rejects the reserved all-zero value.
            pub fn from_bytes(
                bytes: [u8; SESSION_DIGEST_LENGTH],
            ) -> Result<Self, ContractInputError> {
                if bytes == [0; SESSION_DIGEST_LENGTH] {
                    return Err(ContractInputError::InvalidSessionDigest);
                }

                Ok(Self(bytes))
            }

            /// Returns the digest bytes for persistence.
            pub const fn as_bytes(&self) -> &[u8; SESSION_DIGEST_LENGTH] {
                &self.0
            }

            /// Compares two digests without a data-dependent branch.
            pub fn matches(&self, other: &Self) -> bool {
                self.0.ct_eq(&other.0).into()
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(concat!(stringify!($name), "(REDACTED)"))
            }
        }
    };
}

stored_digest!(
    SessionTokenHash,
    "The stored digest of one session bearer token."
);
stored_digest!(
    SessionCsrfHash,
    "The stored digest of one per-session CSRF token."
);

/// A bounded UTC Unix millisecond instant used by session lifetime arithmetic.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SessionInstant(i64);

impl SessionInstant {
    /// Creates an instant and rejects negative or unbounded values.
    pub fn from_unix_milliseconds(value: i64) -> Result<Self, ContractInputError> {
        if !(0..=MAX_SESSION_INSTANT_MILLISECONDS).contains(&value) {
            return Err(ContractInputError::InvalidSessionInstant);
        }

        Ok(Self(value))
    }

    /// Returns the instant in UTC Unix milliseconds.
    pub const fn as_unix_milliseconds(self) -> i64 {
        self.0
    }

    /// Returns this instant advanced by a non-negative bounded duration.
    ///
    /// The accepted instant range makes every offset up to the absolute
    /// lifetime total, so this cannot overflow for the offsets this module
    /// applies.
    const fn plus(self, milliseconds: i64) -> Self {
        Self(self.0.saturating_add(milliseconds))
    }
}

/// A session about to be created, whose absolute expiry is derived, not supplied.
#[derive(Debug)]
pub struct NewSession {
    token_hash: SessionTokenHash,
    csrf_hash: SessionCsrfHash,
    account: StateIdentifier,
    client_module: Name,
    issued_at: SessionInstant,
}

impl NewSession {
    /// Creates the record for one newly issued session.
    ///
    /// There is deliberately no absolute-expiry parameter. The expiry is
    /// derived from `issued_at` here and nowhere else, so no caller can choose,
    /// lengthen, or refresh it.
    pub const fn new(
        token_hash: SessionTokenHash,
        csrf_hash: SessionCsrfHash,
        account: StateIdentifier,
        client_module: Name,
        issued_at: SessionInstant,
    ) -> Self {
        Self {
            token_hash,
            csrf_hash,
            account,
            client_module,
            issued_at,
        }
    }

    /// Returns the stored session-token digest.
    pub const fn token_hash(&self) -> &SessionTokenHash {
        &self.token_hash
    }

    /// Returns the stored CSRF-token digest.
    pub const fn csrf_hash(&self) -> &SessionCsrfHash {
        &self.csrf_hash
    }

    /// Returns the account the session authenticates.
    pub const fn account(&self) -> StateIdentifier {
        self.account
    }

    /// Returns the Client Module the session was issued to.
    pub const fn client_module(&self) -> &Name {
        &self.client_module
    }

    /// Returns the moment the session was issued.
    pub const fn issued_at(&self) -> SessionInstant {
        self.issued_at
    }

    /// Returns the derived immutable absolute expiry.
    pub const fn absolute_expires_at(&self) -> SessionInstant {
        self.issued_at.plus(SESSION_ABSOLUTE_LIFETIME_MILLISECONDS)
    }
}

/// A stored session read back from the session store.
#[derive(Debug)]
pub struct StoredSession {
    csrf_hash: SessionCsrfHash,
    account: StateIdentifier,
    client_module: Name,
    issued_at: SessionInstant,
    last_seen_at: SessionInstant,
    absolute_expires_at: SessionInstant,
}

impl StoredSession {
    /// Rebuilds a stored session from its persisted fields.
    pub const fn new(
        csrf_hash: SessionCsrfHash,
        account: StateIdentifier,
        client_module: Name,
        issued_at: SessionInstant,
        last_seen_at: SessionInstant,
        absolute_expires_at: SessionInstant,
    ) -> Self {
        Self {
            csrf_hash,
            account,
            client_module,
            issued_at,
            last_seen_at,
            absolute_expires_at,
        }
    }

    /// Returns the stored CSRF-token digest.
    pub const fn csrf_hash(&self) -> &SessionCsrfHash {
        &self.csrf_hash
    }

    /// Returns the account the session authenticates.
    pub const fn account(&self) -> StateIdentifier {
        self.account
    }

    /// Returns the Client Module the session was issued to.
    pub const fn client_module(&self) -> &Name {
        &self.client_module
    }

    /// Returns the moment the session was issued.
    pub const fn issued_at(&self) -> SessionInstant {
        self.issued_at
    }

    /// Returns the last moment activity was observed on the session.
    pub const fn last_seen_at(&self) -> SessionInstant {
        self.last_seen_at
    }

    /// Returns the immutable absolute expiry set when the session was issued.
    pub const fn absolute_expires_at(&self) -> SessionInstant {
        self.absolute_expires_at
    }

    /// Returns why the session is unusable at `now`, or `None` when it is usable.
    ///
    /// A clock that has moved backwards is reported before any lifetime
    /// arithmetic runs, so a rolled-back clock can never make an expired
    /// session look fresh.
    pub fn rejection_at(&self, now: SessionInstant) -> Option<SessionRejection> {
        if now < self.issued_at || now < self.last_seen_at {
            return Some(SessionRejection::ClockRollback);
        }
        if now >= self.absolute_expires_at {
            return Some(SessionRejection::AbsoluteLifetime);
        }
        if now >= self.last_seen_at.plus(SESSION_IDLE_TIMEOUT_MILLISECONDS) {
            return Some(SessionRejection::IdleTimeout);
        }

        None
    }
}

/// Why a session presented to the store is not usable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionRejection {
    /// No session is stored for the presented digest.
    Unknown,
    /// The idle timeout elapsed since the last observed activity.
    IdleTimeout,
    /// The immutable absolute lifetime elapsed.
    AbsoluteLifetime,
    /// The clock moved backwards past the session's own recorded times.
    ClockRollback,
}

/// The result of presenting a session digest to the store.
#[derive(Debug)]
pub enum SessionValidation {
    /// The session is usable and its recorded activity has been advanced.
    Valid(StoredSession),
    /// The session is not usable and nothing was advanced.
    Rejected(SessionRejection),
}

/// Durable live-session operations available during normal operation.
///
/// Every operation is atomic on its own. None of them reads, caches, or
/// returns Groups, grants, or any other authorization data; authorization is
/// evaluated live from application state by its owning boundary.
pub trait SessionStore {
    /// Stores one newly issued session.
    fn create(&mut self, session: &NewSession) -> Result<(), DatabaseError>;

    /// Validates a presented session and advances its activity when usable.
    ///
    /// An expired session is removed in the same transaction that rejects it.
    /// A session rejected for a backwards clock is left untouched.
    fn validate_and_touch(
        &mut self,
        token_hash: &SessionTokenHash,
        now: SessionInstant,
    ) -> Result<SessionValidation, DatabaseError>;

    /// Replaces a usable session's CSRF digest and advances its activity.
    ///
    /// A session that is not usable is neither rotated nor advanced.
    fn rotate_csrf(
        &mut self,
        token_hash: &SessionTokenHash,
        csrf_hash: &SessionCsrfHash,
        now: SessionInstant,
    ) -> Result<SessionValidation, DatabaseError>;

    /// Removes one session and reports whether it existed.
    fn revoke(&mut self, token_hash: &SessionTokenHash) -> Result<bool, DatabaseError>;

    /// Removes every session belonging to one account and reports the count.
    fn revoke_for_account(&mut self, account: StateIdentifier) -> Result<usize, DatabaseError>;

    /// Removes every session already expired at `now` and reports the count.
    fn purge_expired(&mut self, now: SessionInstant) -> Result<usize, DatabaseError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    const SESSION_DIGEST: [u8; SESSION_DIGEST_LENGTH] = [7; SESSION_DIGEST_LENGTH];
    const CSRF_DIGEST: [u8; SESSION_DIGEST_LENGTH] = [9; SESSION_DIGEST_LENGTH];

    fn account() -> StateIdentifier {
        StateIdentifier::from_bytes([3; 16]).unwrap()
    }

    fn instant(value: i64) -> SessionInstant {
        SessionInstant::from_unix_milliseconds(value).unwrap()
    }

    fn stored(issued_at: i64, last_seen_at: i64) -> StoredSession {
        StoredSession::new(
            SessionCsrfHash::from_bytes(CSRF_DIGEST).unwrap(),
            account(),
            Name::new("web-ui").unwrap(),
            instant(issued_at),
            instant(last_seen_at),
            instant(issued_at + SESSION_ABSOLUTE_LIFETIME_MILLISECONDS),
        )
    }

    #[test]
    fn a_digest_accepts_only_a_full_length_value_and_rejects_the_reserved_zero() {
        let digest = SessionTokenHash::from_bytes(SESSION_DIGEST).unwrap();
        assert_eq!(digest.as_bytes(), &SESSION_DIGEST);
        assert!(digest.matches(&SessionTokenHash::from_bytes(SESSION_DIGEST).unwrap()));
        assert!(!digest.matches(&SessionTokenHash::from_bytes(CSRF_DIGEST).unwrap()));

        let error = SessionTokenHash::from_bytes([0; SESSION_DIGEST_LENGTH])
            .expect_err("the all-zero digest must be rejected");
        assert_eq!(error, ContractInputError::InvalidSessionDigest);
        let error = SessionCsrfHash::from_bytes([0; SESSION_DIGEST_LENGTH])
            .expect_err("the all-zero digest must be rejected");
        assert_eq!(error, ContractInputError::InvalidSessionDigest);
    }

    #[test]
    fn a_digest_never_renders_its_value() {
        let token = SessionTokenHash::from_bytes(SESSION_DIGEST).unwrap();
        let csrf = SessionCsrfHash::from_bytes(CSRF_DIGEST).unwrap();

        assert_eq!(
            format!("{token:?} {csrf:?}"),
            "SessionTokenHash(REDACTED) SessionCsrfHash(REDACTED)"
        );
    }

    #[test]
    fn an_instant_rejects_negative_and_unbounded_values() {
        assert_eq!(instant(0).as_unix_milliseconds(), 0);
        assert_eq!(
            instant(MAX_SESSION_INSTANT_MILLISECONDS).as_unix_milliseconds(),
            MAX_SESSION_INSTANT_MILLISECONDS
        );
        assert_eq!(
            SessionInstant::from_unix_milliseconds(-1),
            Err(ContractInputError::InvalidSessionInstant)
        );
        assert_eq!(
            SessionInstant::from_unix_milliseconds(MAX_SESSION_INSTANT_MILLISECONDS + 1),
            Err(ContractInputError::InvalidSessionInstant)
        );
        assert_eq!(
            SessionInstant::from_unix_milliseconds(i64::MAX),
            Err(ContractInputError::InvalidSessionInstant)
        );
    }

    #[test]
    fn the_absolute_expiry_is_derived_from_the_issue_time_alone() {
        let session = NewSession::new(
            SessionTokenHash::from_bytes(SESSION_DIGEST).unwrap(),
            SessionCsrfHash::from_bytes(CSRF_DIGEST).unwrap(),
            account(),
            Name::new("web-ui").unwrap(),
            instant(1_700_000_000_000),
        );

        assert_eq!(
            session.absolute_expires_at().as_unix_milliseconds(),
            1_700_000_000_000 + SESSION_ABSOLUTE_LIFETIME_MILLISECONDS
        );
        assert_eq!(
            session.absolute_expires_at().as_unix_milliseconds()
                - session.issued_at().as_unix_milliseconds(),
            SESSION_ABSOLUTE_LIFETIME_MILLISECONDS
        );
    }

    #[test]
    fn the_idle_boundary_is_exact() {
        let issued_at = 1_000_000;
        let last_seen_at = issued_at + 60_000;
        let session = stored(issued_at, last_seen_at);
        let idle_expiry = last_seen_at + SESSION_IDLE_TIMEOUT_MILLISECONDS;

        assert_eq!(session.rejection_at(instant(idle_expiry - 1)), None);
        assert_eq!(
            session.rejection_at(instant(idle_expiry)),
            Some(SessionRejection::IdleTimeout)
        );
    }

    #[test]
    fn the_absolute_boundary_is_exact_and_activity_cannot_extend_it() {
        let issued_at = 1_000_000;
        let absolute_expiry = issued_at + SESSION_ABSOLUTE_LIFETIME_MILLISECONDS;
        // Activity one millisecond before the absolute expiry, which is the
        // most recent activity the absolute bound permits.
        let session = stored(issued_at, absolute_expiry - 1);

        assert_eq!(session.rejection_at(instant(absolute_expiry - 1)), None);
        assert_eq!(
            session.rejection_at(instant(absolute_expiry)),
            Some(SessionRejection::AbsoluteLifetime)
        );
    }

    #[test]
    fn a_backwards_clock_is_rejected_before_any_lifetime_arithmetic() {
        let issued_at = 1_000_000;
        let last_seen_at = issued_at + 60_000;
        let session = stored(issued_at, last_seen_at);

        assert_eq!(
            session.rejection_at(instant(issued_at - 1)),
            Some(SessionRejection::ClockRollback)
        );
        assert_eq!(
            session.rejection_at(instant(last_seen_at - 1)),
            Some(SessionRejection::ClockRollback)
        );
        assert_eq!(session.rejection_at(instant(last_seen_at)), None);

        // A clock far enough back that naive idle arithmetic would report a
        // fresh session must still be rejected.
        let expired = stored(issued_at, issued_at);
        assert_eq!(
            expired.rejection_at(instant(0)),
            Some(SessionRejection::ClockRollback)
        );
    }

    #[test]
    fn the_approved_lifetime_profile_is_pinned() {
        assert_eq!(SESSION_IDLE_TIMEOUT_MILLISECONDS, 1_800_000);
        assert_eq!(SESSION_ABSOLUTE_LIFETIME_MILLISECONDS, 43_200_000);
    }
}
