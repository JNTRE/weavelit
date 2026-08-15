//! Server-owned Init orchestration and its two-request submission protocol.
//!
//! The Init crate, the lifecycle typestate chain, the Log Module catalog, and
//! Server Observability each own one part of a new deployment. This module is
//! the only place that joins them: it authorizes the workflow, prepares and
//! delivers the one-time recovery key, reacquires the exact pending checkpoint
//! for the second request, verifies the proof of possession, proves both Log
//! assignments, replaces the checkpoint with complete initial application
//! state, delivers the Init completion record, seals the deployment, and
//! activates normal operation in-process.
//!
//! Init is submitted in two requests. The first creates a non-operational
//! checkpoint and returns the private recovery key exactly once, then releases
//! the lifecycle mutex, the database handle, and the mutation-lane permit so
//! none of them is retained while a person saves that key. The second presents
//! the proof computed from the delivered key and is the only request that can
//! replace the checkpoint.
//!
//! # What makes finalization reachable
//!
//! The finalization route is mounted only after the recovery-key response has
//! actually been written. The composer hangs that publication off a
//! [`ResponseWriteAcknowledgement`], so a write failure, a disconnect, and an
//! expired budget all leave this Server fail closed with no finalization route
//! and no promoted delivery. Nothing here treats a written response as proof
//! that a person received the key; it is only the one observable event that
//! rules out the Server having failed to send it.
//!
//! # Asymmetric failure
//!
//! An actionable request or proof validation failure preserves same-process
//! finalization with the key already delivered, so a person corrects their
//! input and retries without being issued another key. An internal,
//! persistence, logging, sealing, or activation failure ends this process's
//! Init permanently and serves nothing.
//!
//! This module owns no wire format. The shared Client Module crate owns the
//! routes, schemas, and rendered responses; this module owns the delivery
//! stage, the lifecycle eligibility re-checks, the admission registrations, and
//! the orchestration behind them.

