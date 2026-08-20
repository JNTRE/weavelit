//! Behavioral evidence for the Server-owned new-state Init workflow.
//!
//! The assertions here read through accessors rather than through rendered
//! output. `Name`, `StateIdentifier`, and every secret wrapper redact or elide
//! in `Debug`, so searching a rendering for a value would pass whether or not
//! the value was ever present. Where a rendering is asserted to exclude a
//! needle, the needle is independently proven present first.

use std::cell::RefCell;

use weavelit_server_authentication::{
    PasswordAuthenticator, PasswordPolicy, PasswordVerdict, RustCryptoArgon2, StoredCredential,
};
use weavelit_server_components::{AvailableComponents, LogSettingsFormat, MfaFactorFormat};
use weavelit_server_database::{
    CompletionObligation, ConfigurationKey, ConfigurationValue, CorrelationIdentifier,
    DeploymentIdentifier, GroupGrant, LogClassification, LogDetail, LogModuleSetting, LogType,
    MAX_CHECKPOINT_METADATA_LENGTH, Name, ProtectedValue, StateIdentifier, WorkflowKind,
};
use weavelit_server_init::{
    ADMINISTRATORS_GROUP_NAME, AuthorizedInit, CheckpointError, InitAuthority, InitCheckpoint,
    InitError, InitOperations, InitTarget, InitialAdministrator, InitialLogModuleConfiguration,
    InitialPassword, InitialProtectedSetting, InitialSecret, InitializeServer,
    MAX_LOG_MODULE_CONFIGURATIONS, MAX_LOG_MODULE_SETTINGS, MAX_PASSWORD_BYTES,
    PreparedInitDelivery, RequestError, validate_request,
};
use weavelit_server_lifecycle::{
    CheckpointMetadata, LifecycleError, ProtectedValueKind, ProtectedValueSealer, WorkflowError,
};
use weavelit_server_recovery_key::{DeliveryNonce, RecoveryProof};

const PASSWORD: &str = "correct horse battery staple";
const LOG_MODULE: &str = "sqlite";
const WEB_UI: &str = "web-ui";
const SYSTEM_CONFIGURATION: &str = "system-log";
const AUDIT_CONFIGURATION: &str = "audit-log";
const LOG_SETTING: &str = "destination-path";
const LOG_SECRET: &[u8] = b"log-module-connection-secret";

fn name(value: &str) -> Name {
    Name::new(value).expect("the fixture name must be accepted")
}

fn key(value: &str) -> ConfigurationKey {
    ConfigurationKey::new(value).expect("the fixture key must be accepted")
}

fn components() -> AvailableComponents {
    AvailableComponents {
        client_modules: [name(WEB_UI)].into_iter().collect(),
        mfa_modules: [(
            name("totp"),
            MfaFactorFormat {
                factor_data_bytes: 20,
            },
        )]
        .into_iter()
        .collect(),
        log_modules: [(
            name(LOG_MODULE),
            LogSettingsFormat {
                accepted_keys: [LOG_SETTING.to_owned()].into_iter().collect(),
            },
        )]
        .into_iter()
        .collect(),
        ..AvailableComponents::default()
    }
}

fn operations() -> InitOperations {
    InitOperations::new(components(), name(WEB_UI))
        .expect("the fixture inventory carries the Web UI")
}

fn obligation() -> CompletionObligation {
    CompletionObligation::new(
        StateIdentifier::from_bytes([9; 16]).expect("the fixture identifier is non-zero"),
        WorkflowKind::Init,
        LogClassification::new("lifecycle.init").expect("the fixture classification is accepted"),
        CorrelationIdentifier::new("fixture-correlation")
            .expect("the fixture correlation identifier is accepted"),
        1_700_000_000_000,
        LogDetail::new("initialization completed").expect("the fixture detail is accepted"),
    )
    .expect("the fixture obligation is well formed")
}

fn configuration(configuration_name: &str, enabled: bool) -> InitialLogModuleConfiguration {
    InitialLogModuleConfiguration {
        module: name(LOG_MODULE),
        name: name(configuration_name),
        enabled,
        settings: vec![LogModuleSetting {
            key: key(LOG_SETTING),
            value: ConfigurationValue::new("/var/lib/weavelit/log.sqlite")
                .expect("the fixture value is accepted"),
        }],
        protected_settings: Vec::new(),
    }
}

fn request(proof: Option<RecoveryProof>) -> InitializeServer {
    InitializeServer {
        administrator: InitialAdministrator {
            username: name("first-administrator"),
            display_name: Some(name("First Administrator")),
            password: InitialPassword::new(PASSWORD.to_owned())
                .expect("the fixture password is within bounds"),
        },
        log_module_configurations: vec![
            configuration(SYSTEM_CONFIGURATION, true),
            configuration(AUDIT_CONFIGURATION, true),
        ],
        system_log: name(SYSTEM_CONFIGURATION),
        audit_log: name(AUDIT_CONFIGURATION),
        recovery_key_proof: proof,
    }
}

