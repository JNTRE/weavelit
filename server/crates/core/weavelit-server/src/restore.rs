//! Server-owned Restore orchestration and its two-step submission protocol.
//!
//! The cryptographic validation crate and the lifecycle typestate chain each
//! own one half of a Restore. This module is the only place that joins them:
//! it authorizes the workflow, validates the submitted backup, builds the
//! replacement application state, moves the deployment through its checkpoint,
//! delivers the System Log acknowledgement to the destination the restored
//! backup itself declares, seals the deployment, and activates normal
//! operation in-process.
//!
//! A Restore is submitted in two requests. The first submits the recovery key
//! alone; this module retains it, issues a one-time ticket, and returns only
//! the ticket. The second presents that ticket and uploads the encrypted
//! artifact. The recovery key therefore never travels with the artifact, and
//! the artifact is never admitted without a ticket this module issued.
//!
//! This module owns no wire format. The shared Client Module crate owns the
//! routes, schemas, and rendered responses; this module owns the ticket store,
//! the lifecycle eligibility re-checks, the admission registrations, and the
//! orchestration behind them.

use std::{
    future::Future,
    path::PathBuf,
    pin::Pin,
    sync::{Arc, Mutex, PoisonError},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::http::Method;
use tokio::{
    sync::Semaphore,
    task,
    time::{Instant as Deadline, sleep_until},
};
use weavelit_module_client::{
    ExpectedOrigin, RESTORE_ARTIFACT_ROUTE, RESTORE_ROUTE, RestoreArtifactSubmission,
    RestoreCapability, RestoreCompleted, RestoreKeySubmission, RestoreRejection,
    RestoreTicketIssued, submitted_restore_ticket, validate_restore_artifact_request,
    validate_restore_key_request,
};
use weavelit_server_database::ReconciliationDigest;
use weavelit_server_lifecycle::{
    ApplicationState, BackendCatalog, CheckpointMetadata, DeploymentIdentifier, InitializedState,
    LifecycleError, LifecycleState, SealedDeployment, StateIdentifier, TrustedBackendContext,
    WorkflowArbiter, WorkflowError, WorkflowKind, WorkflowPermit,
};
use weavelit_server_log::{
    CompleteLogRecord, ConfiguredLogDestination, LogModuleCatalog, LogModuleIdentifier,
    TrustedLogModuleContext, TrustedRecordIssuer,
};
use weavelit_server_log_authority::ServerLogAuthority;
use weavelit_server_observability::ServerObservability;
use weavelit_server_restore::{
    AvailableComponents, LogAssignment, LogModuleConfiguration, LogType,
    MAX_CONCURRENT_RESTORE_OPERATIONS, MAX_ENCRYPTED_ARTIFACT_BYTES, RequestBudget,
    RequestDeadline, RestoreAuthority, RestoreError, RestoreRequest, RestoreTarget, RestoreTicket,
    RestoreTicketDigest, RestoreValidator, TOTAL_REQUEST_DEADLINE, UPLOAD_DEADLINE,
    ValidatedBackup, build_application_state,
};
use zeroize::Zeroizing;

use crate::{
    LifecycleTransitionGate, RestrictedStartup, ServingModeSwitch,
    operational::{OperationalComposer, OperationalDatabase, OperationalRuntime},
    reconciliation::{RECONCILIATION_CAPABILITY_ENTROPY_BYTES, ReconciliationCapability},
    transport::{
        AdmittedCheck, BodyAdmission, PreBodyCheck, PreBodyGrant, PreBodyGrantValue,
        PreBodyRejection, TransportProfile, TransportRegistration,
    },
};

/// Opaque non-secret checkpoint metadata this workflow writes.
///
/// The lifecycle crate stores it without interpretation, so it carries only
/// the workflow's own fixed marker and never any backup content.
const RESTORE_CHECKPOINT_METADATA: &[u8] = b"weavelit.restore.v1";

/// The transport profile the encrypted artifact upload is admitted under.
///
/// The read budget is the approved upload deadline and the handling budget is
/// the approved total deadline. Both are further capped at what remains of the
/// budget the recovery-key submission started, so neither can restart it.
const RESTORE_ARTIFACT_PROFILE: TransportProfile = TransportProfile::admitted(
    MAX_ENCRYPTED_ARTIFACT_BYTES,
    UPLOAD_DEADLINE,
    TOTAL_REQUEST_DEADLINE,
);

/// The mutation lane is the Restore admission lane, so the approved concurrency
/// bound and the lane's single permit cannot drift apart.
const _: () = assert!(
    MAX_CONCURRENT_RESTORE_OPERATIONS == 1,
    "the pre-operational mutation lane admits exactly one Restore at a time"
);

/// The pause a test drives the blocking replacement chain through.
///
/// The chain is uninterruptible by construction, so the only way to place a
/// stop inside its irreversible region deterministically is for the chain
/// itself to announce that it is already inside one.
#[cfg(test)]
pub(crate) type ReplacementHook = Arc<dyn Fn() + Send + Sync>;

/// Server-owned composition that runs one Restore at a time.
///
/// It shares the startup composition's `WorkflowArbiter` and mutation lane, so
/// a Restore serializes against pre-operational database selection rather than
/// racing it.
pub struct RestoreOrchestrator {
    arbiter: Arc<WorkflowArbiter>,
    catalog: Arc<BackendCatalog>,
    context: Arc<TrustedBackendContext>,
    log_catalog: Arc<LogModuleCatalog>,
    state_root: PathBuf,
    mutation_lane: Arc<Semaphore>,
    /// The gate this workflow's irreversible replacement region runs inside,
    /// so a signalled shutdown waits for that region instead of exiting inside
    /// it and stranding durable state.
    transition_gate: Arc<LifecycleTransitionGate>,
    pending: Arc<PendingRestoreSlot>,
    serving_modes: Arc<ServingModeSwitch>,
    validator: RestoreValidator,
    observability: ServerObservability,
    /// Retained privately so no caller outside this composition can mint a
    /// trusted Log Module context or a trusted record issuer.
    log_authority: ServerLogAuthority,
    /// The Server-wide values the handed-over operational surface composes from.
    operational_runtime: Arc<OperationalRuntime>,
    /// The operational composition a completed Restore hands over.
    ///
    /// Sealing returns the database the workflow committed through, so this
    /// retains that same open handle for the operational runtime instead of
    /// letting it close and reopening the target afterwards.
    operational: Mutex<Option<OperationalComposer>>,
    /// The pause a test drives the blocking replacement chain through.
    #[cfg(test)]
    replacement_hook: Mutex<Option<ReplacementHook>>,
}

struct ReplaceStateInput<'a> {
    state: &'a ApplicationState,
    reconciliation_digest: &'a ReconciliationDigest,
    log_module: &'a LogModuleIdentifier,
    deployment_identifier: DeploymentIdentifier,
    record_identifier: StateIdentifier,
    record: &'a CompleteLogRecord,
}