use std::{
    future::Future,
    path::PathBuf,
    pin::Pin,
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    extract::Request,
    http::Method,
    response::Response,
    routing::{MethodRouter, any},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use tokio::{
    sync::{Semaphore, oneshot},
    task,
};
use tower::ServiceExt as _;
use weavelit_module_client::{
    ExpectedOrigin, InitCapability, InitCompleted, InitFinalizeSubmission, InitRecoveryKeyPrepared,
    InitRecoveryKeySubmission, InitRejection, InitRequestSubmission, SelectedBackend,
    validate_init_request,
};
use weavelit_server_components::AvailableComponents;
use weavelit_server_database::{
    ConfigurationKey, ConfigurationValue, LogModuleSetting, LogType, Name, StateIdentifier,
};
use weavelit_server_init::{
    InitAuthority, InitCheckpoint, InitError, InitOperations, InitTarget, InitialAdministrator,
    InitialLogModuleConfiguration, InitialPassword, InitialProtectedSetting, InitialSecret,
    InitializeServer, validate_request,
};
use weavelit_server_lifecycle::{
    BackendCatalog, BackendIdentifier, DeploymentIdentifier, LifecycleError, LifecycleState,
    PendingWorkflow, ReleasedInitCheckpoint, TrustedBackendContext, WorkflowArbiter, WorkflowError,
};
use weavelit_server_log::{
    ConfiguredLogDestination, DestinationSettings, LogModuleCatalog, LogModuleIdentifier,
    LogRecordType, TrustedLogModuleContext, TrustedRecordIssuer,
};
use weavelit_server_log_authority::ServerLogAuthority;
use weavelit_server_observability::ServerObservability;
use weavelit_server_recovery_key::{RECOVERY_PROOF_BYTES, RecoveryProof};
use zeroize::Zeroizing;

use crate::{
    ResponseWriteAcknowledgement, RestrictedStartup, ServingModeSwitch,
    operational::{OperationalComposer, OperationalDatabase, OperationalRuntime},
    transport::{
        AdmittedCheck, BodyAdmission, PreBodyCheck, PreBodyGrant, PreBodyRejection,
        TransportProfile, TransportRegistration,
    },
};

/// The action a composer runs once the recovery-key response has been written.
///
/// It is an owned value rather than a borrow because it outlives the request
/// that produced it: the listener runs it after the response has left the
/// Server, from a task that no longer holds anything the route held.
pub(crate) type DeliveryPublication = Arc<dyn Fn() + Send + Sync>;

/// The event-time source an Init completion record is stamped from.
///
/// It is injected so a test observes an exact recorded time instead of
/// asserting against whatever the host clock happened to read.
pub(crate) type EventClock = Arc<dyn Fn() -> Option<i64> + Send + Sync>;

/// The boundaries the blocking preparation chain announces to a test.
///
/// The chain is uninterruptible by construction, so the only way to order a
/// cancellation against it deterministically is for the chain itself to say
/// where it is.
#[cfg(test)]
#[derive(Clone, Copy, Eq, PartialEq)]
enum PreparationPhase {
    /// Nothing is committed yet and the caller's liveness lease is about to be
    /// observed.
    BeforeLivenessCheck,
    /// Fail closed has been published and the checkpoint is about to be written.
    BeforeCheckpoint,
}

/// The pause a test drives the blocking preparation chain through.
#[cfg(test)]
type PreparationHook = Arc<dyn Fn(PreparationPhase) + Send + Sync>;

// ---------------------------------------------------------------------------
// Delivery stage
// ---------------------------------------------------------------------------

/// Everything a paused Init retains between its two requests.
///
/// The private recovery key is deliberately absent. Only the released
/// checkpoint, the non-secret checkpoint values this Server itself recorded,
/// the confirmed backend, and the correlation identifier are here, so no
/// orchestrator state, log, or restart state can reproduce the delivered key.
struct PendingInit {
    released: ReleasedInitCheckpoint,
    checkpoint: InitCheckpoint,
    backend: BackendIdentifier,
    correlation_identifier: String,
}

/// How far this process's single Init has progressed.
///
/// The stage is the one-time guard that route absence supports rather than
/// replaces. A connection accepted before a publication still holds a router
/// that mounts what this stage has already moved past, so every admission
/// re-reads it under the mutation lane.
#[derive(Default)]
enum InitStage {
    /// No delivery has been prepared.
    #[default]
    Idle,
    /// The checkpoint exists and its key response is being written. Nothing may
    /// finalize yet, because the write may still fail.
    Delivering(Box<PendingInit>),
    /// The key response was written successfully, so finalization may run.
    Delivered(Box<PendingInit>),
    /// A finalization attempt holds the pending delivery right now.
    Running,
    /// This process's Init ended, by sealing or by failing closed.
    Ended,
}

/// The one-time delivery stage shared by the orchestrator and its admissions.
#[derive(Default)]
struct DeliveryStage {
    stage: Mutex<InitStage>,
}

impl DeliveryStage {
    /// Records a freshly created checkpoint whose key response is being written.
    ///
    /// Only an idle stage accepts one, so a second checkpoint cannot be created
    /// even if a stale request reaches this Server with the lane free.
    fn begin_delivery(&self, pending: PendingInit) -> bool {
        let mut stage = self.held();
        if matches!(*stage, InitStage::Idle) {
            *stage = InitStage::Delivering(Box::new(pending));
            true
        } else {
            false
        }
    }

    /// Promotes a written key response into a finalizable delivery.
    ///
    /// This is the only transition that makes finalization possible, and the
    /// listener is the only caller, so an unwritten response leaves the stage
    /// where it was.
    fn acknowledge_delivery(&self) -> bool {
        let mut stage = self.held();
        match std::mem::replace(&mut *stage, InitStage::Idle) {
            InitStage::Delivering(pending) => {
                *stage = InitStage::Delivered(pending);
                true
            }
            other => {
                *stage = other;
                false
            }
        }
    }

    /// Takes the delivery for one finalization attempt.
    ///
    /// The stage moves to [`InitStage::Running`] for the whole attempt, so a
    /// concurrent or directly invoked second finalization finds nothing to act
    /// with rather than racing the first for the same checkpoint.
    fn claim(&self) -> Option<Box<PendingInit>> {
        let mut stage = self.held();
        match std::mem::replace(&mut *stage, InitStage::Running) {
            InitStage::Delivered(pending) => Some(pending),
            other => {
                *stage = other;
                None
            }
        }
    }

    /// Returns a claimed delivery after an actionable failure.
    fn release(&self, pending: Box<PendingInit>) {
        *self.held() = InitStage::Delivered(pending);
    }

    /// Ends this process's Init, whether it sealed or failed closed.
    fn end(&self) {
        *self.held() = InitStage::Ended;
    }

    /// Reports whether a new recovery-key preparation is still permitted.
    fn accepts_preparation(&self) -> bool {
        matches!(*self.held(), InitStage::Idle)
    }

    /// Reports whether a finalization request may be admitted.
    fn accepts_finalization(&self) -> bool {
        matches!(*self.held(), InitStage::Delivered(_))
    }

    /// Borrows the stage, recovering it from a panic that left it poisoned.
    ///
    /// A poisoned stage still decides whether Init may proceed, and the safe
    /// answer is the one the stage already records, so recovering the guard
    /// keeps every refusing path reachable.
    fn held(&self) -> MutexGuard<'_, InitStage> {
        self.stage.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

// ---------------------------------------------------------------------------
// Orchestrator
// ---------------------------------------------------------------------------

/// Server-owned composition that runs one Init at a time.
///
/// It shares the startup composition's `WorkflowArbiter` and mutation lane, so
/// Init serializes against pre-operational database selection and against a
/// Restore rather than racing either.
pub struct InitOrchestrator {
    arbiter: Arc<WorkflowArbiter>,
    catalog: Arc<BackendCatalog>,
    context: Arc<TrustedBackendContext>,
    log_catalog: Arc<LogModuleCatalog>,
    state_root: PathBuf,
    mutation_lane: Arc<Semaphore>,
    stage: Arc<DeliveryStage>,
    serving_modes: Arc<ServingModeSwitch>,
    /// The build's Init operations, composed once from its own inventory.
    operations: InitOperations,
    /// Retained so a submitted request is validated against exactly the
    /// inventory the operations were composed from.
    components: AvailableComponents,
    observability: ServerObservability,
    /// Retained privately so no caller outside this composition can mint a
    /// trusted Log Module context or a trusted record issuer.
    log_authority: ServerLogAuthority,
    clock: EventClock,
    /// The Server-wide values the handed-over operational surface composes from.
    operational_runtime: Arc<OperationalRuntime>,
    /// The operational composition a completed Init hands over.
    ///
    /// Sealing returns the database the workflow committed through, so this
    /// retains that same open handle for the operational runtime instead of
    /// letting it close and reopening the target afterwards.
    operational: Mutex<Option<OperationalComposer>>,
    /// The pause a test drives the blocking preparation chain through.
    #[cfg(test)]
    preparation_hook: Mutex<Option<PreparationHook>>,
}

impl InitOrchestrator {
    /// Composes Init over a restricted startup's lifecycle authority.
    ///
    /// `components` is this build's compiled-in inventory. A build whose
    /// administration Client Module is absent from it composes no Init at all,
    /// so this returns `None` rather than an orchestrator that would grant the
    /// first Group access to a module this Server cannot serve.
    ///
    /// `operational_runtime` is the same value startup composes its own
    /// operational surface from, so a deployment sealed by an Init serves the
    /// routes a deployment sealed at startup serves.
    ///
    /// `clock` reads the event time the Init completion record carries.
    /// Production always supplies the host clock.
    #[must_use]
    pub(crate) fn with_clock(
        startup: &RestrictedStartup,
        components: AvailableComponents,
        serving_modes: Arc<ServingModeSwitch>,
        operational_runtime: Arc<OperationalRuntime>,
        clock: EventClock,
    ) -> Option<Arc<Self>> {
        let administration_client_module =
            Name::new(weavelit_module_client_webui::MODULE_IDENTIFIER).ok()?;
        let operations =
            InitOperations::new(components.clone(), administration_client_module).ok()?;
        let log_authority = ServerLogAuthority::new();
        let observability =
            ServerObservability::new(TrustedRecordIssuer::from_server_authority(&log_authority));

        Some(Arc::new(Self {
            arbiter: Arc::clone(&startup.composition.adapter.arbiter),
            catalog: Arc::clone(&startup.composition.catalog),
            context: Arc::clone(&startup.composition.context),
            log_catalog: Arc::clone(&startup.log_catalog),
            state_root: startup.state_root.clone(),
            mutation_lane: Arc::clone(&startup.composition.adapter.mutation_lane),
            stage: Arc::new(DeliveryStage::default()),
            serving_modes,
            operations,
            components,
            observability,
            log_authority,
            clock,
            operational_runtime,
            operational: Mutex::new(None),
            #[cfg(test)]
            preparation_hook: Mutex::new(None),
        }))
    }

    /// Returns the Application Database a completed Init handed over.
    ///
    /// The handle is shared, not reopened: it is the descriptor the workflow
    /// itself committed through and sealed on.
    #[must_use]
    pub fn operational_database(&self) -> Option<OperationalDatabase> {
        self.operational
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .map(|operational| operational.database().clone())
    }

    /// Returns the Init capability a Client Module declares this Server with.
    #[must_use]
    pub fn capability(self: &Arc<Self>, expected_origin: ExpectedOrigin) -> InitCapability {
        let preparing = Arc::clone(self);
        let finalizing = Arc::clone(self);
        InitCapability {
            expected_origin,
            prepare_recovery_key: Arc::new(move |submission: InitRecoveryKeySubmission| {
                let orchestrator = Arc::clone(&preparing);
                Box::pin(async move { orchestrator.prepare_recovery_key(submission).await })
            }),
            finalize: Arc::new(move |submission: InitFinalizeSubmission| {
                let orchestrator = Arc::clone(&finalizing);
                Box::pin(async move { orchestrator.finalize(submission).await })
            }),
        }
    }

    /// Returns the registration that admits a recovery-key preparation.
    ///
    /// The body is bounded text well inside the listener's default bound, so
    /// the route keeps the default profile and its default read budget. What it
    /// adds is the mutation lane and the lifecycle eligibility re-check, both
    /// of which run before any body is allocated.
    #[must_use]
    pub fn recovery_key_registration(
        self: &Arc<Self>,
        expected_origin: ExpectedOrigin,
    ) -> TransportRegistration {
        TransportRegistration::new(
            Method::PUT,
            weavelit_module_client::INIT_RECOVERY_KEY_ROUTE,
            TransportProfile::DEFAULT,
        )
        .with_pre_body_check(Arc::new(InitHeadCheck { expected_origin }))
        .with_admission(Arc::clone(&self.mutation_lane))
        .with_admitted_check(Arc::new(InitEligibility {
            arbiter: Arc::clone(&self.arbiter),
            stage: Arc::clone(&self.stage),
            step: InitStep::Preparation,
        }))
    }

    /// Returns the registration that admits a finalization.
    #[must_use]
    pub fn finalization_registration(
        self: &Arc<Self>,
        expected_origin: ExpectedOrigin,
    ) -> TransportRegistration {
        TransportRegistration::new(
            Method::PUT,
            weavelit_module_client::INIT_ROUTE,
            TransportProfile::DEFAULT,
        )
        .with_pre_body_check(Arc::new(InitHeadCheck { expected_origin }))
        .with_admission(Arc::clone(&self.mutation_lane))
        .with_admitted_check(Arc::new(InitEligibility {
            arbiter: Arc::clone(&self.arbiter),
            stage: Arc::clone(&self.stage),
            step: InitStep::Finalization,
        }))
    }

    /// Promotes a written key response into a finalizable delivery.
    ///
    /// The listener calls this, through the composer's post-write action, and
    /// nothing else does. It reports whether a delivery was actually promoted,
    /// so the composer publishes the finalization route only when there is one
    /// to serve.
    pub(crate) fn acknowledge_delivery(&self) -> bool {
        self.stage.acknowledge_delivery()
    }

    // -----------------------------------------------------------------------
    // Recovery-key preparation
    // -----------------------------------------------------------------------

    /// Creates the Init checkpoint and delivers the private key exactly once.
    ///
    /// The submitted request's secrets are never read here. Preparation needs
    /// only the client's confirmation of the selected backend, so the password
    /// and every Log Module secret are cleared on the way in rather than held
    /// while a key is generated.
    async fn prepare_recovery_key(
        self: &Arc<Self>,
        submission: InitRecoveryKeySubmission,
    ) -> Result<InitRecoveryKeyPrepared, InitRejection> {
        // Carried in from admission: the lane was acquired before the body was
        // allocated, and it is held until this operation finishes.
        let admission = submission
            .context
            .get::<BodyAdmission>()
            .cloned()
            .ok_or(InitRejection::InitializationFailed)?;
        let delivering = submission
            .context
            .get::<DeliveringRequest>()
            .cloned()
            .ok_or(InitRejection::InitializationFailed)?;
        let backend = submission.request.backend;
        // Nothing below reads a submitted secret, so the whole request is
        // cleared here instead of outliving the call that received it.
        drop(submission.request);

        // The liveness lease. Dropping a `spawn_blocking` join handle does not
        // stop the task behind it, so a route future the listener's processing
        // timeout dropped would otherwise commit an Init for a caller that is
        // already gone. This request holds the sending half for exactly as long
        // as it exists, and the blocking chain reads the receiving half.
        let (lease, observer) = oneshot::channel::<()>();

        let orchestrator = Arc::clone(self);
        // The whole authorize-through-release chain is blocking work under a
        // `!Send` lifecycle guard, so it runs in one closure on one thread and
        // outside any cancellation point. Every publication the chain owes is
        // made inside it for the same reason.
        let prepared = task::spawn_blocking(move || {
            let _admission = admission;
            orchestrator.prepare(backend, observer, &delivering)
        })
        .await;

        // Held across the await above and released only here: the dropped
        // sender is the blocking chain's only signal that this request is gone.
        drop(lease);
        prepared.map_err(|_| InitRejection::InitializationFailed)?
    }

    /// Runs the blocking preparation chain.
    ///
    /// `lease` is the caller's liveness lease and `delivering` is its delivery
    /// marker. Both are consulted here rather than after the await, because a
    /// cancelled route future never resumes while this chain still runs on to
    /// commit the deployment.
    fn prepare(
        &self,
        backend: SelectedBackend,
        mut lease: oneshot::Receiver<()>,
        delivering: &DeliveringRequest,
    ) -> Result<InitRecoveryKeyPrepared, InitRejection> {
        if !self.stage.accepts_preparation() {
            return Err(InitRejection::AlreadyInitialized);
        }

        let permit = self
            .arbiter
            .authorize_workflow(&self.catalog, &self.context)
            .map_err(preparation_rejection)?;
        // The lifecycle authority is consulted before anything is generated:
        // this target exists only because `authorize_workflow` re-verified the
        // deployment record and the selected database under the exclusive
        // permit held for the rest of this operation.
        let authority = AuthorizedTarget(InitTarget::new(permit.deployment_identifier()));
        let selected = permit.selected_backend().clone();
        if selected.as_str() != backend.identifier() {
            return Err(InitRejection::BadRequest);
        }

        let prepared = self
            .operations
            .prepare_delivery(&authority)
            .map_err(init_rejection)?;
        let checkpoint = prepared.checkpoint().clone();
        let delivery_nonce = URL_SAFE_NO_PAD.encode(checkpoint.delivery_nonce().as_bytes());
        let metadata = checkpoint.encode().map_err(init_rejection)?;
        let correlation_identifier = crate::authentication::correlation_identifier()
            .ok_or(InitRejection::ServiceUnavailable)?;

        #[cfg(test)]
        self.pause(PreparationPhase::BeforeLivenessCheck);
        // Every step above is reversible, and this is the last moment at which
        // that is still true. The check is advisory rather than a correctness
        // boundary: a cancellation can still land just after it, which is why
        // the publication below precedes the durable write rather than
        // following it.
        if matches!(lease.try_recv(), Err(oneshot::error::TryRecvError::Closed)) {
            // The prepared delivery, and the key material inside it, are
            // dropped here. The stage stays idle, so Init remains retryable.
            return Err(InitRejection::InitializationFailed);
        }

        // Point of no return. Fail closed is published before the durable
        // mutation, so a cancellation that raced the check above still leaves a
        // committed Init visibly fail closed rather than apparently healthy.
        self.serving_modes.publish_fail_closed();
        #[cfg(test)]
        self.pause(PreparationPhase::BeforeCheckpoint);

        // Writes the deployment-bound checkpoint, advances the record to
        // `InitializationPending`, and only then gives back the permit, the
        // database handle, and the lane.
        let released = permit
            .create_init_checkpoint_and_release(metadata)
            .map_err(|error| self.abandon(preparation_rejection(error)))?;

        // The key is produced only after both commit paths completed.
        let recovery_key = prepared
            .into_delivery_line()
            .map_err(|error| self.abandon(init_rejection(error)))?;
        if !self.stage.begin_delivery(PendingInit {
            released,
            checkpoint,
            backend: selected,
            correlation_identifier: correlation_identifier.clone(),
        }) {
            return Err(self.abandon(InitRejection::AlreadyInitialized));
        }

        // Only now does a delivery exist for a written response to publish.
        delivering.mark();

        Ok(InitRecoveryKeyPrepared {
            recovery_key,
            delivery_nonce,
            correlation_id: correlation_identifier,
        })
    }

    /// Ends the delivery stage after a failure past the point of no return.
    ///
    /// Fail closed is already published, so this Server serves nothing; ending
    /// the stage keeps a later preparation or finalization from acting on the
    /// half-built delivery this failure left behind.
    fn abandon(&self, rejection: InitRejection) -> InitRejection {
        self.stage.end();
        rejection
    }

    /// Runs the installed pause, if any, at one preparation boundary.
    ///
    /// The hook is cloned out of its lock before it runs, so a parked chain
    /// holds nothing the test needs.
    #[cfg(test)]
    fn pause(&self, phase: PreparationPhase) {
        let hook = self
            .preparation_hook
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(hook) = hook {
            hook(phase);
        }
    }

    /// Installs the pause a test drives the blocking preparation chain through.
    #[cfg(test)]
    fn pause_preparation(&self, hook: PreparationHook) {
        *self
            .preparation_hook
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(hook);
    }

    // -----------------------------------------------------------------------
    // Finalization
    // -----------------------------------------------------------------------

    /// Verifies the proof and runs one Init through to normal operation.
    async fn finalize(
        self: &Arc<Self>,
        submission: InitFinalizeSubmission,
    ) -> Result<InitCompleted, InitRejection> {
        let admission = submission
            .context
            .get::<BodyAdmission>()
            .cloned()
            .ok_or(InitRejection::InitializationFailed)?;
        let request = submission.request;
        let proof = submission.recovery_key_proof;

        let orchestrator = Arc::clone(self);
        let outcome = task::spawn_blocking(move || {
            let _admission = admission;
            orchestrator.run(request, &proof)
        })
        .await
        .map_err(|_| InitRejection::InitializationFailed)?;

        // Every state publication this outcome owes was already made inside the
        // blocking chain, so a dropped route future cannot skip one. What is
        // left here is only the rendered rejection.
        match outcome {
            Ok(completed) => Ok(completed),
            Err(
                InitFailure::Actionable(rejection)
                | InitFailure::Refused(rejection)
                | InitFailure::Closed(rejection),
            ) => Err(rejection),
        }
    }

    /// Runs the blocking finalization chain.
    ///
    /// A claimed delivery is returned to the stage on an actionable failure and
    /// destroyed on any other, so the asymmetry is decided in one place rather
    /// than restated at each step. A closed failure publishes the fail-closed
    /// surface here, inside the uninterruptible chain, rather than after the
    /// await that a cancellation would never resume from.
    fn run(
        &self,
        request: InitRequestSubmission,
        proof: &str,
    ) -> Result<InitCompleted, InitFailure> {
        let Some(pending) = self.stage.claim() else {
            return Err(InitFailure::Refused(InitRejection::AlreadyInitialized));
        };

        match self.complete(&pending, request, proof) {
            Ok(completed) => {
                self.stage.end();
                Ok(completed)
            }
            Err(failure @ InitFailure::Actionable(_)) => {
                self.stage.release(pending);
                Err(failure)
            }
            Err(failure) => {
                self.stage.end();
                if matches!(failure, InitFailure::Closed(_)) {
                    // Every later connection serves nothing at all: this
                    // process will not retry, reset, recreate, or reconcile the
                    // retained state, and only a redeploy resolves it.
                    self.serving_modes.publish_fail_closed();
                }
                Err(failure)
            }
        }
    }

    /// Runs one finalization against the claimed pending delivery.
    fn complete(
        &self,
        pending: &PendingInit,
        request: InitRequestSubmission,
        proof: &str,
    ) -> Result<InitCompleted, InitFailure> {
        // Reopens and re-verifies the exact deployment-bound Init checkpoint
        // under a fresh exclusive permit, before the submitted proof is parsed
        // and before any submitted secret is read.
        let workflow = self
            .arbiter
            .reauthorize_pending_init(&self.catalog, &self.context, &pending.released)
            .map_err(finalization_failure)?;

        let submitted = decode_proof(proof).ok_or(InitFailure::Actionable(
            InitRejection::RecoveryKeyConfirmationInvalid,
        ))?;
        pending
            .checkpoint
            .confirm(Some(&submitted))
            .map_err(|error| InitFailure::Actionable(init_rejection(error)))?;

        if request.backend.identifier() != pending.backend.as_str() {
            return Err(InitFailure::Actionable(InitRejection::BadRequest));
        }
        // The proof matched, so the submitted secrets are read only now.
        let request = normalize(request, submitted)
            .map_err(|_| InitFailure::Actionable(InitRejection::InitializationFailed))?;
        // Validated here as well as inside the Init operations, so a request a
        // person can correct is separable from an internal failure that is not.
        validate_request(&request, &self.components)
            .map_err(|_| InitFailure::Actionable(InitRejection::InitializationFailed))?;

        self.commit(workflow, pending, &request)
    }

    /// Commits, logs, seals, and activates one validated Init.
    ///
    /// Every failure from here on is internal, so all of them end this
    /// process's Init rather than inviting another attempt.
    fn commit(
        &self,
        workflow: PendingWorkflow<'_>,
        pending: &PendingInit,
        request: &InitializeServer,
    ) -> Result<InitCompleted, InitFailure> {
        let deployment_identifier = workflow.deployment_identifier();
        let record_identifier =
            StateIdentifier::from_bytes(crate::authentication::random_bytes().ok_or(closed())?)
                .map_err(|_| closed())?;
        let event_time = (self.clock)().ok_or(closed())?;

        let (record, obligation) = self
            .observability
            .prepare_init_completion(
                record_identifier,
                deployment_identifier,
                event_time,
                &pending.correlation_identifier,
            )
            .map_err(|_| closed())?
            .into_parts();

        let state = self
            .operations
            .finalize(
                &AuthorizedTarget(InitTarget::new(deployment_identifier)),
                &pending.checkpoint,
                request,
                workflow.sealer(),
                obligation,
            )
            .map_err(|_| closed())?;

        // Both assignments are proven before anything is committed, so a
        // request naming a Log Module this Server cannot serve, a disabled
        // configuration, or a module that does not serve its assigned type
        // fails while failing is still free.
        let system_log = self.preflight(request, LogType::System, deployment_identifier)?;
        self.preflight(request, LogType::Audit, deployment_identifier)?;

        // Point of no return begins here. Every later connection observes the
        // fail-closed surface before any durable state changes, and keeps
        // observing it if the replacement does not complete.
        self.serving_modes.publish_fail_closed();

        let committed = workflow.complete_checkpoint(&state).map_err(|_| closed())?;
        // Delivered through the destination the committed System Log assignment
        // named and preflighted, not through one resolved again afterwards.
        system_log.deliver(&record).map_err(|_| closed())?;
        let sealed = committed
            .acknowledge_completion(record_identifier)
            .map_err(|_| closed())?
            .seal()
            .map_err(|_| closed())?;

        // The database sealing handed back is retained here and composed into
        // the operational surface, so normal operation begins on the handle the
        // Init committed through rather than on a newly opened one.
        let (state, database) = OperationalDatabase::from_sealed(sealed);
        let mount = self
            .operational
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(OperationalComposer::new(
                Arc::clone(&self.operational_runtime),
                &state,
                database,
            ))
            .mount();
        self.serving_modes.publish_operational(mount);

        Ok(InitCompleted {
            correlation_id: pending.correlation_identifier.clone(),
        })
    }

    /// Proves one assigned Log Module can durably accept its assigned type.
    ///
    /// The destination this returns is the one the assignment names, so the
    /// System Log acknowledgement is delivered through exactly the destination
    /// that was proven rather than through one resolved again afterwards.
    fn preflight(
        &self,
        request: &InitializeServer,
        log_type: LogType,
        deployment_identifier: DeploymentIdentifier,
    ) -> Result<ConfiguredLogDestination, InitFailure> {
        let assignment = match log_type {
            LogType::System => &request.system_log,
            LogType::Audit => &request.audit_log,
        };
        let module = request
            .log_module_configurations
            .iter()
            .find(|configuration| &configuration.name == assignment && configuration.enabled)
            .ok_or_else(closed)?;
        let identifier = LogModuleIdentifier::new(module.module.as_str()).map_err(|_| closed())?;
        // The configuration's own non-secret settings are supplied, so the
        // proof is about this assignment rather than about the module in the
        // abstract: a module that refuses a setting this request configured is
        // refused here instead of at the first record it can no longer deliver.
        let settings = DestinationSettings::new(
            module
                .settings
                .iter()
                .map(|setting| {
                    (
                        setting.key.as_str().to_owned(),
                        setting.value.as_str().to_owned(),
                    )
                })
                .collect(),
        )
        .map_err(|_| closed())?;

        let context = TrustedLogModuleContext::from_server_authority(
            &self.log_authority,
            self.state_root.clone(),
            *deployment_identifier.as_bytes(),
        )
        .with_settings(settings);
        let destination = self
            .log_catalog
            .create_destination(&identifier, &context)
            .map_err(|_| closed())?;
        destination
            .preflight(record_type(log_type))
            .map_err(|_| closed())?;
        Ok(destination)
    }
}

/// Returns the record type an assigned Log Module must be able to accept.
const fn record_type(log_type: LogType) -> LogRecordType {
    match log_type {
        LogType::System => LogRecordType::System,
        LogType::Audit => LogRecordType::Audit,
    }
}

/// Init authority backed by an already-granted lifecycle permit.
struct AuthorizedTarget(InitTarget);

impl InitAuthority for AuthorizedTarget {
    fn authorize(&self) -> Result<InitTarget, InitError> {
        Ok(self.0)
    }
}

// ---------------------------------------------------------------------------
// Failure classification
// ---------------------------------------------------------------------------

/// How one finalization failure changes what this process may still do.
enum InitFailure {
    /// A request or proof failure a person can correct. Same-process
    /// finalization is preserved with the key already delivered.
    Actionable(InitRejection),
    /// An internal, persistence, logging, sealing, or activation failure. This
    /// process's Init ends and the Server serves nothing.
    Closed(InitRejection),
    /// A request there was nothing to act on. Nothing was read, nothing
    /// changed, and the serving mode is left exactly as it was.
    Refused(InitRejection),
}

/// The fixed closed failure every internal step reports.
const fn closed() -> InitFailure {
    InitFailure::Closed(InitRejection::InitializationFailed)
}

/// Maps a lifecycle refusal of a finalization onto its documented outcome.
///
/// A sealed deployment is refused without touching the serving mode, because
/// the only way to observe it is a direct in-process call after this Server has
/// already activated normal operation.
fn finalization_failure(error: WorkflowError) -> InitFailure {
    match error {
        WorkflowError::AlreadyInitialized => {
            InitFailure::Refused(InitRejection::AlreadyInitialized)
        }
        WorkflowError::Lifecycle(
            LifecycleError::Persistence | LifecycleError::DependencyUnavailable,
        ) => InitFailure::Closed(InitRejection::ServiceUnavailable),
        _ => closed(),
    }
}

/// Maps a lifecycle refusal of a preparation onto its documented rejection.
fn preparation_rejection(error: WorkflowError) -> InitRejection {
    match error {
        WorkflowError::AlreadyInitialized
        | WorkflowError::AlreadyPending
        | WorkflowError::NotAllowed => InitRejection::AlreadyInitialized,
        WorkflowError::Lifecycle(
            LifecycleError::Persistence | LifecycleError::DependencyUnavailable,
        ) => InitRejection::ServiceUnavailable,
        _ => InitRejection::InitializationFailed,
    }
}

/// Maps a stable Init failure onto its documented route rejection.
fn init_rejection(error: InitError) -> InitRejection {
    match error {
        InitError::RecoveryKeyConfirmationRequired => {
            InitRejection::RecoveryKeyConfirmationRequired
        }
        InitError::RecoveryKeyConfirmationInvalid => InitRejection::RecoveryKeyConfirmationInvalid,
        InitError::AlreadyInitialized => InitRejection::AlreadyInitialized,
        InitError::Lifecycle(
            LifecycleError::Persistence | LifecycleError::DependencyUnavailable,
        ) => InitRejection::ServiceUnavailable,
        _ => InitRejection::InitializationFailed,
    }
}

/// Maps a route-level rejection onto the transport's closed rejection set.
fn pre_body_rejection(rejection: InitRejection) -> PreBodyRejection {
    match rejection {
        InitRejection::RequestOriginDenied => PreBodyRejection::RequestOriginDenied,
        InitRejection::AlreadyInitialized => PreBodyRejection::AlreadyInitialized,
        _ => PreBodyRejection::BadRequest,
    }
}

// ---------------------------------------------------------------------------
// Request normalization
// ---------------------------------------------------------------------------

/// Normalization refused a submitted value.
///
/// It carries nothing: the value that was refused is a submitted secret or a
/// bounded text this Server never returns, and a client learns only that Init
/// did not complete.
struct SubmissionInvalid;

/// Converts one transport submission into the Server-owned normalized request.
///
/// This is where the submitted password and Log Module secrets are first read,
/// and it runs only after the proof of possession matched.
fn normalize(
    submission: InitRequestSubmission,
    proof: RecoveryProof,
) -> Result<InitializeServer, SubmissionInvalid> {
    let administrator = InitialAdministrator {
        username: name(&submission.administrator.username)?,
        display_name: submission
            .administrator
            .display_name
            .as_deref()
            .map(name)
            .transpose()?,
        password: InitialPassword::new(submission.administrator.password.as_str().to_owned())
            .map_err(|_| SubmissionInvalid)?,
    };

    let mut log_module_configurations = Vec::with_capacity(submission.log_modules.len());
    for configuration in submission.log_modules {
        let mut settings = Vec::with_capacity(configuration.settings.len());
        for setting in configuration.settings {
            settings.push(LogModuleSetting {
                key: ConfigurationKey::new(setting.key).map_err(|_| SubmissionInvalid)?,
                value: ConfigurationValue::new(setting.value).map_err(|_| SubmissionInvalid)?,
            });
        }
        let mut protected_settings = Vec::with_capacity(configuration.protected_settings.len());
        for setting in configuration.protected_settings {
            protected_settings.push(InitialProtectedSetting {
                key: ConfigurationKey::new(setting.key).map_err(|_| SubmissionInvalid)?,
                value: InitialSecret::new(setting.value.as_bytes().to_vec())
                    .map_err(|_| SubmissionInvalid)?,
            });
        }
        log_module_configurations.push(InitialLogModuleConfiguration {
            module: name(&configuration.module)?,
            name: name(&configuration.name)?,
            enabled: configuration.enabled,
            settings,
            protected_settings,
        });
    }

    Ok(InitializeServer {
        administrator,
        log_module_configurations,
        system_log: name(&submission.system_log)?,
        audit_log: name(&submission.audit_log)?,
        recovery_key_proof: Some(proof),
    })
}

fn name(value: &str) -> Result<Name, SubmissionInvalid> {
    Name::new(value).map_err(|_| SubmissionInvalid)
}

/// Decodes the unpadded URL-safe Base64 proof the delivery nonce is proved with.
///
/// The transport already checked the proof's shape, so a value that does not
/// decode to an untruncated HMAC-SHA-256 length here is refused rather than
/// padded, truncated, or compared against a shorter expected value.
fn decode_proof(proof: &str) -> Option<RecoveryProof> {
    let decoded = Zeroizing::new(URL_SAFE_NO_PAD.decode(proof).ok()?);
    let bytes = <[u8; RECOVERY_PROOF_BYTES]>::try_from(decoded.as_slice()).ok()?;
    Some(RecoveryProof::from_bytes(bytes))
}

// ---------------------------------------------------------------------------
// Admission checks
// ---------------------------------------------------------------------------

/// Head validation shared by both Init routes.
///
/// One predicate serves both, so they cannot disagree about what a trusted Init
/// request looks like.
struct InitHeadCheck {
    expected_origin: ExpectedOrigin,
}

impl PreBodyCheck for InitHeadCheck {
    fn check(
        &self,
        method: &Method,
        _uri: &axum::http::Uri,
        headers: &axum::http::HeaderMap,
    ) -> Result<PreBodyGrant, PreBodyRejection> {
        validate_init_request(method, headers, self.expected_origin).map_err(pre_body_rejection)?;
        Ok(PreBodyGrant::accepted())
    }
}

/// Which Init request an eligibility check admits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InitStep {
    Preparation,
    Finalization,
}

/// Lifecycle and delivery eligibility re-checked under the acquired lane.
///
/// Route absence is necessary but not sufficient. The listener snapshots the
/// router when it accepts a connection, so a connection accepted before a
/// publication still holds a router that mounts what this Server has moved
/// past. Reading the authoritative deployment record and the delivery stage
/// here, under the same lane a selection, a Restore, and a checkpoint take,
/// rejects that stale request before anything sensitive is read or allocated.
struct InitEligibility {
    arbiter: Arc<WorkflowArbiter>,
    stage: Arc<DeliveryStage>,
    step: InitStep,
}

impl AdmittedCheck for InitEligibility {
    fn check(&self) -> Pin<Box<dyn Future<Output = Result<(), PreBodyRejection>> + Send + '_>> {
        let arbiter = Arc::clone(&self.arbiter);
        let stage = Arc::clone(&self.stage);
        let step = self.step;
        Box::pin(async move {
            let eligible = task::spawn_blocking(move || match step {
                InitStep::Preparation => {
                    stage.accepts_preparation()
                        && arbiter.record_state() == LifecycleState::Uninitialized
                        && arbiter
                            .projection()
                            .is_ok_and(|projection| projection.database_selected())
                }
                InitStep::Finalization => {
                    stage.accepts_finalization()
                        && arbiter.record_state() == LifecycleState::InitializationPending
                }
            })
            .await
            .unwrap_or(false);
            if eligible {
                Ok(())
            } else {
                Err(PreBodyRejection::AlreadyInitialized)
            }
        })
    }
}