/// An authority that permits Init and records how often it was consulted.
struct EligibleAuthority {
    consultations: RefCell<usize>,
}

impl EligibleAuthority {
    fn new() -> Self {
        Self {
            consultations: RefCell::new(0),
        }
    }
}

impl InitAuthority for EligibleAuthority {
    fn authorize(&self) -> Result<InitTarget, InitError> {
        *self.consultations.borrow_mut() += 1;
        Ok(InitTarget::new(
            DeploymentIdentifier::from_bytes([3; 16])
                .expect("the fixture deployment identifier is non-zero"),
        ))
    }
}

/// An authority standing in for a deployment record that is already sealed.
struct SealedAuthority;

impl InitAuthority for SealedAuthority {
    fn authorize(&self) -> Result<InitTarget, InitError> {
        Err(WorkflowError::AlreadyInitialized.into())
    }
}

/// A sealer that transforms plaintext and records every call it received.
struct RecordingSealer {
    calls: RefCell<Vec<(ProtectedValueKind, usize)>>,
}

impl RecordingSealer {
    fn new() -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<(ProtectedValueKind, usize)> {
        self.calls.borrow().clone()
    }
}

impl ProtectedValueSealer for RecordingSealer {
    fn seal(
        &self,
        kind: ProtectedValueKind,
        plaintext: &[u8],
    ) -> Result<ProtectedValue, LifecycleError> {
        self.calls.borrow_mut().push((kind, plaintext.len()));
        let mut sealed = b"sealed:".to_vec();
        sealed.extend(plaintext.iter().rev());
        ProtectedValue::new(sealed).map_err(|_| LifecycleError::IntegrityFailure)
    }
}

/// Recomputes the proof a client that retained the delivered key would submit.
///
/// The checkpoint's encoding is the only place the expected proof is readable
/// from outside the crate, which is exactly the property the one-time delivery
/// depends on.
fn valid_proof(prepared: &PreparedInitDelivery) -> RecoveryProof {
    let metadata = prepared
        .checkpoint()
        .encode()
        .expect("the checkpoint encodes");
    let proof: [u8; 32] = metadata.as_bytes()[33..65]
        .try_into()
        .expect("the fixed layout carries a 32-byte proof");
    RecoveryProof::from_bytes(proof)
}

#[test]
fn a_finalized_deployment_has_exactly_one_active_administrator_without_an_mfa_factor() {
    let prepared = PreparedInitDelivery::prepare().expect("preparation succeeds");
    let request = request(Some(valid_proof(&prepared)));
    let state = operations()
        .finalize(
            &EligibleAuthority::new(),
            prepared.checkpoint(),
            &request,
            &RecordingSealer::new(),
            obligation(),
        )
        .expect("a valid request initializes the deployment");

    assert_eq!(state.accounts().len(), 1);
    let account = &state.accounts()[0];
    assert_eq!(account.username, name("first-administrator"));
    assert_eq!(account.display_name, Some(name("First Administrator")));
    assert!(account.active);
    assert!(
        !account.mfa_required,
        "Init creates the first user without an enrolled factor, so requiring one would lock the deployment out"
    );
    assert_eq!(
        account.credential_revision,
        weavelit_server_database::CredentialRevision::INITIAL
    );
    assert!(!account.must_change_password);
    assert_eq!(account.temporary_credential_expiration, None);
    assert!(
        state.mfa_factors().is_empty(),
        "Init must enroll no MFA factor"
    );
    assert_eq!(state.password_verifiers().len(), 1);
    assert_eq!(state.password_verifiers()[0].account, account.identifier);
    assert!(state.service_connections().is_empty());

    assert_eq!(state.account_public_identities().len(), 1);
    assert_eq!(
        state.account_public_identities()[0].account(),
        account.identifier
    );
    assert_eq!(state.account_audit_references().len(), 1);
    assert_eq!(state.group_audit_references().len(), 1);
    assert_eq!(
        state.log_configuration_audit_references().len(),
        state.log_module_configurations().len()
    );
    let account_reference = state.account_audit_references()[0];
    let group_reference = state.group_audit_references()[0];
    assert_eq!(account_reference.account(), account.identifier);
    assert_eq!(group_reference.group(), state.groups()[0].identifier);
    assert!(
        state
            .log_module_configurations()
            .iter()
            .all(|configuration| {
                state
                    .log_configuration_audit_references()
                    .iter()
                    .any(|reference| reference.configuration() == configuration.identifier)
            })
    );

    let mut audit_references = vec![
        account_reference.audit_reference(),
        group_reference.audit_reference(),
    ];
    audit_references.extend(
        state
            .log_configuration_audit_references()
            .iter()
            .map(|reference| reference.audit_reference()),
    );
    let expected_reference_count = audit_references.len();
    audit_references.sort_unstable();
    audit_references.dedup();
    assert_eq!(audit_references.len(), expected_reference_count);

    for reference in audit_references
        .into_iter()
        .map(|reference| reference.to_string())
    {
        assert_eq!(reference.len(), 35);
        assert!(reference.starts_with("ar-"));
        assert!(!reference.contains(account.username.as_str()));
        assert!(!reference.contains(state.groups()[0].name.as_str()));
        assert!(
            state
                .log_module_configurations()
                .iter()
                .all(|configuration| !reference.contains(configuration.name.as_str()))
        );
    }
}