impl RestoreOrchestrator {
    /// Composes Restore over a restricted startup's lifecycle authority.
    ///
    /// `components` is the inventory a backup may reference. The runtime call
    /// site supplies the Server's compiled-in Client, MFA, Service, and Log
    /// Module names; a test may supply a narrower or wider inventory to drive
    /// a compatibility decision it wants to observe.
    ///
    /// `operational_runtime` is the same value startup composes its own
    /// operational surface from, so a deployment sealed by a Restore serves the
    /// routes a deployment sealed at startup serves.
    #[must_use]
    pub fn new(
        startup: &RestrictedStartup,
        components: AvailableComponents,
        serving_modes: Arc<ServingModeSwitch>,
        operational_runtime: Arc<OperationalRuntime>,
    ) -> Arc<Self> {
        let log_authority = ServerLogAuthority::new();
        let observability =
            ServerObservability::new(TrustedRecordIssuer::from_server_authority(&log_authority));

        Arc::new(Self {
            arbiter: Arc::clone(&startup.composition.adapter.arbiter),
            catalog: Arc::clone(&startup.composition.catalog),
            context: Arc::clone(&startup.composition.context),
            log_catalog: Arc::clone(&startup.log_catalog),
            state_root: startup.state_root.clone(),
            mutation_lane: Arc::clone(&startup.composition.adapter.mutation_lane),
            transition_gate: Arc::clone(&startup.composition.adapter.transition_gate),
            pending: Arc::new(PendingRestoreSlot::default()),
            serving_modes,
            validator: RestoreValidator::new(components),
            observability,
            log_authority,
            operational_runtime,
            operational: Mutex::new(None),
            #[cfg(test)]
            replacement_hook: Mutex::new(None),
        })
    }

    /// Returns the Application Database a completed Restore handed over.
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

    /// Returns the Restore capability a Client Module declares this Server with.
    #[must_use]
    pub fn capability(self: &Arc<Self>, expected_origin: ExpectedOrigin) -> RestoreCapability {
        let submitting = Arc::clone(self);
        let uploading = Arc::clone(self);
        RestoreCapability {
            expected_origin,
            max_artifact_bytes: MAX_ENCRYPTED_ARTIFACT_BYTES,
            submit_key: Arc::new(move |submission: RestoreKeySubmission| {
                let orchestrator = Arc::clone(&submitting);
                Box::pin(async move {
                    orchestrator.submit_recovery_key(&submission.context, submission.recovery_key)
                })
            }),
            upload_artifact: Arc::new(move |submission: RestoreArtifactSubmission| {
                let orchestrator = Arc::clone(&uploading);
                Box::pin(async move { orchestrator.upload_artifact(submission).await })
            }),
        }
    }

    /// Returns the registration that admits a recovery-key submission.
    ///
    /// The submission carries no artifact, so it keeps the listener's default
    /// body bound and its default read budget. What it does add is the Restore
    /// admission lane and the lifecycle eligibility re-check, so the recovery
    /// key is read only while this Server still holds the only Restore slot and
    /// only while the deployment still permits a Restore.
    #[must_use]
    pub fn key_registration(
        self: &Arc<Self>,
        expected_origin: ExpectedOrigin,
    ) -> TransportRegistration {
        TransportRegistration::new(Method::PUT, RESTORE_ROUTE, TransportProfile::DEFAULT)
            .with_pre_body_check(Arc::new(RestoreKeyCheck { expected_origin }))
            .with_admission(Arc::clone(&self.mutation_lane))
            .with_admitted_check(self.eligibility())
    }

    /// Returns the registration that admits an encrypted artifact upload.
    ///
    /// The ticket is claimed in the pre-body check, before the artifact bound
    /// is allocated, so an unticketed upload never costs this Server memory.
    #[must_use]
    pub fn artifact_registration(
        self: &Arc<Self>,
        expected_origin: ExpectedOrigin,
    ) -> TransportRegistration {
        TransportRegistration::new(
            Method::PUT,
            RESTORE_ARTIFACT_ROUTE,
            RESTORE_ARTIFACT_PROFILE,
        )
        .with_pre_body_check(Arc::new(RestoreArtifactCheck {
            expected_origin,
            pending: Arc::clone(&self.pending),
        }))
        .with_admission(Arc::clone(&self.mutation_lane))
        .with_admitted_check(self.eligibility())
    }

    fn eligibility(self: &Arc<Self>) -> Arc<dyn AdmittedCheck> {
        Arc::new(RestoreEligibility {
            arbiter: Arc::clone(&self.arbiter),
        })
    }

    /// Retains a submitted recovery key and issues its one-time ticket.
    ///
    /// The ticket is independent, cryptographically random bearer material. It
    /// is never derived from the correlation identifier, and only its digest is
    /// retained, so this Server cannot reproduce a ticket it already returned.
    fn submit_recovery_key(
        self: &Arc<Self>,
        context: &axum::http::Extensions,
        recovery_key: Zeroizing<String>,
    ) -> Result<RestoreTicketIssued, RestoreRejection> {
        let admission = context
            .get::<PreBodyGrantValue>()
            .and_then(PreBodyGrantValue::get::<RestoreKeyAdmission>)
            .ok_or(RestoreRejection::RestoreFailed)?;
        // The budget started before the recovery key was read, and the ticket
        // inherits it unchanged.
        let budget = admission.budget;
        let remaining = budget.remaining().ok_or(RestoreRejection::RestoreFailed)?;

        let ticket = RestoreTicket::from_zeroizing_entropy(
            crate::authentication::random_zeroizing_bytes()
                .ok_or(RestoreRejection::RestoreFailed)?,
        );
        let digest = ticket.digest();
        let reconciliation =
            ReconciliationCapability::from_zeroizing_entropy(
                crate::authentication::random_zeroizing_bytes::<
                    RECONCILIATION_CAPABILITY_ENTROPY_BYTES,
                >()
                .ok_or(RestoreRejection::RestoreFailed)?,
            );
        let reconciliation_digest = reconciliation.digest();
        let correlation_identifier = correlation_identifier().map_err(restore_rejection)?;
        let expires_at = Deadline::now() + UPLOAD_DEADLINE.min(remaining);

        self.pending
            .issue(PendingRestore {
                digest,
                reconciliation_digest,
                recovery_key,
                correlation_identifier: correlation_identifier.clone(),
                budget,
                expires_at,
            })
            .map_err(restore_rejection)?;

        // An expired ticket is destroyed on its own schedule rather than
        // waiting for a later request to notice it, so an abandoned submission
        // does not leave a recovery key resident for the listener's lifetime.
        let pending = Arc::clone(&self.pending);
        task::spawn(async move {
            sleep_until(expires_at).await;
            pending.expire(&digest);
        });

        Ok(RestoreTicketIssued {
            ticket: Zeroizing::new(ticket.as_str().to_owned()),
            correlation_id: correlation_identifier,
            reconciliation_capability: Zeroizing::new(reconciliation.as_str().to_owned()),
        })
    }