// ---------------------------------------------------------------------------
// Delivery publication
// ---------------------------------------------------------------------------

/// The per-request marker that records whether this request created a delivery.
///
/// It travels in the request extensions, so the wrapper that mounted the route
/// learns what the orchestration behind it did without inspecting the rendered
/// response, and without a second request's outcome ever publishing this one's
/// capability.
#[derive(Clone, Default)]
struct DeliveringRequest(Arc<AtomicBool>);

impl DeliveringRequest {
    fn mark(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    fn delivered(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Wraps the recovery-key route so a written key response publishes finalization.
///
/// The acknowledgement is attached only to the response of the request that
/// actually created a checkpoint, so a refused, malformed, or stale request
/// publishes nothing however it was rendered. The listener owns when the action
/// runs, and runs it only after every byte of the response was written and the
/// connection was shut down cleanly.
pub(crate) fn delivering_route(route: MethodRouter, publish: DeliveryPublication) -> MethodRouter {
    any(move |mut request: Request| {
        let route = route.clone();
        let publish = Arc::clone(&publish);
        async move {
            let delivering = DeliveringRequest::default();
            request.extensions_mut().insert(delivering.clone());
            let mut response: Response = route
                .oneshot(request)
                .await
                .unwrap_or_else(|error| match error {});
            if delivering.delivered() {
                response
                    .extensions_mut()
                    .insert(ResponseWriteAcknowledgement::new(move || publish()));
            }
            response
        }
    })
}

// ---------------------------------------------------------------------------
// Clock
// ---------------------------------------------------------------------------

/// Returns the host clock every shipped Init completion record is stamped from.
pub(crate) fn system_event_clock() -> EventClock {
    Arc::new(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|elapsed| i64::try_from(elapsed.as_millis()).ok())
    })
}

#[cfg(test)]
mod tests {
    use std::{
        ffi::OsString,
        net::SocketAddr,
        os::unix::fs::PermissionsExt as _,
        path::{Path, PathBuf},
        sync::{
            Arc, Mutex, PoisonError,
            atomic::{AtomicUsize, Ordering as AtomicOrdering},
        },
    };

    use axum::{
        Router,
        body::Body,
        extract::Request,
        http::{Method, StatusCode, header},
    };
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
    use tokio::{
        io::AsyncWriteExt as _,
        sync::{oneshot, watch},
    };
    use tower::ServiceExt as _;
    use weavelit_module_client::{
        APPLICATION_DATABASE_ROUTE, AUTH_LOGIN_ROUTE, AUTH_SESSION_ROUTE, CSRF_COOKIE_NAME,
        INIT_RECOVERY_KEY_ROUTE, INIT_ROUTE, InitAdministratorSubmission, InitFinalizeSubmission,
        InitRejection, InitRequestSubmission, RESTORE_ROUTE, SESSION_COOKIE_NAME, STATUS_ROUTE,
        SelectedBackend as SubmittedBackend,
    };
    use weavelit_server_database::Name;
    use weavelit_server_lifecycle::{
        BackendIdentifier, LifecycleError, LifecycleState, LifecycleStore, TrustedBackendContext,
        WorkflowError,
    };
    use weavelit_server_log::{
        CompleteLogRecord, DurableAcknowledgement, LogCapabilities, LogDestination,
        LogDestinationError, LogDestinationFactory, LogModuleCatalog, LogModuleFactoryContext,
        LogModuleRegistration, LogRecordType, LogSettingsContract,
    };
    use weavelit_server_recovery_key::{
        DeliveryNonce, RECOVERY_PROOF_BYTES, RecoveryKey, RecoveryProof,
    };
    use zeroize::Zeroizing;