#[test]
fn the_system_defined_group_grants_only_web_ui_access_and_server_administration() {
    let prepared = PreparedInitDelivery::prepare().expect("preparation succeeds");
    let request = request(Some(valid_proof(&prepared)));
    let state = operations()
        .finalize(
            &EligibleAuthority::new(),
            prepared.checkpoint(),
            &request,
            &RecordingSealer::new(),
            obligation(),
        )
        .expect("a valid request initializes the deployment");

    assert_eq!(state.groups().len(), 1);
    let group = &state.groups()[0];
    assert_eq!(group.name, name(ADMINISTRATORS_GROUP_NAME));
    assert_eq!(group.description, None);

    assert_eq!(state.group_memberships().len(), 1);
    assert_eq!(state.group_memberships()[0].group, group.identifier);
    assert_eq!(
        state.group_memberships()[0].account,
        state.accounts()[0].identifier
    );

    let grants: Vec<&GroupGrant> = state
        .group_grants()
        .iter()
        .map(|record| &record.grant)
        .collect();
    assert_eq!(grants.len(), 2, "the Group carries exactly two grants");
    assert!(grants.contains(&&GroupGrant::ClientModule(name(WEB_UI))));
    assert!(grants.contains(&&GroupGrant::ServerAdministration));
    assert!(
        !grants
            .iter()
            .any(|grant| matches!(grant, GroupGrant::Operation(_))),
        "the Administrators Group grants no named Operation"
    );
    assert!(
        !grants
            .iter()
            .any(|grant| matches!(grant, GroupGrant::ServiceModule(_))),
        "the Administrators Group grants no Service Module"
    );
    assert!(
        state
            .group_grants()
            .iter()
            .all(|record| record.group == group.identifier),
        "no grant belongs to a Group a request defined"
    );
}

#[test]
fn the_stored_verifier_authenticates_the_submitted_password_at_the_current_profile() {
    let prepared = PreparedInitDelivery::prepare().expect("preparation succeeds");
    let request = request(Some(valid_proof(&prepared)));
    let state = operations()
        .finalize(
            &EligibleAuthority::new(),
            prepared.checkpoint(),
            &request,
            &RecordingSealer::new(),
            obligation(),
        )
        .expect("a valid request initializes the deployment");

    let policy = PasswordPolicy::approved();
    let authenticator = PasswordAuthenticator::new(RustCryptoArgon2::new(policy), policy)
        .expect("the approved policy builds an authenticator");
    let stored = state.password_verifiers()[0].verifier.as_str().to_owned();

    assert!(
        matches!(
            authenticator.authenticate(StoredCredential::Verifier(&stored), PASSWORD.as_bytes()),
            Ok(PasswordVerdict::Verified { replacement: None })
        ),
        "a verifier created at the current profile authenticates and needs no replacement"
    );
    assert!(matches!(
        authenticator.authenticate(StoredCredential::Verifier(&stored), b"wrong horse"),
        Ok(PasswordVerdict::Denied)
    ));
    assert!(
        !stored.contains(PASSWORD),
        "the stored verifier carries no submitted password"
    );
}

#[test]
fn the_initial_state_records_the_prepared_recipient_and_explicit_log_assignments() {
    let prepared = PreparedInitDelivery::prepare().expect("preparation succeeds");
    let expected_recipient = prepared
        .checkpoint()
        .recovery_public_key()
        .as_str()
        .to_owned();
    let request = request(Some(valid_proof(&prepared)));
    let state = operations()
        .finalize(
            &EligibleAuthority::new(),
            prepared.checkpoint(),
            &request,
            &RecordingSealer::new(),
            obligation(),
        )
        .expect("a valid request initializes the deployment");

    assert!(expected_recipient.starts_with("age1"));
    assert_eq!(state.recovery_public_key().as_str(), expected_recipient);

    assert_eq!(state.log_module_configurations().len(), 2);
    assert_eq!(state.log_assignments().len(), 2);
    for log_type in LogType::ALL {
        let assignment = state
            .log_assignments()
            .iter()
            .find(|assignment| assignment.log_type == log_type)
            .expect("each log type is explicitly assigned");
        let configuration = state
            .log_module_configurations()
            .iter()
            .find(|configuration| configuration.identifier == assignment.configuration)
            .expect("an assignment resolves to a submitted configuration");
        assert!(configuration.enabled);
        assert_eq!(configuration.module, name(LOG_MODULE));
    }

    let system = state
        .log_assignments()
        .iter()
        .find(|assignment| assignment.log_type == LogType::System)
        .expect("the System Log is assigned");
    let audit = state
        .log_assignments()
        .iter()
        .find(|assignment| assignment.log_type == LogType::Audit)
        .expect("the Audit Log is assigned");
    assert_ne!(
        system.configuration, audit.configuration,
        "the two log types are independently assigned"
    );

    assert_eq!(state.completion_obligation().workflow(), WorkflowKind::Init);
    assert_eq!(state.configuration().len(), 1);
    assert_eq!(state.configuration()[0].component.as_str(), "totp");
    assert_eq!(state.configuration()[0].key.as_str(), "mfa-module.enabled");
    assert_eq!(state.configuration()[0].value.as_str(), "false");
}