    /// Runs one Restore against an already-claimed ticket.
    async fn upload_artifact(
        self: &Arc<Self>,
        submission: RestoreArtifactSubmission,
    ) -> Result<RestoreCompleted, RestoreRejection> {
        // The pre-body check already consumed the pending entry. Taking it here
        // moves the recovery key out of the request extensions, so it is owned
        // by this operation and cleared when the operation ends.
        let claimed = submission
            .context
            .get::<PreBodyGrantValue>()
            .and_then(PreBodyGrantValue::get::<ClaimedRestore>)
            .and_then(ClaimedRestore::take)
            .ok_or(RestoreRejection::RestoreTicketInvalid)?;
        // Carried in from admission: the lane was acquired before the artifact
        // was allocated, and it is held until this operation finishes.
        let admission = submission
            .context
            .get::<BodyAdmission>()
            .cloned()
            .ok_or(RestoreRejection::RestoreFailed)?;

        let correlation_identifier = claimed.correlation_identifier;
        let completed = RestoreCompleted {
            correlation_id: correlation_identifier.clone(),
        };
        self.restore(
            admission,
            claimed.budget,
            correlation_identifier,
            claimed.reconciliation_digest,
            owned_bytes(submission.artifact),
            claimed.recovery_key,
        )
        .await
        .map(|_| completed)
        .map_err(restore_rejection)
    }

    /// Runs one Restore from an already-admitted artifact and recovery key.
    ///
    /// The admission permit, the request budget, and the correlation identifier
    /// are all inherited from the transport that admitted the upload. Nothing
    /// here acquires a lane or starts a budget of its own, so the memory bound
    /// and the total deadline are the ones enforced before the artifact was
    /// allocated rather than new ones granted after it.
    ///
    /// Ownership of both sensitive inputs is taken so they can be cleared
    /// inside the operation instead of outliving it in a caller's buffer.
    ///
    /// Returns the sealed deployment's loaded state on success. The Server is
    /// already serving its operational surface by then.
    pub async fn restore(
        self: &Arc<Self>,
        admission: BodyAdmission,
        budget: RequestBudget,
        correlation_identifier: String,
        reconciliation_digest: ReconciliationDigest,
        artifact: Vec<u8>,
        recovery_key: Zeroizing<String>,
    ) -> Result<InitializedState, RestoreError> {
        let artifact = Zeroizing::new(artifact);
        let orchestrator = Arc::clone(self);

        // The entire authorize-through-seal chain is blocking work. Running it
        // in one closure keeps the workflow permit, the checkpoint, and the
        // seal on a single thread and outside any cancellation point, so no
        // caller timeout can abandon the deployment mid-replacement.
        task::spawn_blocking(move || {
            let _admission = admission;
            orchestrator.run(
                &budget,
                &correlation_identifier,
                &reconciliation_digest,
                artifact,
                recovery_key,
            )
        })
        .await
        .map_err(|_| RestoreError::RestoreFailed)?
    }

    /// Drives the blocking Restore chain against a supplied deadline.
    ///
    /// The public entry point takes a [`RequestBudget`] no caller can lengthen.
    /// This exists only so a test can place an overrun at an exact step of the
    /// chain instead of waiting out the approved total deadline in real time.
    #[cfg(test)]
    pub(crate) fn run_against_deadline(
        &self,
        deadline: &dyn RequestDeadline,
        correlation_identifier: &str,
        artifact: Zeroizing<Vec<u8>>,
        recovery_key: Zeroizing<String>,
    ) -> Result<InitializedState, RestoreError> {
        self.run(
            deadline,
            correlation_identifier,
            &ReconciliationDigest::from_bytes([1; 32]),
            artifact,
            recovery_key,
        )
    }

    /// Runs the blocking Restore chain.
    fn run(
        &self,
        budget: &dyn RequestDeadline,
        correlation_identifier: &str,
        reconciliation_digest: &ReconciliationDigest,
        artifact: Zeroizing<Vec<u8>>,
        recovery_key: Zeroizing<String>,
    ) -> Result<InitializedState, RestoreError> {
        let permit = self
            .arbiter
            .authorize_workflow(&self.catalog, &self.context)
            .map_err(map_workflow_error)?;

        // The lifecycle authority is consulted before any sensitive input is
        // read: this target exists only because `authorize_workflow` already
        // re-verified the deployment record and the selected database under
        // the exclusive permit held for the rest of this operation.
        let authority = AuthorizedTarget(RestoreTarget::new(
            permit.deployment_identifier(),
            permit.selected_backend().clone(),
            permit.audit_reference_persistence(),
        ));

        let validated = self.validator.validate(
            &authority,
            budget,
            RestoreRequest {
                artifact: &artifact,
                recovery_key: &recovery_key,
            },
        );
        // Neither input is needed after validation, so both are cleared here
        // rather than at the end of the operation.
        drop(recovery_key);
        drop(artifact);
        let validated = validated?;

        let deployment_identifier = validated.deployment_identifier();
        let record_identifier = StateIdentifier::from_bytes(random_bytes()?)
            .map_err(|_| RestoreError::RestoreFailed)?;

        let (record, obligation) = self
            .observability
            .prepare_restore_completion(
                record_identifier,
                deployment_identifier,
                event_time_milliseconds()?,
                correlation_identifier,
            )
            .map_err(|_| RestoreError::RestoreFailed)?
            .into_parts();

        let state = build_application_state(&validated, permit.sealer(), obligation)?;
        // Resolved before the point of no return so a backup naming a Log
        // Module this Server cannot serve fails while failing is still free.
        let log_module = system_log_module(&validated)?;
        drop(validated);

        // Resealing a backup at the collection limits is itself substantial
        // work, so the deadline is observed once more before it stops being
        // free to abandon.
        budget.check().map_err(|_| RestoreError::RestoreFailed)?;

        // Entered before the point of no return and held past it, so a stop
        // signalled from here on waits for the replacement instead of exiting
        // between the publication below and the sealed record. A gate already
        // closed refuses entry, and that refusal is reported as the same
        // `restore_failed` every other abandoned step reports, so no new
        // outcome becomes visible to a submitter.
        let transition = self
            .transition_gate
            .try_enter()
            .ok_or(RestoreError::RestoreFailed)?;

        // Point of no return begins here. Every later connection observes the
        // fail-closed surface before any durable state changes, and keeps
        // observing it if the replacement does not complete.
        self.serving_modes.publish_fail_closed();
        #[cfg(test)]
        self.pause_replacement();

        let sealed = self.replace_state(
            permit,
            ReplaceStateInput {
                state: &state,
                reconciliation_digest,
                log_module: &log_module,
                deployment_identifier,
                record_identifier,
                record: &record,
            },
        )?;

        // The database sealing handed back is retained here and composed into
        // the operational surface, so normal operation begins on the handle the
        // replacement committed through rather than on a newly opened one.
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
        // The replacement is sealed and the database it committed through is
        // registered, so a shutdown from here on closes the handle this took
        // over. Releasing any earlier would let a shutdown find the slot still
        // empty, close nothing, and leave this activation's writes behind.
        drop(transition);
        Ok(state)
    }