    use super::{
        DeliveryStage, InitFailure, InitOrchestrator, InitStage, PendingInit, PreparationPhase,
        closed, decode_proof, finalization_failure, system_event_clock,
    };
    use crate::{
        APPLICATION_DATABASE_FILE, PreoperationalComposer, RateLimiter,
        ResponseWriteAcknowledgement, RestrictedStartup, ServingMode, ServingModeSwitch,
        StartupError, StartupOutcome, bounded_response_from_axum, classify_restricted_startup,
        fallback_router, server_components, sqlite_catalog,
        transport::{BodyAdmission, MountedSurface, PreBodyRejection, TransportProfile},
    };

    /// The authority every Init request in these tests targets.
    const LISTENER: &str = "127.0.0.1:8443";

    /// The source address the admission chain rate-limits against.
    const SOURCE: &str = "203.0.113.10";

    /// The exact event time an injected clock reports.
    const EVENT_TIME: i64 = 1_700_000_000_000;

    /// The Log Module configuration name the System Log assignment points at.
    const SYSTEM_ASSIGNMENT: &str = "system-local";

    /// The Log Module configuration name the Audit Log assignment points at.
    const AUDIT_ASSIGNMENT: &str = "audit-local";

    /// The classification every Init completion record carries.
    const INIT_CLASSIFICATION: &str = "lifecycle.init";

    /// The username the seeded first Administrator is created with.
    const ADMINISTRATOR: &str = "administrator";

    /// The password the seeded first Administrator is created with.
    const ADMINISTRATOR_PASSWORD: &str = "correct horse battery staple";

    /// The body an Application Database selection submits.
    const SELECTION_BODY: &str = "{\"backend\":\"sqlite\",\"settings\":{}}";

    // -----------------------------------------------------------------------
    // Harness
    // -----------------------------------------------------------------------

    /// A pre-operational deployment with a real, selected SQLite database.
    ///
    /// The surface is composed by the production pre-operational composer over
    /// the production component inventory, catalogs, and startup
    /// classification, so a route this harness cannot reach is a route the
    /// shipped binary cannot reach either.
    struct InitSurface {
        /// Retained so the state root outlives the orchestration under test.
        _root: tempfile::TempDir,
        state_root: PathBuf,
        startup: RestrictedStartup,
        /// Retained so a test reaches the same orchestrator the mounted routes
        /// call, rather than a second one composed beside it.
        composer: Arc<PreoperationalComposer>,
        switch: Arc<ServingModeSwitch>,
        modes: watch::Receiver<ServingMode>,
    }

    impl InitSurface {
        fn new() -> Self {
            Self::composed(Arc::new(|| Some(EVENT_TIME)), None)
        }

        /// Composes the surface against the clock the shipped binary uses.
        fn production() -> Self {
            Self::composed(system_event_clock(), None)
        }

        /// Composes the surface with a substituted Log Module implementation.
        ///
        /// The substitution keeps the production module identifier, so the
        /// request is still judged against the production component inventory
        /// and still travels the production route composition; only the
        /// destination behind the assignment is observable.
        fn recording(
            capabilities: Vec<LogRecordType>,
            preflight: Preflight,
        ) -> (Self, Arc<LogRecorder>) {
            let recorder = Arc::new(LogRecorder::default());
            let surface = Self::composed(
                Arc::new(|| Some(EVENT_TIME)),
                Some(recording_catalog(&recorder, capabilities, preflight)),
            );
            (surface, recorder)
        }

        fn composed(clock: super::EventClock, log_catalog: Option<LogModuleCatalog>) -> Self {
            let root = tempfile::tempdir().unwrap();
            std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
            let state_root = root.path().canonicalize().unwrap();
            {
                let mut store = LifecycleStore::open_or_create(&state_root).unwrap();
                store
                    .select_database(
                        &sqlite_catalog(),
                        &TrustedBackendContext::new(state_root.join(APPLICATION_DATABASE_FILE)),
                        &BackendIdentifier::new("sqlite").unwrap(),
                        Vec::new(),
                    )
                    .unwrap();
            }

            let mut startup = classify_restricted_startup(&state_root).unwrap();
            assert_eq!(startup.outcome(), StartupOutcome::UninitializedWithDatabase);
            if let Some(catalog) = log_catalog {
                startup.log_catalog = Arc::new(catalog);
            }

            let (switch, modes) = ServingModeSwitch::new(ServingMode::FailClosed(
                MountedSurface::without_registrations(fallback_router()),
            ));
            let switch = Arc::new(switch);
            let composer =
                crate::PreoperationalComposer::with_clock(&startup, listener(), &switch, clock);
            composer.publish_initial(startup.composition.outcome);

            Self {
                _root: root,
                state_root,
                startup,
                composer,
                switch,
                modes,
            }
        }

        /// Returns the orchestrator the mounted Init routes call into.
        fn orchestrator(&self) -> &Arc<InitOrchestrator> {
            self.composer
                .init
                .as_ref()
                .expect("this build compiles in its administration Client Module")
        }

        /// Snapshots the surface the next accepted connection would serve.
        fn served(&self) -> MountedSurface {
            self.modes.borrow().surface().clone()
        }

        /// Runs one Init request through the listener's whole admission chain
        /// and the mounted router, exactly as a served connection does.
        async fn submit(&self, target: &str, body: &str) -> Served {
            serve(&self.served(), target, body).await
        }

        /// Prepares a recovery key and acknowledges its delivery, leaving the
        /// deployment where a person who has kept their key stands.
        async fn deliver_key(&self) -> Delivered {
            let served = self
                .submit(INIT_RECOVERY_KEY_ROUTE, &preparation_body())
                .await;
            assert_eq!(served.status, StatusCode::OK, "{}", served.body);
            let delivered = Delivered::from_response(&served.body);
            served.acknowledge();
            delivered
        }

        /// Prepares a key and finalizes with the well-formed submission.
        async fn complete(&self) -> Delivered {
            let delivered = self.deliver_key().await;
            let served = self
                .submit(INIT_ROUTE, &finalization_body(&delivered.proof()))
                .await;
            assert_eq!(served.status, StatusCode::OK, "{}", served.body);
            delivered
        }

        /// Returns the lifecycle state the authoritative record holds.
        ///
        /// It is read through the live arbiter rather than by reopening the
        /// store, because this process already holds the state-root lock for
        /// the whole surface; a restart's own view is asserted by the restart
        /// tests, which release that lock first.
        fn record_state(&self) -> LifecycleState {
            self.startup.composition.adapter.arbiter.record_state()
        }

        fn anchor_snapshot(&self) -> Vec<(OsString, Vec<u8>, i64, i64)> {
            crate::tests::anchor_snapshot(&self.state_root)
        }

        /// Releases every retained handle so a restart can reopen the state root.
        ///
        /// The published surface, the composer, and the startup each retain the
        /// lifecycle store, the Application Database, or both, so all three are
        /// dropped here rather than left to outlive the restart they precede.
        /// A published surface's routes close over the composer that serves
        /// them, so the surface is replaced with a fail-closed one first; a
        /// real restart is a new process and never has to unpick that.
        fn release(self) -> (tempfile::TempDir, PathBuf) {
            let Self {
                _root,
                state_root,
                startup,
                composer,
                switch,
                modes,
            } = self;
            switch.publish_fail_closed();
            drop(modes);
            drop(switch);
            drop(composer);
            drop(startup);
            (_root, state_root)
        }
    }

    // -----------------------------------------------------------------------
    // Substituted Log Module
    // -----------------------------------------------------------------------

    /// What the substituted Log Module's preflight does before it delegates.
    #[derive(Clone, Copy, Eq, PartialEq)]
    enum Preflight {
        /// Prove the commit path exactly as the shipped module does.
        Prove,
        /// Refuse, as a destination whose storage cannot reach commit does.
        Refuse,
    }

    /// Everything the substituted Log Module observed.
    #[derive(Default)]
    struct LogRecorder {
        preflighted: Mutex<Vec<LogRecordType>>,
        delivered: AtomicUsize,
    }

    impl LogRecorder {
        fn preflighted(&self) -> Vec<LogRecordType> {
            self.preflighted
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
        }

        fn delivered(&self) -> usize {
            self.delivered.load(AtomicOrdering::SeqCst)
        }
    }

    /// Builds a catalog whose `sqlite` entry is the recording destination.
    fn recording_catalog(
        recorder: &Arc<LogRecorder>,
        capabilities: Vec<LogRecordType>,
        preflight: Preflight,
    ) -> LogModuleCatalog {
        LogModuleCatalog::new(vec![LogModuleRegistration::new(
            weavelit_module_log_sqlite::MODULE_IDENTIFIER,
            LogCapabilities::new(capabilities).expect("the declared capabilities are valid"),
            Box::new(RecordingFactory {
                recorder: Arc::clone(recorder),
                preflight,
            }),
        )])
        .expect("the substituted catalog is valid")
    }

    struct RecordingFactory {
        recorder: Arc<LogRecorder>,
        preflight: Preflight,
    }

    impl LogDestinationFactory for RecordingFactory {
        fn accepted_settings(&self) -> LogSettingsContract {
            weavelit_module_log_sqlite::SqliteLogDestinationFactory.accepted_settings()
        }

        fn create(
            &self,
            context: &LogModuleFactoryContext<'_>,
        ) -> Result<Box<dyn LogDestination>, LogDestinationError> {
            Ok(Box::new(RecordingDestination {
                inner: weavelit_module_log_sqlite::SqliteLogDestinationFactory.create(context)?,
                recorder: Arc::clone(&self.recorder),
                preflight: self.preflight,
            }))
        }
    }

    struct RecordingDestination {
        inner: Box<dyn LogDestination>,
        recorder: Arc<LogRecorder>,
        preflight: Preflight,
    }

    impl LogDestination for RecordingDestination {
        fn deliver(
            &self,
            record: &CompleteLogRecord,
            acknowledgement: DurableAcknowledgement,
        ) -> Result<DurableAcknowledgement, LogDestinationError> {
            self.recorder.delivered.fetch_add(1, AtomicOrdering::SeqCst);
            self.inner.deliver(record, acknowledgement)
        }

        fn preflight(&self, record_type: LogRecordType) -> Result<(), LogDestinationError> {
            self.recorder
                .preflighted
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(record_type);
            match self.preflight {
                Preflight::Prove => self.inner.preflight(record_type),
                Preflight::Refuse => Err(LogDestinationError::Unavailable),
            }
        }
    }

    fn listener() -> SocketAddr {
        LISTENER.parse().expect("the listener authority parses")
    }

    /// One served response and the acknowledgement its write would run.
    struct Served {
        status: StatusCode,
        body: String,
        cookies: Option<String>,
        acknowledgement: Option<ResponseWriteAcknowledgement>,
    }

    impl Served {
        /// Runs the acknowledgement the listener runs after the response bytes
        /// were written successfully.
        fn acknowledge(self) {
            self.acknowledgement
                .expect("a delivered recovery key carries a write acknowledgement")
                .run();
        }

        /// Drops the response without writing it, which is what a disconnect,
        /// a write failure, and an expired budget are all indistinguishable
        /// from at this seam.
        fn discard(self) {
            drop(self);
        }

        /// Returns the session cookie value a successful sign-in established.
        fn session_cookie(&self) -> String {
            self.cookie(SESSION_COOKIE_NAME)
        }

        /// Reads one named cookie value out of the sign-in's cookie effect.
        ///
        /// Session validation is a double-submit check, so a test that sent
        /// only the session cookie would be refused exactly as a forged
        /// cross-site request is. Both cookies are read from the same effect
        /// the production sign-in emitted rather than restated here.
        fn cookie(&self, name: &str) -> String {
            let rendered = self
                .cookies
                .as_ref()
                .expect("a verified sign-in carries the session cookie effect");
            let start = rendered
                .find(name)
                .unwrap_or_else(|| panic!("the sign-in must set {name}: {rendered}"));
            let end = start
                + rendered[start..]
                    .find(';')
                    .expect("the cookie attributes follow the value");
            rendered[start..end].to_owned()
        }
    }

    /// The values one successful recovery-key delivery returned.
    struct Delivered {
        recovery_key: String,
        delivery_nonce: String,
    }

    impl Delivered {
        fn from_response(body: &str) -> Self {
            Self {
                recovery_key: field(body, "recovery_key"),
                delivery_nonce: field(body, "delivery_nonce"),
            }
        }

        /// Computes the proof a client that kept the delivered key computes.
        fn proof(&self) -> String {
            let identity = RecoveryKey::parse(&self.recovery_key)
                .expect("the delivered line is canonical")
                .into_identity()
                .expect("the delivered line is a private identity");
            let nonce = DeliveryNonce::from_bytes(
                URL_SAFE_NO_PAD
                    .decode(&self.delivery_nonce)
                    .expect("the delivered nonce decodes")
                    .try_into()
                    .expect("the delivered nonce is the expected length"),
            );
            URL_SAFE_NO_PAD.encode(
                RecoveryProof::compute(&identity, &nonce)
                    .expect("the proof computes")
                    .as_bytes(),
            )
        }
    }