#[test]
fn protected_log_module_settings_are_sealed_before_they_are_stored() {
    let prepared = PreparedInitDelivery::prepare().expect("preparation succeeds");
    let mut request = request(Some(valid_proof(&prepared)));
    request.log_module_configurations[0]
        .protected_settings
        .push(InitialProtectedSetting {
            key: key("connection-secret"),
            value: InitialSecret::new(LOG_SECRET.to_vec()).expect("the fixture secret is bounded"),
        });

    let sealer = RecordingSealer::new();
    let state = operations()
        .finalize(
            &EligibleAuthority::new(),
            prepared.checkpoint(),
            &request,
            &sealer,
            obligation(),
        )
        .expect("a valid request initializes the deployment");

    assert_eq!(
        sealer.calls(),
        vec![(ProtectedValueKind::ComponentSecret, LOG_SECRET.len())],
        "each submitted secret is sealed exactly once as a component secret"
    );
    assert_eq!(state.protected_secrets().len(), 1);
    let stored = &state.protected_secrets()[0];
    assert_eq!(stored.component, name(LOG_MODULE));
    assert_eq!(stored.key, key("connection-secret"));
    assert_ne!(
        stored.value.as_bytes(),
        LOG_SECRET,
        "a submitted secret is never stored as submitted"
    );
    assert!(stored.value.as_bytes().starts_with(b"sealed:"));
}

#[test]
fn finalization_without_a_proof_reports_that_confirmation_is_required() {
    let prepared = PreparedInitDelivery::prepare().expect("preparation succeeds");
    let error = operations()
        .finalize(
            &EligibleAuthority::new(),
            prepared.checkpoint(),
            &request(None),
            &RecordingSealer::new(),
            obligation(),
        )
        .expect_err("finalization without a proof cannot succeed");

    assert_eq!(error, InitError::RecoveryKeyConfirmationRequired);
    assert_eq!(
        error.category_reason(),
        (
            "recovery_key_confirmation_required",
            "recovery_key_confirmation_required"
        )
    );
}

#[test]
fn finalization_with_a_wrong_proof_reports_that_confirmation_is_invalid() {
    let prepared = PreparedInitDelivery::prepare().expect("preparation succeeds");
    let correct = valid_proof(&prepared);
    let mut wrong = *correct.as_bytes();
    wrong[0] ^= 0x01;

    let sealer = RecordingSealer::new();
    let error = operations()
        .finalize(
            &EligibleAuthority::new(),
            prepared.checkpoint(),
            &request(Some(RecoveryProof::from_bytes(wrong))),
            &sealer,
            obligation(),
        )
        .expect_err("a wrong proof cannot finalize Init");

    assert_eq!(error, InitError::RecoveryKeyConfirmationInvalid);
    assert!(
        sealer.calls().is_empty(),
        "a rejected proof causes no protection side effect"
    );
    // The correct proof is proven usable, so the rejection above is a real
    // mismatch rather than a checkpoint that could never confirm anything.
    assert!(prepared.checkpoint().confirm(Some(&correct)).is_ok());
}

#[test]
fn checkpoint_metadata_round_trips_through_its_versioned_encoding() {
    let prepared = PreparedInitDelivery::prepare().expect("preparation succeeds");
    let proof = valid_proof(&prepared);
    let encoded = prepared
        .checkpoint()
        .encode()
        .expect("the checkpoint encodes");
    assert!(encoded.as_bytes().len() <= MAX_CHECKPOINT_METADATA_LENGTH);

    let decoded = InitCheckpoint::decode(&encoded).expect("this Server decodes what it wrote");
    assert_eq!(
        decoded.recovery_public_key().as_str(),
        prepared.checkpoint().recovery_public_key().as_str()
    );
    assert_eq!(
        decoded.delivery_nonce().as_bytes(),
        prepared.checkpoint().delivery_nonce().as_bytes()
    );
    assert!(
        decoded.confirm(Some(&proof)).is_ok(),
        "a decoded checkpoint confirms the same proof the prepared one does"
    );
    assert_eq!(
        decoded.confirm(None),
        Err(InitError::RecoveryKeyConfirmationRequired)
    );
    assert_eq!(format!("{decoded:?}"), "InitCheckpoint(REDACTED)");
}