    /// Runs the installed pause, if any, inside the irreversible region.
    ///
    /// The hook is cloned out of its lock before it runs, so a parked chain
    /// holds nothing the test needs.
    #[cfg(test)]
    fn pause_replacement(&self) {
        let hook = self
            .replacement_hook
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(hook) = hook {
            hook();
        }
    }

    /// Installs the pause a test drives the blocking replacement chain through.
    ///
    /// It runs after the gate admitted this Restore and before any durable
    /// state is replaced, so a test that acts from inside it acts on a region
    /// that is already occupied rather than on one being entered.
    #[cfg(test)]
    pub(crate) fn pause_replacement_with(&self, hook: ReplacementHook) {
        *self
            .replacement_hook
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(hook);
    }

    /// Replaces retained state atomically and seals the deployment.
    ///
    /// A failure anywhere in this chain leaves the Server fail-closed with the
    /// interrupted state retained. No rollback is attempted, because the
    /// replaced state is exactly what an operator asked to discard.
    ///
    /// Completing the checkpoint also clears every live session, inside the
    /// same commit that installs the replacement state. Session invalidation is
    /// therefore not a follow-up step that an interruption could skip: either
    /// the replacement and the clearing both commit, or neither does.
    ///
    /// The sealed deployment it returns still owns the database this chain
    /// committed through, so the caller activates normal operation on that
    /// handle instead of reopening the target.
    fn replace_state(
        &self,
        permit: WorkflowPermit<'_>,
        input: ReplaceStateInput<'_>,
    ) -> Result<SealedDeployment, RestoreError> {
        let metadata = CheckpointMetadata::from_bytes(RESTORE_CHECKPOINT_METADATA)
            .map_err(|_| RestoreError::RestoreFailed)?;

        let committed = permit
            .create_checkpoint(WorkflowKind::Restore, metadata)
            .map_err(map_workflow_error)?
            .complete_checkpoint(input.state, input.reconciliation_digest)
            .map_err(map_workflow_error)?;

        // Opened only now: creating the Log Module's local storage before the
        // checkpoint would write durable state a pre-checkpoint failure had
        // promised not to leave behind.
        self.open_system_log(input.log_module, input.deployment_identifier)?
            .deliver(input.record)
            .map_err(|_| RestoreError::RestoreFailed)?;

        committed
            .acknowledge_completion(input.record_identifier)
            .map_err(map_workflow_error)?
            .seal()
            .map_err(map_workflow_error)
    }

    /// Builds the System Log destination under the trusted Log Module context.
    fn open_system_log(
        &self,
        module: &LogModuleIdentifier,
        deployment_identifier: DeploymentIdentifier,
    ) -> Result<ConfiguredLogDestination, RestoreError> {
        let context = TrustedLogModuleContext::from_server_authority(
            &self.log_authority,
            self.state_root.clone(),
            *deployment_identifier.as_bytes(),
        );
        self.log_catalog
            .create_destination(module, &context)
            .map_err(|_| RestoreError::RestoreFailed)
    }
}

/// Restore authority backed by an already-granted workflow permit.
struct AuthorizedTarget(RestoreTarget);

impl RestoreAuthority for AuthorizedTarget {
    fn authorize(&self) -> Result<RestoreTarget, RestoreError> {
        Ok(self.0.clone())
    }
}

// ---------------------------------------------------------------------------
// Pending submission store
// ---------------------------------------------------------------------------

/// A retained recovery-key submission awaiting its artifact upload.
///
/// The ticket itself is not here: only its digest is, so this Server cannot
/// reproduce, log, or leak a ticket it already returned. The recovery key is
/// owned and cleared whenever this value is dropped, which is what every
/// destroying path below relies on.
struct PendingRestore {
    digest: RestoreTicketDigest,
    reconciliation_digest: ReconciliationDigest,
    recovery_key: Zeroizing<String>,
    correlation_identifier: String,
    budget: RequestBudget,
    expires_at: Deadline,
}

/// The single outstanding pending Restore.
///
/// At most one submission is retained at a time. Issuing while one is
/// outstanding is rejected, and every claim consumes the entry whether or not
/// it succeeds, so a replay, a concurrent claim, a wrong ticket, and an expired
/// ticket all destroy the retained recovery key rather than leaving it
/// available for another attempt.
#[derive(Default)]
struct PendingRestoreSlot {
    entry: Mutex<Option<PendingRestore>>,
}

impl PendingRestoreSlot {
    /// Retains one submission, rejecting a second outstanding one.
    fn issue(&self, pending: PendingRestore) -> Result<(), RestoreError> {
        let mut entry = self.held();
        if entry
            .as_ref()
            .is_some_and(|held| held.expires_at > Deadline::now())
        {
            return Err(RestoreError::RestorePending);
        }
        // Assigning drops any expired entry still held, clearing its key.
        *entry = Some(pending);
        Ok(())
    }

    /// Consumes the outstanding submission and returns it only to its ticket.
    ///
    /// The entry is taken before the ticket is compared, so a failed claim is
    /// not retryable: the recovery key is dropped on the way out of this
    /// function no matter which check rejected it.
    fn claim(&self, ticket: &str) -> Result<PendingRestore, PreBodyRejection> {
        let submitted = RestoreTicketDigest::of(ticket);
        let pending = self.held().take();

        let pending = pending.ok_or(PreBodyRejection::RestoreTicketInvalid)?;
        if !pending.digest.matches(&submitted) || pending.expires_at <= Deadline::now() {
            return Err(PreBodyRejection::RestoreTicketInvalid);
        }
        Ok(pending)
    }

    /// Destroys the outstanding submission once its ticket has expired.
    fn expire(&self, digest: &RestoreTicketDigest) {
        let mut entry = self.held();
        let expired = entry
            .as_ref()
            .is_some_and(|held| held.digest.matches(digest) && held.expires_at <= Deadline::now());
        if expired {
            *entry = None;
        }
    }

    fn held(&self) -> std::sync::MutexGuard<'_, Option<PendingRestore>> {
        // A poisoned lane still holds a recovery key that must be destroyed, so
        // recovering the guard is what keeps every destroying path reachable.
        self.entry.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The claimed submission a validated artifact upload carries to its handler.
///
/// It travels in the request extensions, so a client disconnect, an expired
/// body deadline, an unreadable body, or any later failure drops it with the
/// request and clears the recovery key it holds.
struct ClaimedRestore {
    pending: Mutex<Option<PendingRestore>>,
}

impl ClaimedRestore {
    fn new(pending: PendingRestore) -> Self {
        Self {
            pending: Mutex::new(Some(pending)),
        }
    }