    /// Extracts one string field from a typed success envelope.
    ///
    /// The needle is built from the field name rather than matched against a
    /// rendered value, so a redacted or renamed field fails the extraction
    /// instead of silently matching nothing.
    fn field(body: &str, name: &str) -> String {
        let needle = format!("\"{name}\":\"");
        let start = body
            .find(&needle)
            .unwrap_or_else(|| panic!("the response must carry {name}: {body}"))
            + needle.len();
        let end = start
            + body[start..]
                .find('"')
                .expect("the field value is terminated");
        body[start..end].to_owned()
    }

    /// Builds the head both Init routes accept.
    fn init_head(target: &str, declared_bytes: usize) -> Request {
        head(target, declared_bytes, None, None)
    }

    /// Builds the head every Server-owned `PUT` route in these tests accepts.
    ///
    /// A presented session travels as the one cookie the operational routes
    /// read, so a signed-in request is built from the value a sign-in actually
    /// set rather than from a restatement of it.
    fn head(
        target: &str,
        declared_bytes: usize,
        session: Option<&str>,
        csrf: Option<&str>,
    ) -> Request {
        let builder = Request::builder()
            .method(Method::PUT)
            .uri(target)
            .header(header::HOST, LISTENER)
            .header(header::ORIGIN, format!("https://{LISTENER}"))
            .header("x-weavelit-csrf", csrf.unwrap_or("1"))
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ACCEPT, "application/json")
            .header(header::CONTENT_LENGTH, declared_bytes.to_string());
        match session {
            Some(cookie) => builder.header(header::COOKIE, cookie),
            None => builder,
        }
        .body(Body::empty())
        .expect("the request head is well formed")
    }

    /// Runs the listener's ordered admission chain over a mounted surface.
    ///
    /// Every stage the listener runs before it allocates a body runs here in
    /// the same order, so a test cannot reach a handler through a path the
    /// listener would have refused.
    async fn admit(
        surface: &MountedSurface,
        target: &str,
        body: &str,
    ) -> Result<Request, PreBodyRejection> {
        admit_head(surface, init_head(target, body.len()), body).await
    }

    async fn admit_head(
        surface: &MountedSurface,
        request: Request,
        body: &str,
    ) -> Result<Request, PreBodyRejection> {
        let admitted = crate::transport::HeadRead::new(request)
            .admit_rate(
                &RateLimiter::new(),
                SOURCE.parse().expect("the source address parses"),
                std::time::Instant::now(),
            )
            .map_err(|_| PreBodyRejection::BadRequest)?
            .classify(surface.registry())
            .check_framing()
            .map_err(|_| PreBodyRejection::BadRequest)?
            .validate()?
            .acquire()
            .await?;

        let (mut client, mut server) = tokio::io::duplex(body.len().max(1));
        client.write_all(body.as_bytes()).await.unwrap();
        client.shutdown().await.unwrap();
        admitted
            .read_body(&mut server)
            .await
            .map_err(|_| PreBodyRejection::BadRequest)
    }

    /// Serves one admitted request and renders it the way the listener does.
    async fn serve(surface: &MountedSurface, target: &str, body: &str) -> Served {
        serve_head(surface, init_head(target, body.len()), body).await
    }

    async fn serve_head(surface: &MountedSurface, request: Request, body: &str) -> Served {
        let request = match admit_head(surface, request, body).await {
            Ok(request) => request,
            Err(rejection) => {
                // Renders through the listener's own fixed-response path, so
                // the test observes the bytes a refused client receives rather
                // than a restatement of them.
                let refused = rejection.response();
                return Served {
                    status: refused.status,
                    body: String::from_utf8(refused.body.to_vec())
                        .expect("a fixed rejection body is valid UTF-8"),
                    cookies: None,
                    acknowledgement: None,
                };
            }
        };
        let response = surface
            .router()
            .clone()
            .oneshot(request)
            .await
            .expect("the mounted router is infallible");
        let bounded = bounded_response_from_axum(response).await;
        Served {
            status: bounded.status,
            body: String::from_utf8_lossy(&bounded.body).into_owned(),
            cookies: bounded
                .cookies
                .as_ref()
                .map(|lines| lines.as_str().to_owned()),
            acknowledgement: bounded.acknowledgement,
        }
    }

    // -----------------------------------------------------------------------
    // Request bodies
    // -----------------------------------------------------------------------

    fn log_module(module: &str, name: &str, enabled: bool) -> String {
        format!(
            "{{\"module\":\"{module}\",\"name\":\"{name}\",\"enabled\":{enabled},\
             \"settings\":[],\"protected_settings\":[]}}"
        )
    }

    /// One configuration carrying a single non-secret setting.
    fn configured_log_module(module: &str, name: &str, key: &str, value: &str) -> String {
        format!(
            "{{\"module\":\"{module}\",\"name\":\"{name}\",\"enabled\":true,\
             \"settings\":[{{\"key\":\"{key}\",\"value\":\"{value}\"}}],\
             \"protected_settings\":[]}}"
        )
    }

    /// The two enabled configurations a well-formed submission carries.
    ///
    /// Each log type is an independent assignment, so the System Log and the
    /// Audit Log name two different configurations of the compiled-in module.
    fn log_modules() -> String {
        format!(
            "{},{}",
            log_module("sqlite", SYSTEM_ASSIGNMENT, true),
            log_module("sqlite", AUDIT_ASSIGNMENT, true)
        )
    }

    /// The complete preparation body a well-formed submission carries.
    fn preparation_body() -> String {
        body_with(&log_modules(), SYSTEM_ASSIGNMENT, AUDIT_ASSIGNMENT, None)
    }

    /// The finalization body for a delivered key.
    fn finalization_body(proof: &str) -> String {
        body_with(
            &log_modules(),
            SYSTEM_ASSIGNMENT,
            AUDIT_ASSIGNMENT,
            Some(proof),
        )
    }

    fn body_with(
        log_modules: &str,
        system_log: &str,
        audit_log: &str,
        proof: Option<&str>,
    ) -> String {
        let proof = proof.map_or_else(String::new, |proof| {
            format!(",\"recovery_key_proof\":\"{proof}\"")
        });
        format!(
            "{{\"database\":{{\"backend\":\"sqlite\"}},\
             \"administrator\":{{\"username\":\"{ADMINISTRATOR}\",\
             \"display_name\":\"First Administrator\",\
             \"password\":\"{ADMINISTRATOR_PASSWORD}\"}},\
             \"log_modules\":[{log_modules}],\
             \"system_log\":\"{system_log}\",\"audit_log\":\"{audit_log}\"{proof}}}"
        )
    }

    /// The body a sign-in through the operational surface submits.
    fn login_body(username: &str, password: &str) -> String {
        format!(
            "{{\"username\":\"{username}\",\"password\":\"{password}\",\
             \"client_module\":\"web-ui\"}}"
        )
    }

    // -----------------------------------------------------------------------
    // Mounting and the delivery gate
    // -----------------------------------------------------------------------

    /// The exact targets a surface serves a non-404 response for.
    async fn serves(surface: &MountedSurface, target: &str) -> bool {
        let response = surface
            .router()
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(target)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        response.status() != StatusCode::NOT_FOUND
    }

    /// The exact bytes a surface renders for one plain target.
    ///
    /// Comparing the whole rendered response, rather than a status alone, is
    /// what makes an isolation assertion about an unrelated route meaningful.
    async fn rendered(router: &Router, method: Method, target: &str) -> (StatusCode, Vec<u8>) {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(target)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bounded = bounded_response_from_axum(response).await;
        (bounded.status, bounded.body.to_vec())
    }

    /// The transport profile a surface grants one method and target.
    fn profile_for(surface: &MountedSurface, target: &str) -> TransportProfile {
        let Ok(admitted) = crate::transport::HeadRead::new(init_head(target, 0)).admit_rate(
            &RateLimiter::new(),
            SOURCE.parse().expect("the source address parses"),
            std::time::Instant::now(),
        ) else {
            panic!("a first request from one source is never rate limited")
        };
        admitted.classify(surface.registry()).profile()
    }

    /// A pre-operational deployment mounts the route that prepares a key and
    /// does not mount the route that finalizes against one, because no key has
    /// been delivered for a finalization to prove possession of.
    #[tokio::test]
    async fn a_preoperational_surface_mounts_preparation_without_finalization() {
        let surface = InitSurface::new();
        let served = surface.served();

        assert!(serves(&served, INIT_RECOVERY_KEY_ROUTE).await);
        assert!(!serves(&served, INIT_ROUTE).await);
    }

    /// The finalization route becomes reachable only after the response
    /// carrying the private key was actually written.
    #[tokio::test]
    async fn a_written_key_response_publishes_finalization_exactly_once() {
        let surface = InitSurface::new();
        let served = surface
            .submit(INIT_RECOVERY_KEY_ROUTE, &preparation_body())
            .await;
        assert_eq!(served.status, StatusCode::OK);

        // Still unwritten: the person does not have the key yet.
        assert!(!serves(&surface.served(), INIT_ROUTE).await);

        let acknowledgement = served
            .acknowledgement
            .expect("a delivered recovery key carries a write acknowledgement");
        acknowledgement.run();
        let after_first = surface.served();
        assert!(serves(&after_first, INIT_ROUTE).await);
        assert!(!serves(&after_first, INIT_RECOVERY_KEY_ROUTE).await);

        // A second run of the same acknowledgement must not republish, which
        // would reopen a delivery that has already been claimed.
        acknowledgement.run();
        assert!(serves(&surface.served(), INIT_ROUTE).await);
        assert!(!serves(&surface.served(), INIT_RECOVERY_KEY_ROUTE).await);
    }

    /// A key response that never reached the client leaves the Server fail
    /// closed with no finalization route at all. A write failure, a
    /// disconnect, and an expired budget are indistinguishable here and all
    /// three take this path.
    #[tokio::test]
    async fn an_unwritten_key_response_leaves_no_finalization_route() {
        let surface = InitSurface::new();
        let served = surface
            .submit(INIT_RECOVERY_KEY_ROUTE, &preparation_body())
            .await;
        assert_eq!(served.status, StatusCode::OK);
        served.discard();

        let after = surface.served();
        assert!(!serves(&after, INIT_ROUTE).await);
        assert!(!serves(&after, INIT_RECOVERY_KEY_ROUTE).await);
        assert!(!serves(&after, RESTORE_ROUTE).await);
        assert!(!serves(&after, STATUS_ROUTE).await);
        assert!(!serves(&after, APPLICATION_DATABASE_ROUTE).await);
        assert_eq!(
            surface.record_state(),
            LifecycleState::InitializationPending
        );
    }

    /// While a live finalization is pending, nothing else is mounted: the
    /// already-loaded page has exactly one call left to make.
    #[tokio::test]
    async fn a_pending_finalization_mounts_nothing_but_finalization() {
        let surface = InitSurface::new();
        surface.deliver_key().await;

        let served = surface.served();
        assert!(serves(&served, INIT_ROUTE).await);
        for absent in [
            INIT_RECOVERY_KEY_ROUTE,
            RESTORE_ROUTE,
            STATUS_ROUTE,
            APPLICATION_DATABASE_ROUTE,
        ] {
            assert!(!serves(&served, absent).await, "{absent} must be absent");
        }
    }

    // -----------------------------------------------------------------------
    // Cancellation
    // -----------------------------------------------------------------------

    /// A one-shot barrier a test drives one blocking chain through.
    ///
    /// The blocking side announces that it reached a boundary and then parks
    /// until the test releases it, so a cancellation is ordered against an
    /// uninterruptible chain by construction rather than by a sleep or by a
    /// timing assumption.
    struct Barrier {
        arrival: Mutex<Option<oneshot::Sender<()>>>,
        blocked: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
    }

    impl Barrier {
        fn new() -> (Arc<Self>, BarrierControl) {
            let (arrival, arrived) = oneshot::channel();
            let (release, blocked) = std::sync::mpsc::channel();
            (
                Arc::new(Self {
                    arrival: Mutex::new(Some(arrival)),
                    blocked: Mutex::new(Some(blocked)),
                }),
                BarrierControl { arrived, release },
            )
        }

        /// Parks the calling blocking chain until the test releases it.
        ///
        /// Only the first arrival parks, so a retry through the same installed
        /// hook runs straight past this boundary.
        fn wait(&self) {
            let arrival = self
                .arrival
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take();
            let blocked = self
                .blocked
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take();
            if let Some(arrival) = arrival {
                let _ = arrival.send(());
            }
            if let Some(blocked) = blocked {
                let _ = blocked.recv();
            }
        }
    }

    /// The test side of a [`Barrier`].
    struct BarrierControl {
        arrived: oneshot::Receiver<()>,
        release: std::sync::mpsc::Sender<()>,
    }

    impl BarrierControl {
        /// Resolves once the blocking chain is parked at the boundary.
        async fn reached(&mut self) {
            (&mut self.arrived)
                .await
                .expect("the blocking chain reaches the boundary");
        }

        /// Lets the parked blocking chain run to completion.
        fn release(self) {
            let _ = self.release.send(());
        }
    }

    /// Installs a preparation hook that parks the chain at exactly one boundary.
    fn park_preparation_at(surface: &InitSurface, boundary: PreparationPhase) -> BarrierControl {
        let (barrier, control) = Barrier::new();
        surface
            .orchestrator()
            .pause_preparation(Arc::new(move |phase| {
                if phase == boundary {
                    barrier.wait();
                }
            }));
        control
    }

    /// A clock that parks the finalization chain and then refuses to report a
    /// time, which is the closed failure every internal step reports.
    fn parking_clock() -> (super::EventClock, BarrierControl) {
        let (barrier, control) = Barrier::new();
        (
            Arc::new(move || {
                barrier.wait();
                None
            }),
            control,
        )
    }

    /// Serves one Init request from its own task, as the listener does.
    ///
    /// Aborting the returned handle drops the route future at its await, which
    /// is exactly what the listener's processing timeout does to it.
    fn spawn_request(
        surface: &InitSurface,
        target: &'static str,
        body: String,
    ) -> tokio::task::JoinHandle<Served> {
        let served = surface.served();
        tokio::spawn(async move { serve(&served, target, &body).await })
    }