#[test]
fn malformed_checkpoint_metadata_is_rejected_before_any_proof_comparison() {
    let prepared = PreparedInitDelivery::prepare().expect("preparation succeeds");
    let valid = prepared
        .checkpoint()
        .encode()
        .expect("the checkpoint encodes")
        .as_bytes()
        .to_vec();
    // The unmodified encoding decodes, so each rejection below is caused by the
    // mutation and not by a layout that never decoded at all.
    assert!(
        InitCheckpoint::decode(
            &CheckpointMetadata::from_bytes(valid.clone()).expect("the encoding is bounded")
        )
        .is_ok()
    );

    let mut wrong_version = valid.clone();
    wrong_version[0] = 2;
    assert_eq!(
        decode_error(&wrong_version),
        CheckpointError::UnsupportedFormatVersion
    );

    assert_eq!(decode_error(&valid[..40]), CheckpointError::Malformed);
    assert_eq!(decode_error(&[1_u8]), CheckpointError::Malformed);

    let mut wrong_length = valid.clone();
    wrong_length[65] = wrong_length[65].wrapping_add(1);
    assert_eq!(decode_error(&wrong_length), CheckpointError::Malformed);

    let mut wrong_recipient = valid;
    wrong_recipient[66] = b'Z';
    assert_eq!(
        decode_error(&wrong_recipient),
        CheckpointError::RecipientInvalid
    );
}

fn decode_error(bytes: &[u8]) -> CheckpointError {
    let metadata =
        CheckpointMetadata::from_bytes(bytes.to_vec()).expect("the fixture metadata is bounded");
    InitCheckpoint::decode(&metadata).expect_err("malformed metadata must be rejected")
}

#[test]
fn every_recovery_key_preparation_produces_a_distinct_recipient_nonce_and_proof() {
    let first = PreparedInitDelivery::prepare().expect("preparation succeeds");
    let second = PreparedInitDelivery::prepare().expect("preparation succeeds");

    assert_ne!(
        first.checkpoint().recovery_public_key().as_str(),
        second.checkpoint().recovery_public_key().as_str()
    );
    assert_ne!(
        first.checkpoint().delivery_nonce().as_bytes(),
        second.checkpoint().delivery_nonce().as_bytes()
    );
    assert_ne!(
        valid_proof(&first).as_bytes(),
        valid_proof(&second).as_bytes()
    );
    assert!(
        first
            .checkpoint()
            .confirm(Some(&valid_proof(&second)))
            .is_err(),
        "one deployment's proof cannot confirm another's checkpoint"
    );
}

#[test]
fn the_delivered_private_key_leaves_the_preparation_exactly_once() {
    let prepared = PreparedInitDelivery::prepare().expect("preparation succeeds");
    let recipient = prepared
        .checkpoint()
        .recovery_public_key()
        .as_str()
        .to_owned();
    let rendered = format!("{prepared:?}");

    // The delivery line is produced and proven non-empty, so the redaction
    // assertions below are about a value that genuinely exists.
    let line = prepared
        .into_delivery_line()
        .expect("the canonical delivery line is produced");
    assert!(line.starts_with("AGE-SECRET-KEY-1"));
    assert!(line.len() > "AGE-SECRET-KEY-1".len());
    assert_eq!(rendered, "PreparedInitDelivery(REDACTED)");
    assert!(!rendered.contains(line.as_str()));
    assert!(
        !recipient.contains(line.as_str()),
        "the retained recipient carries no private key material"
    );
}

#[test]
fn a_sealed_deployment_refuses_every_mutating_entry_point_invoked_directly() {
    let sealer = RecordingSealer::new();
    let prepared = PreparedInitDelivery::prepare().expect("preparation succeeds");
    let operations = operations();

    // Both entry points are called directly, as a routing or composition defect
    // would call them, rather than through any request path.
    assert_eq!(
        operations
            .prepare_delivery(&SealedAuthority)
            .map(|_| ())
            .expect_err("a sealed deployment prepares no recovery key"),
        InitError::AlreadyInitialized
    );
    assert_eq!(
        operations
            .finalize(
                &SealedAuthority,
                prepared.checkpoint(),
                &request(Some(valid_proof(&prepared))),
                &sealer,
                obligation(),
            )
            .map(|_| ())
            .expect_err("a sealed deployment finalizes nothing"),
        InitError::AlreadyInitialized
    );
    assert!(
        sealer.calls().is_empty(),
        "a sealed deployment causes no protection side effect"
    );
    assert_eq!(
        InitError::AlreadyInitialized.category_reason(),
        ("deployment_state_invalid", "already_initialized")
    );
}