    /// Takes the claimed submission exactly once.
    fn take(&self) -> Option<PendingRestore> {
        self.pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }
}

/// The request budget a recovery-key submission started.
struct RestoreKeyAdmission {
    budget: RequestBudget,
}

// ---------------------------------------------------------------------------
// Admission checks
// ---------------------------------------------------------------------------

/// Head validation for the recovery-key submission.
struct RestoreKeyCheck {
    expected_origin: ExpectedOrigin,
}

impl PreBodyCheck for RestoreKeyCheck {
    fn check(
        &self,
        method: &Method,
        _uri: &axum::http::Uri,
        headers: &axum::http::HeaderMap,
    ) -> Result<PreBodyGrant, PreBodyRejection> {
        validate_restore_key_request(method, headers, self.expected_origin)
            .map_err(pre_body_rejection)?;
        // The total request budget starts here, before any recovery key byte is
        // read, and the artifact upload later inherits exactly this budget.
        Ok(
            PreBodyGrant::accepted().with_value(PreBodyGrantValue::new(RestoreKeyAdmission {
                budget: RequestBudget::start(),
            })),
        )
    }
}

/// Head validation and ticket claim for the artifact upload.
struct RestoreArtifactCheck {
    expected_origin: ExpectedOrigin,
    pending: Arc<PendingRestoreSlot>,
}

impl PreBodyCheck for RestoreArtifactCheck {
    fn check(
        &self,
        method: &Method,
        _uri: &axum::http::Uri,
        headers: &axum::http::HeaderMap,
    ) -> Result<PreBodyGrant, PreBodyRejection> {
        validate_restore_artifact_request(method, headers, self.expected_origin)
            .map_err(pre_body_rejection)?;
        let ticket = submitted_restore_ticket(headers).map_err(pre_body_rejection)?;
        // Claimed before the artifact bound is allocated, so an unticketed or
        // replayed upload costs this Server no memory at all.
        let claimed = self.pending.claim(ticket)?;
        let remaining = claimed
            .budget
            .remaining()
            .ok_or(PreBodyRejection::RestoreTicketInvalid)?;
        Ok(PreBodyGrant::accepted()
            .with_remaining_budget(remaining)
            .with_value(PreBodyGrantValue::new(ClaimedRestore::new(claimed))))
    }
}

/// Lifecycle eligibility re-checked under the acquired mutation lane.
///
/// Route absence is necessary but not sufficient. The listener snapshots the
/// router when it accepts a connection, so a connection accepted before a
/// fail-closed publication still holds a router that mounts Restore. Reading
/// the authoritative deployment record here, under the same lane a database
/// selection and a checkpoint take, rejects that stale request before anything
/// sensitive is read or allocated.
struct RestoreEligibility {
    arbiter: Arc<WorkflowArbiter>,
}

impl AdmittedCheck for RestoreEligibility {
    fn check(&self) -> Pin<Box<dyn Future<Output = Result<(), PreBodyRejection>> + Send + '_>> {
        let arbiter = Arc::clone(&self.arbiter);
        Box::pin(async move {
            let eligible = task::spawn_blocking(move || {
                arbiter.record_state() == LifecycleState::Uninitialized
                    && arbiter
                        .projection()
                        .is_ok_and(|projection| projection.database_selected())
            })
            .await
            .unwrap_or(false);
            if eligible {
                Ok(())
            } else {
                Err(PreBodyRejection::RestoreNotAllowed)
            }
        })
    }
}

/// Maps a route-level rejection onto the transport's closed rejection set.
fn pre_body_rejection(rejection: RestoreRejection) -> PreBodyRejection {
    match rejection {
        RestoreRejection::RequestOriginDenied => PreBodyRejection::RequestOriginDenied,
        RestoreRejection::RestoreTicketInvalid => PreBodyRejection::RestoreTicketInvalid,
        RestoreRejection::RestoreNotAllowed => PreBodyRejection::RestoreNotAllowed,
        _ => PreBodyRejection::BadRequest,
    }
}

/// Maps a stable Restore failure onto its documented route rejection.
fn restore_rejection(error: RestoreError) -> RestoreRejection {
    match error {
        RestoreError::RecoveryKeyInvalid => RestoreRejection::RecoveryKeyInvalid,
        RestoreError::BackupInvalid => RestoreRejection::BackupInvalid,
        RestoreError::BackupIncompatible => RestoreRejection::BackupIncompatible,
        RestoreError::RestorePending => RestoreRejection::RestorePending,
        RestoreError::Lifecycle(LifecycleError::InvalidState) => {
            RestoreRejection::RestoreNotAllowed
        }
        RestoreError::Lifecycle(_) => RestoreRejection::ServiceUnavailable,
        _ => RestoreRejection::RestoreFailed,
    }
}

/// Takes ownership of an admitted body without copying it when it is unique.
///
/// The listener allocated exactly one artifact buffer under the Restore lane.
/// Reusing that allocation keeps the resident cost at the approved bound
/// instead of briefly doubling it on the way into the orchestration.
fn owned_bytes(artifact: axum::body::Bytes) -> Vec<u8> {
    artifact
        .try_into_mut()
        .map_or_else(|shared| shared.to_vec(), Vec::from)
}

/// Selects the Log Module the restored backup assigns the System Log to.
///
/// Reads only the validated in-memory backup, so the acknowledgement is
/// delivered under the configuration being restored rather than any
/// configuration this Server retained before the Restore.
fn system_log_module(validated: &ValidatedBackup) -> Result<LogModuleIdentifier, RestoreError> {
    let backup = validated.backup();
    let configuration = assigned_configuration(
        backup.log_module_configurations(),
        backup.log_assignments(),
        LogType::System,
    )
    .ok_or(RestoreError::BackupIncompatible)?;

    LogModuleIdentifier::new(configuration.module.as_str())
        .map_err(|_| RestoreError::BackupIncompatible)
}

/// Returns the enabled configuration assigned to one log type.
pub(crate) fn assigned_configuration<'backup>(
    configurations: &'backup [LogModuleConfiguration],
    assignments: &[LogAssignment],
    log_type: LogType,
) -> Option<&'backup LogModuleConfiguration> {
    let assignment = assignments
        .iter()
        .find(|assignment| assignment.log_type == log_type)?;
    configurations.iter().find(|configuration| {
        configuration.identifier == assignment.configuration && configuration.enabled
    })
}

/// Maps a lifecycle workflow failure to a stable Restore failure category.
fn map_workflow_error(error: WorkflowError) -> RestoreError {
    match error {
        WorkflowError::Lifecycle(error) => RestoreError::Lifecycle(error),
        WorkflowError::AlreadyPending => RestoreError::RestorePending,
        _ => RestoreError::Lifecycle(LifecycleError::InvalidState),
    }
}