    /// Drops an aborted route task and proves it never completed.
    async fn cancel(request: tokio::task::JoinHandle<Served>) {
        request.abort();
        assert!(
            request
                .await
                .err()
                .is_some_and(|error| error.is_cancelled()),
            "the route future must be dropped rather than resumed"
        );
    }

    /// Resolves once the blocking chain that held the mutation lane finished.
    ///
    /// The admission permit is held for the whole blocking chain, so
    /// reacquiring the single-permit lane is the exact completion signal; no
    /// sleep or timing assumption stands in for it.
    async fn lane_settled(surface: &InitSurface) {
        drop(
            Arc::clone(&surface.orchestrator().mutation_lane)
                .acquire_owned()
                .await
                .expect("the mutation lane is open"),
        );
    }

    /// A preparation whose route future is dropped before its liveness lease is
    /// observed commits nothing at all. The blocking chain runs on regardless,
    /// so the deployment record, the serving mode, and the delivery stage are
    /// all left where an untouched deployment stands, and a retry still works.
    #[tokio::test]
    async fn a_preparation_cancelled_before_the_liveness_check_commits_nothing() {
        let surface = InitSurface::new();
        let anchors = surface.anchor_snapshot();
        let mut barrier = park_preparation_at(&surface, PreparationPhase::BeforeLivenessCheck);

        let request = spawn_request(&surface, INIT_RECOVERY_KEY_ROUTE, preparation_body());
        barrier.reached().await;
        cancel(request).await;
        barrier.release();
        lane_settled(&surface).await;

        assert_eq!(surface.record_state(), LifecycleState::Uninitialized);
        assert_eq!(surface.anchor_snapshot(), anchors);
        assert!(matches!(
            *surface.modes.borrow(),
            ServingMode::PreOperational(_)
        ));
        assert!(surface.orchestrator().stage.accepts_preparation());
        assert!(serves(&surface.served(), INIT_RECOVERY_KEY_ROUTE).await);

        // The whole workflow is still available, key and all.
        surface.complete().await;
        assert_eq!(surface.record_state(), LifecycleState::Initialized);
    }

    /// A preparation cancelled after its liveness check still commits, because
    /// the blocking chain past that point cannot be interrupted. The Server it
    /// leaves behind is visibly fail closed rather than apparently healthy: the
    /// checkpoint exists, nothing is mounted, a stale preparation router
    /// refuses, and the undelivered key is gone for good.
    #[tokio::test]
    async fn a_preparation_cancelled_after_fail_closed_leaves_no_way_to_obtain_the_key() {
        let surface = InitSurface::new();
        let mut barrier = park_preparation_at(&surface, PreparationPhase::BeforeCheckpoint);
        let stale = surface.served();

        let request = spawn_request(&surface, INIT_RECOVERY_KEY_ROUTE, preparation_body());
        barrier.reached().await;
        cancel(request).await;
        barrier.release();
        lane_settled(&surface).await;

        assert_eq!(
            surface.record_state(),
            LifecycleState::InitializationPending
        );
        assert!(matches!(
            *surface.modes.borrow(),
            ServingMode::FailClosed(_)
        ));

        let after = surface.served();
        for absent in [
            INIT_ROUTE,
            INIT_RECOVERY_KEY_ROUTE,
            RESTORE_ROUTE,
            STATUS_ROUTE,
            APPLICATION_DATABASE_ROUTE,
        ] {
            assert!(!serves(&after, absent).await, "{absent} must be absent");
        }

        // A connection accepted before the publication still holds a router
        // that mounts preparation, and it obtains nothing through it.
        let refused = serve(&stale, INIT_RECOVERY_KEY_ROUTE, &preparation_body()).await;
        assert_eq!(refused.status, InitRejection::AlreadyInitialized.status());
        assert!(
            !refused.body.contains("recovery_key"),
            "no response may carry the key: {}",
            refused.body
        );
    }

    /// A finalization that fails closed publishes the fail-closed surface from
    /// inside its own blocking chain, so an aborted route task cannot leave
    /// this Server serving a healthier surface than its state deserves.
    #[tokio::test]
    async fn a_closed_finalization_fails_closed_even_when_its_route_task_is_aborted() {
        let (clock, mut barrier) = parking_clock();
        let surface = InitSurface::composed(clock, None);
        let delivered = surface.deliver_key().await;
        let stale = surface.served();

        let request = spawn_request(&surface, INIT_ROUTE, finalization_body(&delivered.proof()));
        barrier.reached().await;
        cancel(request).await;
        barrier.release();
        lane_settled(&surface).await;

        assert!(matches!(
            *surface.modes.borrow(),
            ServingMode::FailClosed(_)
        ));
        assert_eq!(
            surface.record_state(),
            LifecycleState::InitializationPending
        );
        assert!(system_log_records(&surface.state_root).is_empty());
        assert!(!serves(&surface.served(), INIT_ROUTE).await);

        let refused = serve(&stale, INIT_ROUTE, &finalization_body(&delivered.proof())).await;
        assert_eq!(refused.status, InitRejection::AlreadyInitialized.status());
    }

    // -----------------------------------------------------------------------
    // Completion
    // -----------------------------------------------------------------------

    /// One complete Init over a real SQLite deployment, driven end to end
    /// through the production composer, seals the deployment in the same
    /// process and activates its operational surface without a restart.
    #[tokio::test]
    async fn a_complete_init_seals_and_activates_in_the_same_process() {
        let surface = InitSurface::new();
        let delivered = surface.deliver_key().await;

        let served = surface
            .submit(INIT_ROUTE, &finalization_body(&delivered.proof()))
            .await;
        assert_eq!(served.status, StatusCode::OK, "{}", served.body);
        assert!(served.body.contains("\"lifecycle\":\"initialized\""));

        assert_eq!(surface.record_state(), LifecycleState::Initialized);

        // Both Init routes are gone and the operational surface is serving.
        let after = surface.served();
        assert!(!serves(&after, INIT_ROUTE).await);
        assert!(!serves(&after, INIT_RECOVERY_KEY_ROUTE).await);
        assert!(matches!(
            *surface.modes.borrow(),
            ServingMode::Operational(_)
        ));
        assert!(serves(&after, AUTH_SESSION_ROUTE).await);
    }

    /// The completion record reaches the destination the committed System Log
    /// assignment named, carrying the exact event time the clock reported.
    #[tokio::test]
    async fn the_completion_record_is_delivered_through_the_committed_system_log() {
        let surface = InitSurface::new();
        let delivered = surface.deliver_key().await;
        let served = surface
            .submit(INIT_ROUTE, &finalization_body(&delivered.proof()))
            .await;
        assert_eq!(served.status, StatusCode::OK, "{}", served.body);

        let records = system_log_records(&surface.state_root);
        assert_eq!(records.len(), 1, "exactly one Init completion record");
        let (classification, event_time) = &records[0];
        assert_eq!(classification, INIT_CLASSIFICATION);
        assert_eq!(*event_time, EVENT_TIME);
    }