#[test]
fn a_sealed_deployment_is_reported_before_the_submitted_request_is_examined() {
    let prepared = PreparedInitDelivery::prepare().expect("preparation succeeds");
    let sealer = RecordingSealer::new();

    // Each request below would produce its own distinct error if the request
    // were read first, so answering `AlreadyInitialized` for all of them is
    // evidence that the authority is consulted before anything is examined.
    let mut unknown_module = request(None);
    unknown_module.log_module_configurations[0].module = name("not-compiled-in");
    let mut wrong_proof_bytes = *valid_proof(&prepared).as_bytes();
    wrong_proof_bytes[0] ^= 0x01;
    let mut wrong_proof = request(Some(RecoveryProof::from_bytes(wrong_proof_bytes)));
    wrong_proof.system_log = name("unassigned");

    for candidate in [request(None), unknown_module, wrong_proof] {
        assert_eq!(
            operations()
                .finalize(
                    &SealedAuthority,
                    prepared.checkpoint(),
                    &candidate,
                    &sealer,
                    obligation(),
                )
                .map(|_| ())
                .expect_err("a sealed deployment refuses every request"),
            InitError::AlreadyInitialized
        );
    }

    // The same requests do produce their own distinct errors under an eligible
    // authority, so the uniform answer above is not simply the only answer
    // these requests can ever get.
    assert_eq!(
        operations()
            .finalize(
                &EligibleAuthority::new(),
                prepared.checkpoint(),
                &request(None),
                &sealer,
                obligation(),
            )
            .map(|_| ())
            .expect_err("a missing proof is its own category"),
        InitError::RecoveryKeyConfirmationRequired
    );
    assert!(sealer.calls().is_empty());
}

#[test]
fn the_authority_is_consulted_once_for_every_mutating_operation() {
    let authority = EligibleAuthority::new();
    let operations = operations();

    let prepared = operations
        .prepare_delivery(&authority)
        .expect("preparation succeeds");
    assert_eq!(*authority.consultations.borrow(), 1);

    operations
        .finalize(
            &authority,
            prepared.checkpoint(),
            &request(Some(valid_proof(&prepared))),
            &RecordingSealer::new(),
            obligation(),
        )
        .expect("a valid request initializes the deployment");
    assert_eq!(*authority.consultations.borrow(), 2);
}

#[test]
fn request_validation_rejects_state_this_build_cannot_serve() {
    let components = components();

    let mut unknown_module = request(None);
    unknown_module.log_module_configurations[0].module = name("not-compiled-in");
    assert_eq!(
        validate_request(&unknown_module, &components).map(|_| ()),
        Err(RequestError::ComponentUnavailable)
    );

    let mut duplicate_name = request(None);
    duplicate_name.log_module_configurations[1].name = name(SYSTEM_CONFIGURATION);
    assert_eq!(
        validate_request(&duplicate_name, &components).map(|_| ()),
        Err(RequestError::DuplicateEntry)
    );

    let mut unresolved = request(None);
    unresolved.audit_log = name("never-submitted");
    assert_eq!(
        validate_request(&unresolved, &components).map(|_| ()),
        Err(RequestError::UnresolvedAssignment)
    );

    let mut disabled = request(None);
    disabled.log_module_configurations[1].enabled = false;
    assert_eq!(
        validate_request(&disabled, &components).map(|_| ()),
        Err(RequestError::DisabledAssignment)
    );

    let mut shared = request(None);
    shared.audit_log = name(SYSTEM_CONFIGURATION);
    assert_eq!(
        validate_request(&shared, &components).map(|_| ()),
        Err(RequestError::DuplicateEntry)
    );

    // The unmodified request validates, so each rejection above is caused by
    // its mutation rather than by a fixture that never validated.
    assert!(validate_request(&request(None), &components).is_ok());
}

#[test]
fn request_validation_rejects_a_setting_its_log_module_does_not_declare() {
    let components = components();

    let mut undeclared = request(None);
    undeclared.log_module_configurations[0]
        .settings
        .push(LogModuleSetting {
            key: key("retention-days"),
            value: ConfigurationValue::new("30").expect("the fixture value is accepted"),
        });
    assert_eq!(
        validate_request(&undeclared, &components).map(|_| ()),
        Err(RequestError::SettingUnsupported),
        "a setting the named Log Module never declared cannot reach finalization"
    );

    // A module that declares no setting at all accepts only a configuration
    // that carries none, which is what the compiled-in module does.
    let declares_nothing = AvailableComponents {
        log_modules: [(name(LOG_MODULE), LogSettingsFormat::default())]
            .into_iter()
            .collect(),
        ..components.clone()
    };
    assert_eq!(
        validate_request(&request(None), &declares_nothing).map(|_| ()),
        Err(RequestError::SettingUnsupported)
    );
    let mut unconfigured = request(None);
    for configuration in &mut unconfigured.log_module_configurations {
        configuration.settings.clear();
    }
    assert!(
        validate_request(&unconfigured, &declares_nothing).is_ok(),
        "a configuration carrying nothing the module refuses is accepted"
    );

    // The declaration governs non-secret settings only: a secret setting is
    // never carried to the module through it, so it is not judged against it.
    let mut protected = request(None);
    protected.log_module_configurations[0]
        .protected_settings
        .push(InitialProtectedSetting {
            key: key("connection-secret"),
            value: InitialSecret::new(LOG_SECRET.to_vec()).expect("the fixture secret is bounded"),
        });
    assert!(
        validate_request(&protected, &components).is_ok(),
        "a protected setting is outside the declared non-secret format"
    );
}