/// Fills a fixed-size buffer from operating-system randomness.
fn random_bytes<const BYTES: usize>() -> Result<[u8; BYTES], RestoreError> {
    crate::authentication::random_bytes().ok_or(RestoreError::RestoreFailed)
}

/// Generates a correlation identifier that carries no request content.
fn correlation_identifier() -> Result<String, RestoreError> {
    crate::authentication::correlation_identifier().ok_or(RestoreError::RestoreFailed)
}

/// Reads the current UTC event time in Unix milliseconds.
fn event_time_milliseconds() -> Result<i64, RestoreError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| i64::try_from(elapsed.as_millis()).ok())
        .ok_or(RestoreError::RestoreFailed)
}

#[cfg(test)]
mod tests {
    use std::{fmt, sync::Barrier, thread, time::Duration, time::Instant as Monotonic};

    use axum::{Router, body::Body, extract::Request};
    use weavelit_module_client::RESTORE_TICKET_HEADER_NAME;

    use super::{
        Arc, ClaimedRestore, Deadline, ExpectedOrigin, MAX_CONCURRENT_RESTORE_OPERATIONS,
        MAX_ENCRYPTED_ARTIFACT_BYTES, Method, PendingRestore, PendingRestoreSlot,
        RESTORE_ARTIFACT_PROFILE, RESTORE_ARTIFACT_ROUTE, RequestBudget, RestoreArtifactCheck,
        RestoreError, RestoreTicket, TOTAL_REQUEST_DEADLINE, UPLOAD_DEADLINE, Zeroizing,
    };
    use crate::{
        RateLimiter, capped_deadline,
        transport::{
            Classified, MountedSurface, PreBodyRejection, TransportBudget, TransportCapability,
            TransportRegistration,
        },
    };

    /// The listener authority every Restore request in this module targets.
    const LISTENER: &str = "127.0.0.1:8443";

    /// The source address a driven request is admitted from.
    const SOURCE: &str = "127.0.0.1";

    /// A recovery key stand-in. No assertion below may ever find it rendered.
    const RECOVERY_KEY: &str = "AGE-SECRET-KEY-1TESTONLYRECOVERYKEYMATERIAL";

    /// The correlation identifier a retained submission carries.
    const CORRELATION: &str = "0123456789abcdef0123456789abcdef";

    fn expected_origin() -> ExpectedOrigin {
        ExpectedOrigin::from_listener(LISTENER.parse().expect("the listener authority parses"))
    }

    /// Mints a distinct well-formed ticket for each seed.
    fn seeded_ticket(seed: u8) -> RestoreTicket {
        let mut entropy = [0_u8; 32];
        for (index, byte) in entropy.iter_mut().enumerate() {
            *byte = seed
                .wrapping_mul(37)
                .wrapping_add(u8::try_from(index).unwrap_or(u8::MAX));
        }
        RestoreTicket::from_zeroizing_entropy(Zeroizing::new(entropy))
    }

    /// Builds a retained submission holding [`RECOVERY_KEY`].
    fn retained(
        ticket: &RestoreTicket,
        budget: RequestBudget,
        expires_at: Deadline,
    ) -> PendingRestore {
        PendingRestore {
            digest: ticket.digest(),
            reconciliation_digest: weavelit_server_database::ReconciliationDigest::from_bytes(
                [1; 32],
            ),
            recovery_key: Zeroizing::new(RECOVERY_KEY.to_owned()),
            correlation_identifier: CORRELATION.to_owned(),
            budget,
            expires_at,
        }
    }

    /// An expiry the slot has not reached yet.
    fn live() -> Deadline {
        Deadline::now() + UPLOAD_DEADLINE
    }

    /// An expiry every later read has already passed.
    ///
    /// The slot compares with `<=` and the monotonic clock never runs
    /// backwards, so this instant is expired at every subsequent observation
    /// without any test waiting on wall-clock time.
    fn already_expired() -> Deadline {
        Deadline::now()
    }

    fn retains_a_submission(slot: &PendingRestoreSlot) -> bool {
        slot.held().is_some()
    }

    /// Returns the rejection a claim produced.
    ///
    /// Written without `unwrap_err` because the claimed submission implements
    /// no `Debug` that could render the recovery key it holds.
    fn claim_rejection(slot: &PendingRestoreSlot, ticket: &str) -> PreBodyRejection {
        slot.claim(ticket)
            .err()
            .expect("the claim must have been rejected")
    }

    // -----------------------------------------------------------------------
    // One-time ticket slot
    // -----------------------------------------------------------------------

    #[test]
    fn a_ticket_is_claimable_exactly_once() {
        let slot = PendingRestoreSlot::default();
        let ticket = seeded_ticket(1);
        slot.issue(retained(&ticket, RequestBudget::start(), live()))
            .expect("the first submission is retained");

        let claimed = slot
            .claim(ticket.as_str())
            .expect("the issuing ticket claims its own submission");
        assert_eq!(claimed.recovery_key.as_str(), RECOVERY_KEY);
        assert_eq!(claimed.correlation_identifier, CORRELATION);

        assert_eq!(
            claim_rejection(&slot, ticket.as_str()),
            PreBodyRejection::RestoreTicketInvalid
        );
    }

    #[test]
    fn a_replayed_ticket_finds_the_recovery_key_already_destroyed() {
        let slot = PendingRestoreSlot::default();
        let ticket = seeded_ticket(2);
        slot.issue(retained(&ticket, RequestBudget::start(), live()))
            .expect("the first submission is retained");

        let claimed = slot.claim(ticket.as_str()).expect("the first claim wins");
        // The successful claim moved the retained submission out of the slot,
        // so the replay below is refused by an empty slot rather than by a
        // comparison against a recovery key this Server still holds.
        assert!(!retains_a_submission(&slot));
        drop(claimed);

        assert_eq!(
            claim_rejection(&slot, ticket.as_str()),
            PreBodyRejection::RestoreTicketInvalid
        );
        assert!(!retains_a_submission(&slot));
    }

    #[test]
    fn a_wrong_ticket_destroys_the_outstanding_submission() {
        let slot = PendingRestoreSlot::default();
        let issued = seeded_ticket(3);
        let wrong = seeded_ticket(4);
        slot.issue(retained(&issued, RequestBudget::start(), live()))
            .expect("the submission is retained");

        assert_eq!(
            claim_rejection(&slot, wrong.as_str()),
            PreBodyRejection::RestoreTicketInvalid
        );
        // A failed claim is deliberately not retryable: the entry is taken
        // before the ticket is compared, so the recovery key is gone and the
        // ticket this Server actually issued no longer opens anything.
        assert!(!retains_a_submission(&slot));
        assert_eq!(
            claim_rejection(&slot, issued.as_str()),
            PreBodyRejection::RestoreTicketInvalid
        );
    }