    /// Reads every System Log record the SQLite Log Module durably committed.
    ///
    /// The destination is the one the module derives from the trusted local
    /// root, and the table and columns are the ones its own migrations create,
    /// so a query that finds nothing means nothing was committed rather than
    /// that this helper looked in the wrong place.
    fn system_log_records(state_root: &Path) -> Vec<(String, i64)> {
        let database = state_root.join("log.sqlite3");
        if !database.exists() {
            return Vec::new();
        }
        let connection = rusqlite::Connection::open(&database).unwrap();
        let mut statement = connection
            .prepare(
                "SELECT classification, event_time_milliseconds \
                 FROM weavelit_log_system_records ORDER BY rowid",
            )
            .unwrap();
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?
                        .parse::<i64>()
                        .expect("the module stores the event time as decimal milliseconds"),
                ))
            })
            .unwrap();
        rows.map(Result::unwrap).collect()
    }

    // -----------------------------------------------------------------------
    // Log Module preflight
    // -----------------------------------------------------------------------

    /// Both assignments are proven before the checkpoint is replaced, and the
    /// completion record is delivered through the proven System Log.
    #[tokio::test]
    async fn both_log_assignments_are_preflighted_before_the_checkpoint_is_replaced() {
        let (surface, recorder) = InitSurface::recording(
            vec![LogRecordType::System, LogRecordType::Audit],
            Preflight::Prove,
        );
        surface.complete().await;

        let mut proven = recorder.preflighted();
        proven.sort_unstable();
        assert_eq!(proven, vec![LogRecordType::System, LogRecordType::Audit]);
        assert_eq!(recorder.delivered(), 1);
        assert_eq!(surface.record_state(), LifecycleState::Initialized);
    }

    /// An assignment naming a disabled configuration is refused, and the
    /// deployment is left exactly where the delivered key left it.
    #[tokio::test]
    async fn a_disabled_assignment_is_refused_before_anything_is_committed() {
        let modules = format!(
            "{},{}",
            log_module("sqlite", SYSTEM_ASSIGNMENT, false),
            log_module("sqlite", AUDIT_ASSIGNMENT, true)
        );
        assert_uncommitted_actionable_refusal(&modules, SYSTEM_ASSIGNMENT, AUDIT_ASSIGNMENT).await;
    }

    /// An assignment naming no configuration at all is refused the same way.
    #[tokio::test]
    async fn an_absent_assignment_is_refused_before_anything_is_committed() {
        assert_uncommitted_actionable_refusal(&log_modules(), "not-configured", AUDIT_ASSIGNMENT)
            .await;
    }

    /// A configuration carrying a setting its Log Module never declared is
    /// refused as a correctable request, not as a late module failure.
    ///
    /// The compiled-in module declares no setting at all, so the request below
    /// is one its factory would refuse at preflight. Refusing it at validation
    /// is what keeps the delivered key usable: the refusal is asserted directly
    /// against the delivery stage and the serving mode, because reaching the
    /// factory would have destroyed the delivery and published the fail-closed
    /// surface, leaving no retry short of redeployment.
    #[tokio::test]
    async fn an_undeclared_log_module_setting_is_refused_before_anything_is_committed() {
        let modules = format!(
            "{},{}",
            configured_log_module("sqlite", SYSTEM_ASSIGNMENT, "retention-days", "30"),
            log_module("sqlite", AUDIT_ASSIGNMENT, true)
        );

        let (surface, recorder) = InitSurface::recording(
            vec![LogRecordType::System, LogRecordType::Audit],
            Preflight::Prove,
        );
        let delivered = surface.deliver_key().await;
        let anchors = surface.anchor_snapshot();

        let served = surface
            .submit(
                INIT_ROUTE,
                &body_with(
                    &modules,
                    SYSTEM_ASSIGNMENT,
                    AUDIT_ASSIGNMENT,
                    Some(&delivered.proof()),
                ),
            )
            .await;

        assert_eq!(served.status, InitRejection::InitializationFailed.status());
        // The named module was never reached, so nothing durable exists to
        // undo: no preflight ran, no record was delivered, and the checkpoint
        // the delivered key belongs to is untouched.
        assert!(recorder.preflighted().is_empty());
        assert_eq!(recorder.delivered(), 0);
        assert_eq!(
            surface.record_state(),
            LifecycleState::InitializationPending
        );
        assert_eq!(surface.anchor_snapshot(), anchors);
        assert!(system_log_records(&surface.state_root).is_empty());
        assert!(
            !matches!(*surface.modes.borrow(), ServingMode::FailClosed(_)),
            "a correctable request must not publish the fail-closed surface"
        );
        assert!(
            matches!(
                *surface.orchestrator().stage.held(),
                InitStage::Delivered(_)
            ),
            "the pending delivery must remain claimable"
        );

        // The same key still finalizes once the setting is removed.
        let retried = surface
            .submit(INIT_ROUTE, &finalization_body(&delivered.proof()))
            .await;
        assert_eq!(retried.status, StatusCode::OK, "{}", retried.body);
        assert_eq!(surface.record_state(), LifecycleState::Initialized);
    }

    /// Submits one correctable assignment failure and proves nothing durable
    /// changed and the delivered key still finalizes.
    async fn assert_uncommitted_actionable_refusal(
        modules: &str,
        system_log: &str,
        audit_log: &str,
    ) {
        let (surface, recorder) = InitSurface::recording(
            vec![LogRecordType::System, LogRecordType::Audit],
            Preflight::Prove,
        );
        let delivered = surface.deliver_key().await;
        let anchors = surface.anchor_snapshot();
        assert!(!anchors.is_empty(), "the deployment record must exist");

        let served = surface
            .submit(
                INIT_ROUTE,
                &body_with(modules, system_log, audit_log, Some(&delivered.proof())),
            )
            .await;

        assert_eq!(served.status, InitRejection::InitializationFailed.status());
        assert_eq!(
            surface.record_state(),
            LifecycleState::InitializationPending
        );
        assert_eq!(surface.anchor_snapshot(), anchors);
        assert_eq!(recorder.delivered(), 0);
        assert!(system_log_records(&surface.state_root).is_empty());
        assert!(
            serves(&surface.served(), INIT_ROUTE).await,
            "a correctable failure keeps finalization reachable"
        );
        assert!(!serves(&surface.served(), INIT_RECOVERY_KEY_ROUTE).await);

        // The same key still finalizes once the request is corrected.
        let retried = surface
            .submit(INIT_ROUTE, &finalization_body(&delivered.proof()))
            .await;
        assert_eq!(retried.status, StatusCode::OK, "{}", retried.body);
        assert_eq!(surface.record_state(), LifecycleState::Initialized);
    }

    /// A module that does not serve the log type it was assigned is refused
    /// before the checkpoint is replaced, and that refusal is not correctable.
    #[tokio::test]
    async fn a_type_incompatible_assignment_is_refused_before_anything_is_committed() {
        let (surface, recorder) =
            InitSurface::recording(vec![LogRecordType::System], Preflight::Prove);
        let delivered = surface.deliver_key().await;
        let anchors = surface.anchor_snapshot();

        let served = surface
            .submit(INIT_ROUTE, &finalization_body(&delivered.proof()))
            .await;

        assert_eq!(served.status, InitRejection::InitializationFailed.status());
        assert_eq!(
            surface.record_state(),
            LifecycleState::InitializationPending
        );
        assert_eq!(surface.anchor_snapshot(), anchors);
        assert_eq!(recorder.delivered(), 0);
        assert!(system_log_records(&surface.state_root).is_empty());
        // The declared capability is checked before the module is reached, so
        // only the System assignment ever ran a preflight.
        assert_eq!(recorder.preflighted(), vec![LogRecordType::System]);
        assert!(matches!(
            *surface.modes.borrow(),
            ServingMode::FailClosed(_)
        ));
    }

    /// A module that cannot prove its own commit path ends this process's Init.
    #[tokio::test]
    async fn a_log_module_that_cannot_prove_its_commit_path_fails_closed() {
        let (surface, recorder) = InitSurface::recording(
            vec![LogRecordType::System, LogRecordType::Audit],
            Preflight::Refuse,
        );
        let delivered = surface.deliver_key().await;
        let anchors = surface.anchor_snapshot();
        // Captured while finalization is still mounted, so the retry below is
        // made through a router that still offers the route.
        let stale = surface.served();

        let served = surface
            .submit(INIT_ROUTE, &finalization_body(&delivered.proof()))
            .await;

        assert_eq!(served.status, InitRejection::InitializationFailed.status());
        assert_eq!(recorder.preflighted(), vec![LogRecordType::System]);
        assert_eq!(recorder.delivered(), 0);
        assert_eq!(
            surface.record_state(),
            LifecycleState::InitializationPending
        );
        assert_eq!(surface.anchor_snapshot(), anchors);
        assert_permanently_closed(&surface, &stale, &delivered).await;
    }

    /// Proves a process whose Init failed closed serves nothing and cannot be
    /// retried, even through a router accepted before the failure.
    async fn assert_permanently_closed(
        surface: &InitSurface,
        stale: &MountedSurface,
        delivered: &Delivered,
    ) {
        assert!(matches!(
            *surface.modes.borrow(),
            ServingMode::FailClosed(_)
        ));
        let after = surface.served();
        for absent in [
            INIT_ROUTE,
            INIT_RECOVERY_KEY_ROUTE,
            RESTORE_ROUTE,
            STATUS_ROUTE,
            APPLICATION_DATABASE_ROUTE,
        ] {
            assert!(!serves(&after, absent).await, "{absent} must be absent");
        }

        let retried = serve(stale, INIT_ROUTE, &finalization_body(&delivered.proof())).await;
        assert_eq!(retried.status, InitRejection::AlreadyInitialized.status());
        assert_eq!(
            surface.record_state(),
            LifecycleState::InitializationPending
        );
    }

    // -----------------------------------------------------------------------
    // Asymmetric failure
    // -----------------------------------------------------------------------

    /// A proof that does not match preserves same-process finalization with the
    /// key already delivered, and the retry that follows completes with it.
    #[tokio::test]
    async fn an_invalid_proof_preserves_finalization_with_the_delivered_key() {
        let surface = InitSurface::new();
        let delivered = surface.deliver_key().await;
        let anchors = surface.anchor_snapshot();
        let wrong = URL_SAFE_NO_PAD.encode([0_u8; RECOVERY_PROOF_BYTES]);
        assert_ne!(wrong, delivered.proof(), "the wrong proof must differ");

        let served = surface.submit(INIT_ROUTE, &finalization_body(&wrong)).await;
        assert_eq!(
            served.status,
            InitRejection::RecoveryKeyConfirmationInvalid.status()
        );
        assert_eq!(
            surface.record_state(),
            LifecycleState::InitializationPending
        );
        assert_eq!(surface.anchor_snapshot(), anchors);
        assert!(system_log_records(&surface.state_root).is_empty());

        // No second key is issued, and the one already delivered still works.
        assert!(!serves(&surface.served(), INIT_RECOVERY_KEY_ROUTE).await);
        assert!(serves(&surface.served(), INIT_ROUTE).await);
        let retried = surface
            .submit(INIT_ROUTE, &finalization_body(&delivered.proof()))
            .await;
        assert_eq!(retried.status, StatusCode::OK, "{}", retried.body);
        assert_eq!(surface.record_state(), LifecycleState::Initialized);
    }

    /// A request a person can correct, rather than a proof failure, takes the
    /// same preserving path.
    #[tokio::test]
    async fn a_correctable_request_failure_preserves_finalization_with_the_delivered_key() {
        // Both log types assigned to one configuration: each assignment must be
        // independent, so this is refused as a request the submitter corrects.
        assert_uncommitted_actionable_refusal(&log_modules(), SYSTEM_ASSIGNMENT, SYSTEM_ASSIGNMENT)
            .await;
    }

    /// Every internal step reports the failure that ends this process's Init,
    /// and a lifecycle refusal of an already-sealed deployment does not.
    #[test]
    fn the_failure_classification_separates_actionable_from_closed() {
        assert!(matches!(
            closed(),
            InitFailure::Closed(InitRejection::InitializationFailed)
        ));
        assert!(matches!(
            finalization_failure(WorkflowError::AlreadyInitialized),
            InitFailure::Refused(InitRejection::AlreadyInitialized)
        ));
        assert!(matches!(
            finalization_failure(WorkflowError::Lifecycle(LifecycleError::Persistence)),
            InitFailure::Closed(InitRejection::ServiceUnavailable)
        ));
        assert!(matches!(
            finalization_failure(WorkflowError::Lifecycle(
                LifecycleError::DependencyUnavailable
            )),
            InitFailure::Closed(InitRejection::ServiceUnavailable)
        ));
        for internal in [
            WorkflowError::NotAllowed,
            WorkflowError::DatabaseNotSelected,
            WorkflowError::AlreadyPending,
            WorkflowError::StateMismatch,
            WorkflowError::Lifecycle(LifecycleError::IntegrityFailure),
        ] {
            assert!(matches!(
                finalization_failure(internal),
                InitFailure::Closed(InitRejection::InitializationFailed)
            ));
        }
    }

    /// A paused Init retains only what it must, and nothing that reproduces the
    /// key a person was asked to save.
    #[tokio::test]
    async fn a_paused_delivery_retains_nothing_that_reproduces_the_key() {
        let surface = InitSurface::new();
        let served = surface
            .submit(INIT_RECOVERY_KEY_ROUTE, &preparation_body())
            .await;
        assert_eq!(served.status, StatusCode::OK, "{}", served.body);
        let correlation = field(&served.body, "correlation_id");
        let delivered = Delivered::from_response(&served.body);
        assert!(!delivered.recovery_key.is_empty());
        assert!(!correlation.is_empty());
        served.acknowledge();

        let stage: &DeliveryStage = &surface.orchestrator().stage;
        assert!(matches!(*stage.held(), InitStage::Delivered(_)));
        let pending = stage.claim().expect("a written delivery is claimable");
        assert!(matches!(*stage.held(), InitStage::Running));
        assert!(
            stage.claim().is_none(),
            "a claimed delivery cannot be claimed twice"
        );

        // Exhaustive on purpose: a field added to the retained delivery has to
        // be accounted for here rather than silently retained.
        let PendingInit {
            released: _,
            checkpoint,
            backend,
            correlation_identifier,
        } = &*pending;
        assert_eq!(backend.as_str(), SubmittedBackend::Sqlite.identifier());
        assert_eq!(correlation_identifier, &correlation);
        // The retained checkpoint still demands the proof it never stored.
        assert!(checkpoint.confirm(None).is_err());

        stage.release(pending);
        assert!(matches!(*stage.held(), InitStage::Delivered(_)));
        let completed = surface
            .submit(INIT_ROUTE, &finalization_body(&delivered.proof()))
            .await;
        assert_eq!(completed.status, StatusCode::OK, "{}", completed.body);
    }

    // -----------------------------------------------------------------------
    // Concurrency and stale requests
    // -----------------------------------------------------------------------

    /// Two preparations admitted against the same router create exactly one
    /// checkpoint: the second is refused under the lane the first held.
    #[tokio::test]
    async fn concurrent_preparations_create_exactly_one_checkpoint() {
        let surface = InitSurface::new();
        let served = surface.served();
        let accepted = AtomicUsize::new(0);
        let refused = AtomicUsize::new(0);

        let body = preparation_body();
        let (first, second) = tokio::join!(
            serve(&served, INIT_RECOVERY_KEY_ROUTE, &body),
            serve(&served, INIT_RECOVERY_KEY_ROUTE, &body),
        );
        for outcome in [first, second] {
            if outcome.status == StatusCode::OK {
                accepted.fetch_add(1, AtomicOrdering::SeqCst);
            } else {
                assert_eq!(outcome.status, InitRejection::AlreadyInitialized.status());
                refused.fetch_add(1, AtomicOrdering::SeqCst);
            }
        }

        assert_eq!(accepted.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(refused.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(
            surface.record_state(),
            LifecycleState::InitializationPending
        );
    }

    /// A stale router that still mounts preparation cannot create a second
    /// checkpoint, start a Restore, or select another Application Database.
    #[tokio::test]
    async fn a_stale_preoperational_router_cannot_reopen_a_pending_init() {
        let surface = InitSurface::new();
        let stale = surface.served();
        assert!(serves(&stale, INIT_RECOVERY_KEY_ROUTE).await);
        assert!(serves(&stale, RESTORE_ROUTE).await);
        assert!(serves(&stale, APPLICATION_DATABASE_ROUTE).await);

        surface.deliver_key().await;
        let anchors = surface.anchor_snapshot();

        let refused = AtomicUsize::new(0);
        for (target, body) in [
            (INIT_RECOVERY_KEY_ROUTE, preparation_body()),
            (RESTORE_ROUTE, String::from("{}")),
            (APPLICATION_DATABASE_ROUTE, String::from(SELECTION_BODY)),
        ] {
            let served = serve(&stale, target, &body).await;
            assert_ne!(served.status, StatusCode::OK, "{target}: {}", served.body);
            refused.fetch_add(1, AtomicOrdering::SeqCst);
        }

        assert_eq!(refused.load(AtomicOrdering::SeqCst), 3);
        assert_eq!(
            surface.record_state(),
            LifecycleState::InitializationPending
        );
        assert_eq!(surface.anchor_snapshot(), anchors);
        assert!(system_log_records(&surface.state_root).is_empty());
    }

    /// A stale router that still mounts finalization cannot finalize a second
    /// time, so a sealed deployment cannot be resealed or relogged.
    #[tokio::test]
    async fn a_stale_finalization_router_cannot_finalize_twice() {
        let surface = InitSurface::new();
        let delivered = surface.deliver_key().await;
        let stale = surface.served();

        let first = serve(&stale, INIT_ROUTE, &finalization_body(&delivered.proof())).await;
        assert_eq!(first.status, StatusCode::OK, "{}", first.body);
        let anchors = surface.anchor_snapshot();

        let second = serve(&stale, INIT_ROUTE, &finalization_body(&delivered.proof())).await;
        assert_eq!(second.status, InitRejection::AlreadyInitialized.status());
        assert_eq!(surface.record_state(), LifecycleState::Initialized);
        assert_eq!(surface.anchor_snapshot(), anchors);
        assert_eq!(system_log_records(&surface.state_root).len(), 1);
        assert!(matches!(
            *surface.modes.borrow(),
            ServingMode::Operational(_)
        ));
    }

    // -----------------------------------------------------------------------
    // Proof decoding
    // -----------------------------------------------------------------------

    /// The proof is decoded from exactly one encoding: an untruncated
    /// unpadded URL-safe value of the full HMAC length.
    #[test]
    fn a_malformed_wrong_length_or_wrong_alphabet_proof_is_refused() {
        let canonical = URL_SAFE_NO_PAD.encode([7_u8; RECOVERY_PROOF_BYTES]);
        assert!(
            decode_proof(&canonical).is_some(),
            "the canonical encoding must decode"
        );

        for refused in [
            String::new(),
            URL_SAFE_NO_PAD.encode([7_u8; RECOVERY_PROOF_BYTES - 1]),
            URL_SAFE_NO_PAD.encode([7_u8; RECOVERY_PROOF_BYTES + 1]),
            format!("{canonical}="),
            // The standard alphabet renders `+` and `/`, which the URL-safe
            // decoder refuses rather than reinterprets.
            base64::engine::general_purpose::STANDARD.encode([0xff_u8; RECOVERY_PROOF_BYTES]),
            String::from("not a proof at all"),
        ] {
            assert!(decode_proof(&refused).is_none(), "{refused:?} must refuse");
        }
    }

    // -----------------------------------------------------------------------
    // Sealing, direct invocation, and the production inventory
    // -----------------------------------------------------------------------

    /// After sealing, neither Init route is mounted and the orchestrator itself
    /// refuses a finalization admitted with a real mutation-lane permit,
    /// leaving every durable artifact exactly as sealing left it.
    #[tokio::test]
    async fn a_sealed_deployment_refuses_a_direct_finalization_without_any_side_effect() {
        let surface = InitSurface::new();
        let delivered = surface.complete().await;
        let anchors = surface.anchor_snapshot();
        let records = system_log_records(&surface.state_root);
        assert_eq!(records.len(), 1);

        let after = surface.served();
        assert!(!serves(&after, INIT_ROUTE).await);
        assert!(!serves(&after, INIT_RECOVERY_KEY_ROUTE).await);

        let orchestrator: &Arc<InitOrchestrator> = surface.orchestrator();
        let permit = Arc::clone(&orchestrator.mutation_lane)
            .acquire_owned()
            .await
            .expect("the mutation lane is open");
        let mut context = axum::http::Extensions::new();
        context.insert(BodyAdmission::from_permit(permit));
        let rejection = orchestrator
            .finalize(InitFinalizeSubmission {
                request: direct_request(),
                recovery_key_proof: delivered.proof(),
                context,
            })
            .await
            .expect_err("a sealed deployment cannot be initialized again");

        assert_eq!(rejection, InitRejection::AlreadyInitialized);
        assert_eq!(surface.anchor_snapshot(), anchors);
        assert_eq!(system_log_records(&surface.state_root), records);
        assert!(matches!(
            *surface.modes.borrow(),
            ServingMode::Operational(_)
        ));
    }

    /// Builds a well-formed submission whose secrets are sentinels.
    ///
    /// The password is never read on the refusing path, so a value that could
    /// not be accepted proves the refusal happened before it was reached.
    fn direct_request() -> InitRequestSubmission {
        InitRequestSubmission {
            backend: SubmittedBackend::Sqlite,
            administrator: InitAdministratorSubmission {
                username: String::from(ADMINISTRATOR),
                display_name: None,
                password: Zeroizing::new(String::new()),
            },
            log_modules: Vec::new(),
            system_log: String::from(SYSTEM_ASSIGNMENT),
            audit_log: String::from(AUDIT_ASSIGNMENT),
        }
    }

    /// Init is judged against the inventory this build actually compiles in,
    /// not one a caller supplies, so a request naming a Log Module the shipped
    /// binary cannot serve is refused here exactly as it is there.
    #[tokio::test]
    async fn init_is_judged_against_the_production_component_inventory() {
        let components = server_components();
        let compiled_in = Name::new(weavelit_module_log_sqlite::MODULE_IDENTIFIER).unwrap();
        let absent = Name::new("postgres").unwrap();
        assert!(components.log_modules.contains_key(&compiled_in));
        assert!(!components.log_modules.contains_key(&absent));

        let surface = InitSurface::new();
        let delivered = surface.deliver_key().await;
        let modules = format!(
            "{},{}",
            log_module("postgres", SYSTEM_ASSIGNMENT, true),
            log_module("sqlite", AUDIT_ASSIGNMENT, true)
        );
        let served = surface
            .submit(
                INIT_ROUTE,
                &body_with(
                    &modules,
                    SYSTEM_ASSIGNMENT,
                    AUDIT_ASSIGNMENT,
                    Some(&delivered.proof()),
                ),
            )
            .await;

        assert_eq!(served.status, InitRejection::InitializationFailed.status());
        assert_eq!(
            surface.record_state(),
            LifecycleState::InitializationPending
        );
        assert!(system_log_records(&surface.state_root).is_empty());
    }

    // -----------------------------------------------------------------------
    // Transport profiles and route isolation
    // -----------------------------------------------------------------------

    /// A profile comes from the registration a surface actually mounted, never
    /// from the target string, so an unmounted route falls back to the
    /// listener's default bound and shared budget rather than keeping whatever
    /// its mounted form would have granted.
    #[tokio::test]
    async fn the_transport_profile_is_derived_from_the_mounted_registration() {
        let surface = InitSurface::new();

        let preoperational = surface.served();
        assert!(
            preoperational
                .registry()
                .registered_routes()
                .contains(&(Method::PUT, INIT_RECOVERY_KEY_ROUTE))
        );
        // The Restore artifact route is the one registration that grants an
        // admitted body, so it is the non-vacuous comparison here.
        let artifact = profile_for(
            &preoperational,
            weavelit_module_client::RESTORE_ARTIFACT_ROUTE,
        );
        assert_ne!(artifact, TransportProfile::DEFAULT);
        assert!(artifact.max_body_bytes() > TransportProfile::DEFAULT.max_body_bytes());
        assert_eq!(
            profile_for(&preoperational, INIT_RECOVERY_KEY_ROUTE),
            TransportProfile::DEFAULT
        );
        assert_eq!(
            profile_for(&preoperational, INIT_ROUTE),
            TransportProfile::DEFAULT
        );

        let delivered = surface.deliver_key().await;
        let finalization = surface.served();
        assert_eq!(
            finalization.registry().registered_routes(),
            vec![(Method::PUT, INIT_ROUTE)]
        );
        assert_eq!(
            profile_for(&finalization, INIT_ROUTE),
            TransportProfile::DEFAULT
        );
        // Unmounted here, so it grants nothing its mounted form would have.
        assert_eq!(
            profile_for(
                &finalization,
                weavelit_module_client::RESTORE_ARTIFACT_ROUTE
            ),
            TransportProfile::DEFAULT
        );

        let completed = surface
            .submit(INIT_ROUTE, &finalization_body(&delivered.proof()))
            .await;
        assert_eq!(completed.status, StatusCode::OK, "{}", completed.body);
        let operational = surface.served();
        for absent in [INIT_ROUTE, INIT_RECOVERY_KEY_ROUTE] {
            assert!(
                !operational
                    .registry()
                    .registered_routes()
                    .iter()
                    .any(|(_, target)| *target == absent),
                "{absent} must register nothing once sealed"
            );
            assert_eq!(profile_for(&operational, absent), TransportProfile::DEFAULT);
        }
    }

    /// An Init request is decided in the pre-body stage wherever it can be:
    /// a cross-origin submission and a request the lifecycle has moved past
    /// are both refused before a submitted secret is read or allocated.
    #[tokio::test]
    async fn an_ineligible_init_request_is_refused_before_its_body_is_read() {
        let surface = InitSurface::new();
        let body = preparation_body();
        // Snapshotted while preparation is mounted, exactly as the listener
        // snapshots the surface when it accepts a connection.
        let stale = surface.served();
        assert!(
            admit(&stale, INIT_RECOVERY_KEY_ROUTE, &body).await.is_ok(),
            "a same-origin preparation is admitted while none is pending"
        );

        let cross_origin = Request::builder()
            .method(Method::PUT)
            .uri(INIT_RECOVERY_KEY_ROUTE)
            .header(header::HOST, LISTENER)
            .header(header::ORIGIN, "https://elsewhere.example")
            .header("x-weavelit-csrf", "1")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ACCEPT, "application/json")
            .header(header::CONTENT_LENGTH, body.len().to_string())
            .body(Body::empty())
            .expect("the request head is well formed");
        assert_eq!(
            admit_head(&stale, cross_origin, &body).await.err(),
            Some(PreBodyRejection::RequestOriginDenied)
        );

        surface.deliver_key().await;
        assert_eq!(
            admit(&stale, INIT_RECOVERY_KEY_ROUTE, &body).await.err(),
            Some(PreBodyRejection::AlreadyInitialized),
            "the stale router still mounts preparation, so the lane must refuse it"
        );
        // The pre-body refusal is the code the mounted route would have given.
        assert_eq!(
            PreBodyRejection::AlreadyInitialized.response().status,
            InitRejection::AlreadyInitialized.status()
        );
        assert_eq!(
            surface.record_state(),
            LifecycleState::InitializationPending
        );
    }

    /// A refused preparation changes nothing another pre-operational route
    /// serves: both frozen responses stay byte for byte identical.
    #[tokio::test]
    async fn a_refused_preparation_leaves_the_other_preoperational_routes_identical() {
        let surface = InitSurface::new();
        let served = surface.served();
        let before: &Router = served.router();
        let status_before = rendered(before, Method::GET, STATUS_ROUTE).await;
        let database_before = rendered(before, Method::PUT, APPLICATION_DATABASE_ROUTE).await;
        assert_eq!(status_before.0, StatusCode::OK);
        assert!(!status_before.1.is_empty());
        assert!(!database_before.1.is_empty());
        let anchors = surface.anchor_snapshot();

        let refused = surface
            .submit(INIT_RECOVERY_KEY_ROUTE, "{\"database\":")
            .await;
        assert_eq!(refused.status, InitRejection::BadRequest.status());

        let after = surface.served();
        assert_eq!(
            rendered(after.router(), Method::GET, STATUS_ROUTE).await,
            status_before
        );
        assert_eq!(
            rendered(after.router(), Method::PUT, APPLICATION_DATABASE_ROUTE).await,
            database_before
        );
        assert!(serves(&after, RESTORE_ROUTE).await);
        assert!(serves(&after, INIT_RECOVERY_KEY_ROUTE).await);
        assert!(!serves(&after, INIT_ROUTE).await);
        assert_eq!(surface.record_state(), LifecycleState::Uninitialized);
        assert_eq!(surface.anchor_snapshot(), anchors);
    }

    // -----------------------------------------------------------------------
    // Restart and operational sign-in
    // -----------------------------------------------------------------------

    /// A restart over any retained Init checkpoint refuses to serve at all and
    /// changes nothing it found.
    #[tokio::test]
    async fn a_restart_over_an_init_checkpoint_reports_lifecycle_interrupted_and_mutates_nothing() {
        let surface = InitSurface::new();
        surface.deliver_key().await;
        assert_eq!(
            surface.record_state(),
            LifecycleState::InitializationPending
        );
        let anchors = surface.anchor_snapshot();
        assert!(!anchors.is_empty(), "the deployment record must exist");

        let (_root, state_root) = surface.release();
        let error = classify_restricted_startup(&state_root)
            .expect_err("a restart over an Init checkpoint must not serve");

        assert_eq!(error, StartupError::LifecycleInterruptedRedeployNew);
        assert_eq!(
            error.category_reason(),
            ("lifecycle_interrupted", "operator_redeploy_new")
        );
        assert_eq!(crate::tests::anchor_snapshot(&state_root), anchors);
    }

    /// A checkpoint whose key response was never written takes the same path,
    /// so a Server that failed closed mid-delivery cannot be restarted into a
    /// resumed Init.
    #[tokio::test]
    async fn a_restart_over_an_undelivered_checkpoint_also_refuses_to_serve() {
        let surface = InitSurface::new();
        surface
            .submit(INIT_RECOVERY_KEY_ROUTE, &preparation_body())
            .await
            .discard();
        let anchors = surface.anchor_snapshot();

        let (_root, state_root) = surface.release();
        let error = classify_restricted_startup(&state_root)
            .expect_err("a restart over an undelivered checkpoint must not serve");

        assert_eq!(
            error.category_reason(),
            ("lifecycle_interrupted", "operator_redeploy_new")
        );
        assert_eq!(crate::tests::anchor_snapshot(&state_root), anchors);
    }

    /// A completed Init seeds an Administrator who can sign in through the
    /// operational surface the same process activated, over the same real
    /// SQLite deployment and the clock the shipped binary uses.
    #[tokio::test]
    async fn a_completed_init_signs_in_the_seeded_administrator() {
        let surface = InitSurface::production();
        surface.complete().await;

        let records = system_log_records(&surface.state_root);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].0, INIT_CLASSIFICATION);
        assert!(
            records[0].1 > 0,
            "the shipped clock stamps a real event time"
        );

        assert_signs_in(&surface.served()).await;
    }

    /// The sealed deployment survives a restart: a new startup classifies it as
    /// initialized, serves the operational surface, and the seeded
    /// Administrator signs in against the reopened Application Database.
    #[tokio::test]
    async fn a_sealed_deployment_survives_a_restart_and_still_signs_in() {
        let surface = InitSurface::production();
        surface.complete().await;
        let anchors = surface.anchor_snapshot();
        let records = system_log_records(&surface.state_root);

        let (_root, state_root) = surface.release();
        let startup =
            classify_restricted_startup(&state_root).expect("the sealed deployment reopens");
        assert_eq!(startup.outcome(), StartupOutcome::Initialized);

        let (switch, modes) = ServingModeSwitch::new(ServingMode::FailClosed(
            MountedSurface::without_registrations(fallback_router()),
        ));
        let switch = Arc::new(switch);
        let composer =
            PreoperationalComposer::with_clock(&startup, listener(), &switch, system_event_clock());
        composer.publish_initial(startup.composition.outcome);
        assert!(matches!(*modes.borrow(), ServingMode::Operational(_)));

        let served = modes.borrow().surface().clone();
        for absent in [INIT_ROUTE, INIT_RECOVERY_KEY_ROUTE, RESTORE_ROUTE] {
            assert!(!serves(&served, absent).await, "{absent} must be absent");
        }

        // A restart reads the state it found; it does not rewrite it. Asserted
        // before signing in, because signing in legitimately appends records of
        // its own and would mask a restart that had rewritten what it found.
        assert_eq!(crate::tests::anchor_snapshot(&state_root), anchors);
        assert_eq!(system_log_records(&state_root), records);

        assert_signs_in(&served).await;
    }

    /// Signs the seeded Administrator in and validates the session it issued.
    async fn assert_signs_in(surface: &MountedSurface) {
        let denied = serve(
            surface,
            AUTH_LOGIN_ROUTE,
            &login_body(ADMINISTRATOR, "not the password"),
        )
        .await;
        assert_eq!(denied.status, StatusCode::UNAUTHORIZED, "{}", denied.body);

        let signed_in = serve(
            surface,
            AUTH_LOGIN_ROUTE,
            &login_body(ADMINISTRATOR, ADMINISTRATOR_PASSWORD),
        )
        .await;
        assert_eq!(signed_in.status, StatusCode::OK, "{}", signed_in.body);

        let cookie = signed_in.session_cookie();
        assert!(cookie.starts_with(SESSION_COOKIE_NAME));
        assert!(
            cookie.len() > SESSION_COOKIE_NAME.len() + 1,
            "the session cookie must carry a value"
        );

        // The session route enforces a double-submit check, so the request
        // carries both cookies and the header value the CSRF cookie holds. A
        // request that presents the session cookie alone is refused, and the
        // assertion below proves that refusal is still in force.
        let csrf = signed_in.cookie(CSRF_COOKIE_NAME);
        let csrf_value = csrf
            .split_once('=')
            .expect("the CSRF cookie is rendered as a name-value pair")
            .1
            .to_owned();
        assert!(!csrf_value.is_empty(), "the CSRF cookie must carry a value");

        let unaccompanied = serve_head(
            surface,
            head(AUTH_SESSION_ROUTE, 0, Some(&cookie), Some("1")),
            "",
        )
        .await;
        assert_eq!(
            unaccompanied.status,
            StatusCode::UNAUTHORIZED,
            "{}",
            unaccompanied.body
        );

        let session = serve_head(
            surface,
            head(
                AUTH_SESSION_ROUTE,
                0,
                Some(&format!("{cookie}; {csrf}")),
                Some(&csrf_value),
            ),
            "",
        )
        .await;
        assert_eq!(session.status, StatusCode::OK, "{}", session.body);
    }
}