#[test]
fn an_undeclared_setting_does_not_take_an_earlier_rejection_away() {
    let components = components();
    let undeclared = LogModuleSetting {
        key: key("retention-days"),
        value: ConfigurationValue::new("30").expect("the fixture value is accepted"),
    };

    let mut unavailable = request(None);
    unavailable.log_module_configurations[0].module = name("not-compiled-in");
    unavailable.log_module_configurations[0]
        .settings
        .push(undeclared.clone());
    assert_eq!(
        validate_request(&unavailable, &components).map(|_| ()),
        Err(RequestError::ComponentUnavailable)
    );

    let mut duplicated = request(None);
    duplicated.log_module_configurations[0]
        .settings
        .push(undeclared.clone());
    duplicated.log_module_configurations[0]
        .settings
        .push(undeclared.clone());
    assert_eq!(
        validate_request(&duplicated, &components).map(|_| ()),
        Err(RequestError::DuplicateEntry)
    );

    let mut overlong = request(None);
    overlong.log_module_configurations[0].settings = (0..=MAX_LOG_MODULE_SETTINGS)
        .map(|index| LogModuleSetting {
            key: key(&format!("setting-{index}")),
            value: undeclared.value.clone(),
        })
        .collect();
    assert_eq!(
        validate_request(&overlong, &components).map(|_| ()),
        Err(RequestError::CollectionOutOfBounds)
    );
}

#[test]
fn a_request_whose_settings_its_log_module_declares_initializes() {
    let operations = operations();
    let authority = EligibleAuthority::new();
    let prepared = operations
        .prepare_delivery(&authority)
        .expect("preparation succeeds");

    let accepted = request(Some(valid_proof(&prepared)));
    assert!(
        accepted
            .log_module_configurations
            .iter()
            .all(|configuration| !configuration.settings.is_empty()),
        "the accepted request must actually carry a declared setting"
    );
    assert!(validate_request(&accepted, &components()).is_ok());

    operations
        .finalize(
            &authority,
            prepared.checkpoint(),
            &accepted,
            &RecordingSealer::new(),
            obligation(),
        )
        .expect("a request carrying only declared settings initializes");
}

#[test]
fn request_validation_rejects_collections_outside_their_bounds() {
    let components = components();

    let mut empty = request(None);
    empty.log_module_configurations.clear();
    assert_eq!(
        validate_request(&empty, &components).map(|_| ()),
        Err(RequestError::CollectionOutOfBounds)
    );

    let mut overlong = request(None);
    overlong.log_module_configurations = (0..=MAX_LOG_MODULE_CONFIGURATIONS)
        .map(|index| configuration(&format!("configuration-{index}"), true))
        .collect();
    assert_eq!(
        overlong.log_module_configurations.len(),
        MAX_LOG_MODULE_CONFIGURATIONS + 1
    );
    assert_eq!(
        validate_request(&overlong, &components).map(|_| ()),
        Err(RequestError::CollectionOutOfBounds)
    );

    let mut at_bound = request(None);
    at_bound.log_module_configurations = (0..MAX_LOG_MODULE_CONFIGURATIONS)
        .map(|index| configuration(&format!("configuration-{index}"), true))
        .collect();
    at_bound.system_log = name("configuration-0");
    at_bound.audit_log = name("configuration-1");
    assert!(
        validate_request(&at_bound, &components).is_ok(),
        "the exact bound is accepted"
    );

    let mut duplicate_protected = request(None);
    for index in 0..2 {
        duplicate_protected.log_module_configurations[index]
            .protected_settings
            .push(InitialProtectedSetting {
                key: key("connection-secret"),
                value: InitialSecret::new(LOG_SECRET.to_vec())
                    .expect("the fixture secret is bounded"),
            });
    }
    assert_eq!(
        validate_request(&duplicate_protected, &components).map(|_| ()),
        Err(RequestError::DuplicateEntry),
        "two configurations of one Log Module cannot claim the same secret key"
    );
}

