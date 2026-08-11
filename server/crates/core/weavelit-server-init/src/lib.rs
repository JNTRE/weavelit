//! Server-owned new-state Init workflow.
//!
//! This crate owns exactly four things: the normalized `InitializeServer`
//! request, its Server semantic validation, the initial recovery-key generation
//! and one-time delivery with the proof that confirms it, and the atomic
//! construction of the deployment's complete initial application state.
//!
//! It deliberately owns nothing else. Startup classification, the deployment
//! record, the selected database locator, Application Database selection, and
//! final lifecycle sealing all live behind the lifecycle authority this crate
//! consults, and no type here can perform any of them.
//!
//! # Ordering
//!
//! Every mutating operation consults its trusted authority before it reads any
//! submitted secret. That ordering is enforced by the type system rather than by
//! documentation: reading a submitted secret requires a borrow of
//! [`AuthorizedInit`], whose constructor is private to this crate and whose only
//! producer is the authority call. An operation invoked directly inside the
//! process after a routing or composition defect therefore still reaches the
//! authority first, and reports [`InitError::AlreadyInitialized`] against a
//! sealed deployment before it has read anything or caused any effect.

#![forbid(unsafe_code)]

mod checkpoint;
mod error;
mod request;
mod secret;
mod state;

use weavelit_server_components::AvailableComponents;
use weavelit_server_database::{CompletionObligation, DeploymentIdentifier, Name};

pub use crate::checkpoint::{CHECKPOINT_FORMAT_VERSION, InitCheckpoint, PreparedInitDelivery};
pub use crate::error::{CheckpointError, InitError, RequestError};
pub use crate::request::{
    InitialAdministrator, InitialLogModuleConfiguration, InitialProtectedSetting, InitializeServer,
    MAX_LOG_MODULE_CONFIGURATIONS, MAX_LOG_MODULE_SETTINGS, MAX_PROTECTED_LOG_MODULE_SETTINGS,
    ValidatedRequest, validate_request,
};
pub use crate::secret::{
    InitialPassword, InitialSecret, MAX_PASSWORD_BYTES, MAX_PROTECTED_SETTING_BYTES,
};
pub use crate::state::ADMINISTRATORS_GROUP_NAME;

use weavelit_server_authentication::PasswordVerifierFactory;
use weavelit_server_database::ApplicationState;
use weavelit_server_lifecycle::ProtectedValueSealer;

/// The trusted authority every mutating Init operation consults first.
///
/// The implementation re-reads the deployment record under the lifecycle
/// mutation permit. This crate never inspects that record itself, so Init has no
/// way to form its own opinion about whether the deployment is still eligible.
pub trait InitAuthority {
    /// Rechecks Init eligibility and returns the deployment this run binds to.
    ///
    /// # Errors
    ///
    /// Returns [`InitError::AlreadyInitialized`] when the deployment record is
    /// sealed, or a shared lifecycle category for any other refusal.
    fn authorize(&self) -> Result<InitTarget, InitError>;
}

/// The deployment an authorized Init run binds to.
///
/// Init carries no selected database locator, so this deliberately holds only
/// the deployment identity the authority confirmed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InitTarget {
    deployment_identifier: DeploymentIdentifier,
}

impl InitTarget {
    /// Creates the target an authority returns after it re-read the record.
    #[must_use]
    pub const fn new(deployment_identifier: DeploymentIdentifier) -> Self {
        Self {
            deployment_identifier,
        }
    }

    /// Returns the confirmed deployment identifier.
    #[must_use]
    pub const fn deployment_identifier(&self) -> DeploymentIdentifier {
        self.deployment_identifier
    }
}

/// Evidence that the trusted authority answered before any secret was read.
///
/// The constructor is private to this crate and the only caller of it is
/// [`InitOperations::authorize`], so this value cannot be forged from outside
/// the crate and cannot be produced inside it without an authority answer.
/// Every accessor that exposes a submitted secret requires a borrow of it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthorizedInit {
    deployment_identifier: DeploymentIdentifier,
}