    #[test]
    fn two_concurrent_claims_admit_exactly_one() {
        // The lane admits one Restore at a time, and the slot must reach the
        // same outcome even when two claims race outside that lane.
        assert_eq!(MAX_CONCURRENT_RESTORE_OPERATIONS, 1);

        for attempt in 0..64_u8 {
            let slot = Arc::new(PendingRestoreSlot::default());
            let ticket = seeded_ticket(attempt);
            slot.issue(retained(&ticket, RequestBudget::start(), live()))
                .expect("the submission is retained");

            let start = Arc::new(Barrier::new(2));
            let outcomes = thread::scope(|scope| {
                let claimants: Vec<_> = (0..2)
                    .map(|_| {
                        let slot = Arc::clone(&slot);
                        let start = Arc::clone(&start);
                        let submitted = ticket.as_str().to_owned();
                        scope.spawn(move || {
                            start.wait();
                            slot.claim(&submitted)
                                .map(|claimed| claimed.recovery_key.as_str() == RECOVERY_KEY)
                        })
                    })
                    .collect();
                claimants
                    .into_iter()
                    .map(|claimant| claimant.join().expect("a claiming thread must not panic"))
                    .collect::<Vec<_>>()
            });

            let claimed: Vec<_> = outcomes
                .iter()
                .filter_map(|outcome| outcome.as_ref().ok())
                .collect();
            assert_eq!(claimed.len(), 1, "attempt {attempt}");
            assert_eq!(claimed[0], &true, "attempt {attempt}");
            let rejected: Vec<_> = outcomes
                .iter()
                .filter_map(|outcome| outcome.as_ref().err())
                .collect();
            assert_eq!(
                rejected,
                vec![&PreBodyRejection::RestoreTicketInvalid],
                "attempt {attempt}"
            );
            assert!(!retains_a_submission(&slot), "attempt {attempt}");
        }
    }

    #[test]
    fn an_expired_submission_is_destroyed_and_a_new_one_is_then_permitted() {
        let slot = PendingRestoreSlot::default();
        let abandoned = seeded_ticket(5);
        slot.issue(retained(
            &abandoned,
            RequestBudget::start(),
            already_expired(),
        ))
        .expect("the abandoned submission is retained");

        // A digest that is not the retained one destroys nothing.
        slot.expire(&seeded_ticket(6).digest());
        assert!(retains_a_submission(&slot));

        // Its own scheduled expiry destroys it without waiting for a later
        // request to notice, so no recovery key stays resident.
        slot.expire(&abandoned.digest());
        assert!(!retains_a_submission(&slot));

        let replacement = seeded_ticket(7);
        slot.issue(retained(&replacement, RequestBudget::start(), live()))
            .expect("a new submission is permitted once the slot is free");
        assert!(slot.claim(replacement.as_str()).is_ok());
    }

    #[test]
    fn a_live_submission_is_never_destroyed_by_an_expiry_sweep() {
        let slot = PendingRestoreSlot::default();
        let ticket = seeded_ticket(8);
        slot.issue(retained(&ticket, RequestBudget::start(), live()))
            .expect("the submission is retained");

        slot.expire(&ticket.digest());
        assert!(
            slot.claim(ticket.as_str()).is_ok(),
            "an unexpired submission survives its own digest's sweep"
        );
    }

    #[test]
    fn issuing_while_a_live_submission_is_outstanding_is_rejected() {
        let slot = PendingRestoreSlot::default();
        let outstanding = seeded_ticket(9);
        slot.issue(retained(&outstanding, RequestBudget::start(), live()))
            .expect("the first submission is retained");

        let intruder = seeded_ticket(10);
        assert_eq!(
            slot.issue(retained(&intruder, RequestBudget::start(), live()))
                .unwrap_err(),
            RestoreError::RestorePending
        );

        // The rejected issue neither replaced nor destroyed the outstanding
        // submission, so the first ticket still claims its own recovery key.
        let claimed = slot
            .claim(outstanding.as_str())
            .expect("the outstanding submission survives a rejected issue");
        assert_eq!(claimed.recovery_key.as_str(), RECOVERY_KEY);
    }

    // -----------------------------------------------------------------------
    // Rendering
    // -----------------------------------------------------------------------