#[test]
fn an_invalid_request_produces_no_state_and_no_side_effect() {
    let prepared = PreparedInitDelivery::prepare().expect("preparation succeeds");
    let sealer = RecordingSealer::new();
    let mut invalid = request(Some(valid_proof(&prepared)));
    invalid.log_module_configurations[0]
        .protected_settings
        .push(InitialProtectedSetting {
            key: key("connection-secret"),
            value: InitialSecret::new(LOG_SECRET.to_vec()).expect("the fixture secret is bounded"),
        });
    invalid.audit_log = name("never-submitted");

    let error = operations()
        .finalize(
            &EligibleAuthority::new(),
            prepared.checkpoint(),
            &invalid,
            &sealer,
            obligation(),
        )
        .map(|_| ())
        .expect_err("an unresolvable assignment produces no state");

    assert_eq!(error, InitError::InitializationFailed);
    assert!(
        sealer.calls().is_empty(),
        "state construction is not started for a request that failed validation"
    );
}

#[test]
fn the_operations_refuse_to_compose_against_a_client_module_this_build_lacks() {
    assert_eq!(
        InitOperations::new(components(), name("cli")).map(|_| ()),
        Err(InitError::InitializationFailed)
    );
    assert_eq!(
        operations().administration_client_module(),
        &name(WEB_UI),
        "the composed Client Module is the one the Group grants access to"
    );
}

#[test]
fn workflow_refusals_map_to_stable_categories_that_carry_no_detail() {
    assert_eq!(
        InitError::from(WorkflowError::AlreadyInitialized),
        InitError::AlreadyInitialized
    );
    assert_eq!(
        InitError::from(WorkflowError::Lifecycle(LifecycleError::Persistence)),
        InitError::Lifecycle(LifecycleError::Persistence)
    );
    for refused in [
        WorkflowError::NotAllowed,
        WorkflowError::AlreadyPending,
        WorkflowError::DatabaseNotSelected,
        WorkflowError::StateMismatch,
    ] {
        assert_eq!(
            InitError::from(refused),
            InitError::Lifecycle(LifecycleError::InvalidState)
        );
    }

    for (error, expected) in [
        (
            InitError::RecoveryKeyConfirmationRequired,
            "recovery_key_confirmation_required",
        ),
        (
            InitError::RecoveryKeyConfirmationInvalid,
            "recovery_key_confirmation_invalid",
        ),
        (InitError::InitializationFailed, "initialization_failed"),
        (InitError::AlreadyInitialized, "deployment_state_invalid"),
        (
            InitError::Lifecycle(LifecycleError::Persistence),
            "storage_unavailable",
        ),
    ] {
        assert_eq!(error.category_reason().0, expected);
        assert_eq!(error.to_string(), expected);
    }
}

#[test]
fn no_error_rendering_carries_a_submitted_secret() {
    let password =
        InitialPassword::new(PASSWORD.to_owned()).expect("the fixture password is valid");
    let secret = InitialSecret::new(LOG_SECRET.to_vec()).expect("the fixture secret is valid");

    // Both values are proven present through their accessors before their
    // renderings are asserted to exclude them.
    assert_eq!(password.len(), PASSWORD.len());
    assert_eq!(secret.len(), LOG_SECRET.len());
    assert!(!password.is_empty());
    assert!(!secret.is_empty());

    let rendered = format!("{password:?} {secret:?}");
    assert!(!rendered.contains(PASSWORD));
    assert!(!rendered.contains("battery"));
    assert!(!rendered.contains(std::str::from_utf8(LOG_SECRET).expect("the fixture is UTF-8")));

    assert_eq!(
        InitialPassword::new("a".repeat(MAX_PASSWORD_BYTES + 1)).map(|_| ()),
        Err(RequestError::SecretTooLong)
    );
    assert_eq!(
        RequestError::SecretTooLong.to_string(),
        "initialization request is invalid"
    );
    assert_eq!(
        CheckpointError::Malformed.to_string(),
        "initialization checkpoint is invalid"
    );
}

#[test]
fn an_authorization_is_bound_to_the_deployment_the_authority_confirmed() {
    let deployment = DeploymentIdentifier::from_bytes([3; 16]).expect("the fixture is non-zero");
    let target = InitTarget::new(deployment);
    assert_eq!(target.deployment_identifier(), deployment);

    let authority = EligibleAuthority::new();
    assert_eq!(
        authority
            .authorize()
            .expect("the fixture authority permits Init")
            .deployment_identifier(),
        deployment
    );

    // `AuthorizedInit` is nameable but not constructible outside this crate,
    // which is what the forbidden-construction fixtures pin.
    fn accepts_authorization(_authorized: &AuthorizedInit) {}
    let _ = accepts_authorization;
}

#[test]
fn a_delivery_nonce_and_proof_are_opaque_in_every_rendering() {
    let nonce = DeliveryNonce::from_bytes([0xAB; 32]);
    let proof = RecoveryProof::from_bytes([0xCD; 32]);

    assert_eq!(nonce.as_bytes(), &[0xAB; 32]);
    assert_eq!(proof.as_bytes(), &[0xCD; 32]);
    let rendered = format!("{nonce:?} {proof:?}");
    assert!(!rendered.contains("171"));
    assert!(!rendered.contains("205"));
    assert!(!rendered.contains("ab"));
}