impl AuthorizedInit {
    pub(crate) const fn new(deployment_identifier: DeploymentIdentifier) -> Self {
        Self {
            deployment_identifier,
        }
    }

    /// Returns the deployment identifier this authorization is bound to.
    #[must_use]
    pub const fn deployment_identifier(&self) -> DeploymentIdentifier {
        self.deployment_identifier
    }
}

/// The Init operations this Server build can perform.
///
/// The value is composed once from this build's component inventory, so an
/// operation cannot be pointed at a Client Module or Log Module that is not
/// compiled in.
#[derive(Clone, Debug)]
pub struct InitOperations {
    components: AvailableComponents,
    administration_client_module: Name,
    verifier_factory: PasswordVerifierFactory,
}

impl InitOperations {
    /// Composes the Init operations for this build.
    ///
    /// The Client Module the system-defined Group grants access to is supplied
    /// by the composing runtime and checked against the inventory here, so this
    /// crate never depends on a module crate and never grants access to a
    /// module this build cannot serve.
    ///
    /// # Errors
    ///
    /// Returns [`InitError::InitializationFailed`] when the named Client Module
    /// is not compiled into this build.
    pub fn new(
        components: AvailableComponents,
        administration_client_module: Name,
    ) -> Result<Self, InitError> {
        if !components.has_client_module(&administration_client_module) {
            return Err(InitError::InitializationFailed);
        }

        Ok(Self {
            components,
            administration_client_module,
            verifier_factory: PasswordVerifierFactory::approved(),
        })
    }

    /// Prepares the one-time recovery-key delivery for a new deployment.
    ///
    /// The authority is consulted first, so a sealed deployment produces no key
    /// material at all rather than generating a key it would then discard.
    ///
    /// # Errors
    ///
    /// Returns [`InitError::AlreadyInitialized`] when the deployment is sealed,
    /// a shared lifecycle category for any other refusal, and
    /// [`InitError::InitializationFailed`] when the key cannot be prepared.
    pub fn prepare_delivery(
        &self,
        authority: &dyn InitAuthority,
    ) -> Result<PreparedInitDelivery, InitError> {
        let _authorized = Self::authorize(authority)?;
        PreparedInitDelivery::prepare()
    }

    /// Finalizes Init into the deployment's complete initial application state.
    ///
    /// The order is fixed and observable: the authority answers, the submitted
    /// proof is compared in constant time against the checkpoint's stored
    /// expected proof, the request is validated against this build, and only
    /// then are the submitted secrets read and the single candidate state
    /// assembled.
    ///
    /// # Errors
    ///
    /// Returns [`InitError::AlreadyInitialized`] when the deployment is sealed,
    /// [`InitError::RecoveryKeyConfirmationRequired`] when no proof was
    /// submitted, [`InitError::RecoveryKeyConfirmationInvalid`] when the proof
    /// does not match, and [`InitError::InitializationFailed`] when the request
    /// or the assembled state is not acceptable.
    pub fn finalize(
        &self,
        authority: &dyn InitAuthority,
        checkpoint: &InitCheckpoint,
        request: &InitializeServer,
        sealer: &dyn ProtectedValueSealer,
        completion_obligation: CompletionObligation,
    ) -> Result<ApplicationState, InitError> {
        let authorized = Self::authorize(authority)?;
        checkpoint.confirm(request.recovery_key_proof.as_ref())?;
        let validated = validate_request(request, &self.components)?;

        crate::state::build_initial_state(
            &authorized,
            &validated,
            checkpoint,
            &self.administration_client_module,
            &self.verifier_factory,
            sealer,
            completion_obligation,
        )
    }

    /// Returns the Client Module the system-defined Group grants access to.
    #[must_use]
    pub const fn administration_client_module(&self) -> &Name {
        &self.administration_client_module
    }

    /// Consults the trusted authority and mints the evidence it answered.
    fn authorize(authority: &dyn InitAuthority) -> Result<AuthorizedInit, InitError> {
        Ok(AuthorizedInit::new(
            authority.authorize()?.deployment_identifier(),
        ))
    }
}