    /// Renders a value only through the traits it actually implements.
    ///
    /// The inherent methods below apply only when their bound holds, so a type
    /// that implements neither trait falls back to [`UnrenderableValue`] and
    /// reports nothing. Deriving `Debug` on a type asserted here turns its
    /// result from `None` into `Some`, which fails the assertion.
    struct Rendering<'value, T>(&'value T);

    trait UnrenderableValue {
        fn debug_text(&self) -> Option<String> {
            None
        }

        fn display_text(&self) -> Option<String> {
            None
        }
    }

    impl<T> UnrenderableValue for Rendering<'_, T> {}

    impl<T: fmt::Debug> Rendering<'_, T> {
        fn debug_text(&self) -> Option<String> {
            Some(format!("{:?}", self.0))
        }
    }

    impl<T: fmt::Display> Rendering<'_, T> {
        fn display_text(&self) -> Option<String> {
            Some(self.0.to_string())
        }
    }

    #[test]
    fn no_retained_restore_value_renders_a_ticket_or_a_recovery_key() {
        let ticket = seeded_ticket(11);
        let digest = ticket.digest();
        let pending = retained(&ticket, RequestBudget::start(), live());
        let claimed = ClaimedRestore::new(retained(&ticket, RequestBudget::start(), live()));

        // The two values that do render are redacted, and neither reproduces
        // the ticket they were derived from.
        let rendered = [
            Rendering(&ticket).debug_text(),
            Rendering(&digest).debug_text(),
        ];
        assert_eq!(
            rendered,
            [
                Some("RestoreTicket(redacted)".to_owned()),
                Some("RestoreTicketDigest(redacted)".to_owned()),
            ]
        );
        for text in rendered.iter().flatten() {
            assert!(!text.contains(ticket.as_str()), "{text}");
            assert!(!text.contains(RECOVERY_KEY), "{text}");
        }
        assert_eq!(Rendering(&ticket).display_text(), None);
        assert_eq!(Rendering(&digest).display_text(), None);

        // The two values that hold the recovery key render at all through
        // neither trait, so no format string can reach the key they own.
        assert_eq!(Rendering(&pending).debug_text(), None);
        assert_eq!(Rendering(&pending).display_text(), None);
        assert_eq!(Rendering(&claimed).debug_text(), None);
        assert_eq!(Rendering(&claimed).display_text(), None);

        // Control: the probe does report a rendering whenever the trait is
        // implemented, so every `None` above is a real absence rather than a
        // helper that never reports anything.
        let control = RECOVERY_KEY.to_owned();
        assert_eq!(
            Rendering(&control).debug_text(),
            Some(format!("{RECOVERY_KEY:?}"))
        );
        assert_eq!(
            Rendering(&control).display_text(),
            Some(RECOVERY_KEY.to_owned())
        );
    }

    // -----------------------------------------------------------------------
    // Inherited request budget at the artifact route's admission boundary
    // -----------------------------------------------------------------------

    /// Mounts the artifact route's registered pre-body check on its own
    /// surface, so the chain below runs exactly the validation the listener
    /// runs before it allocates an artifact.
    fn artifact_surface(pending: &Arc<PendingRestoreSlot>) -> MountedSurface {
        let registration = TransportRegistration::new(
            Method::PUT,
            RESTORE_ARTIFACT_ROUTE,
            RESTORE_ARTIFACT_PROFILE,
        )
        .with_pre_body_check(Arc::new(RestoreArtifactCheck {
            expected_origin: expected_origin(),
            pending: Arc::clone(pending),
        }));
        MountedSurface::without_registrations(Router::new())
            .with_capability(TransportCapability::new(registration, |router| router))
    }

    /// Builds an otherwise valid artifact upload head.
    fn artifact_request(ticket: &str, declared_bytes: usize) -> Request {
        Request::builder()
            .method(Method::PUT)
            .uri(RESTORE_ARTIFACT_ROUTE)
            .header("host", LISTENER)
            .header("origin", format!("https://{LISTENER}"))
            .header("x-weavelit-csrf", "1")
            .header("content-type", "application/octet-stream")
            .header(RESTORE_TICKET_HEADER_NAME, ticket)
            .header("content-length", declared_bytes.to_string())
            .body(Body::empty())
            .expect("the artifact upload head is well formed")
    }

    /// Runs the listener's admission chain up to route classification.
    fn classify(surface: &MountedSurface, request: Request) -> Classified {
        crate::transport::HeadRead::new(request)
            .admit_rate(
                &RateLimiter::new(),
                SOURCE.parse().expect("the source address parses"),
                Monotonic::now(),
            )
            .map_err(|_| "a fresh rate limiter admits the first request")
            .expect("a fresh rate limiter admits the first request")
            .classify(surface.registry())
    }

    #[test]
    fn an_artifact_upload_inherits_the_budget_the_key_submission_started() {
        /// Elapsed time the claim must still be carrying afterwards.
        const ELAPSED: Duration = Duration::from_millis(20);

        let slot = Arc::new(PendingRestoreSlot::default());
        let ticket = seeded_ticket(12);
        let submitted = RequestBudget::start();
        slot.issue(retained(&ticket, submitted, live()))
            .expect("the submission is retained");

        // Advances the monotonic clock past the exact quantity the assertion
        // reads, so the elapsed time is an established precondition rather
        // than a wall-clock wait that could observe less than it needs.
        while submitted.elapsed() < ELAPSED {
            std::hint::spin_loop();
        }

        let surface = artifact_surface(&slot);
        let validated = classify(&surface, artifact_request(ticket.as_str(), 1024))
            .check_framing()
            .expect("the declared artifact is inside the approved bound")
            .validate()
            .expect("the issuing ticket is admitted");

        let remaining = validated
            .remaining_budget()
            .expect("a claimed upload carries the budget it inherited");
        // A budget restarted by the claim would still have the whole total
        // deadline left; an inherited one has already spent `ELAPSED`.
        assert!(
            remaining <= TOTAL_REQUEST_DEADLINE - ELAPSED,
            "the claim must inherit the elapsed time: {remaining:?}"
        );
        assert!(remaining > Duration::ZERO);
    }

    #[test]
    fn an_artifact_upload_is_bounded_by_the_smaller_of_its_two_deadlines() {
        let slot = Arc::new(PendingRestoreSlot::default());
        let ticket = seeded_ticket(13);
        slot.issue(retained(&ticket, RequestBudget::start(), live()))
            .expect("the submission is retained");

        let surface = artifact_surface(&slot);
        let classified = classify(&surface, artifact_request(ticket.as_str(), 1024));
        let profile = classified.profile();
        assert_eq!(profile.max_body_bytes(), MAX_ENCRYPTED_ARTIFACT_BYTES);
        assert_eq!(
            profile.budget(),
            TransportBudget::Admitted {
                body_read: UPLOAD_DEADLINE,
                processing: TOTAL_REQUEST_DEADLINE,
            }
        );

        let inherited = classified
            .check_framing()
            .expect("the declared artifact is inside the approved bound")
            .validate()
            .expect("the issuing ticket is admitted")
            .remaining_budget()
            .expect("a claimed upload carries the budget it inherited");

        let admitted_at = Deadline::now();
        // A head budget far longer than either Restore deadline, so only the
        // registered profile and the inherited remainder can bound the read.
        let head_deadline = admitted_at + TOTAL_REQUEST_DEADLINE * 2;
        let upload_deadline = profile.body_deadline(head_deadline, admitted_at);
        assert_eq!(upload_deadline, admitted_at + UPLOAD_DEADLINE);

        // The listener caps the read at the earlier of the two. The inherited
        // remainder of a freshly started total budget is the larger, so the
        // upload deadline bounds this request.
        assert!(inherited > UPLOAD_DEADLINE);
        assert_eq!(
            capped_deadline(upload_deadline, Some(admitted_at + inherited)),
            upload_deadline
        );
        // A remainder smaller than the upload deadline bounds it instead, so
        // the upload can never outlive the budget the key submission started.
        let nearly_spent = admitted_at + Duration::from_secs(1);
        assert_eq!(
            capped_deadline(upload_deadline, Some(nearly_spent)),
            nearly_spent
        );
    }

    #[test]
    fn an_oversized_declared_artifact_is_rejected_before_its_ticket_is_claimed() {
        let slot = Arc::new(PendingRestoreSlot::default());
        let ticket = seeded_ticket(14);
        slot.issue(retained(&ticket, RequestBudget::start(), live()))
            .expect("the submission is retained");

        let surface = artifact_surface(&slot);
        let oversized = classify(
            &surface,
            artifact_request(ticket.as_str(), MAX_ENCRYPTED_ARTIFACT_BYTES + 1),
        );
        assert!(
            oversized.check_framing().is_err(),
            "one byte over the approved bound is refused"
        );
        // Framing precedes the pre-body check, which precedes the only body
        // allocation, so the rejection cost this Server neither the artifact's
        // memory nor its outstanding submission.
        assert!(retains_a_submission(&slot));

        // The approved bound itself still frames, so the rejection above is
        // the bound and not a blanket refusal.
        let at_bound = classify(
            &surface,
            artifact_request(ticket.as_str(), MAX_ENCRYPTED_ARTIFACT_BYTES),
        );
        assert!(at_bound.check_framing().is_ok());
        assert!(retains_a_submission(&slot));
    }
}
