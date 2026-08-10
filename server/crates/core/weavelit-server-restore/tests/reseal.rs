//! Re-sealing of recovered secrets into replacement application state.

mod support;

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
};

use support::{committed, committed_text, validate};
use weavelit_server_database::{
    CompletionObligation, CorrelationIdentifier, LogClassification, LogDetail, ProtectedValue,
    StateIdentifier, WorkflowKind,
};
use weavelit_server_lifecycle::{LifecycleError, ProtectedValueKind, ProtectedValueSealer};
use weavelit_server_restore::{RestoreError, ValidatedBackup, build_application_state};

/// Records what it was asked to seal so a test can prove the binding, and
/// returns a distinguishable value rather than real encryption.
struct RecordingSealer {
    calls: RefCell<Vec<(ProtectedValueKind, Vec<u8>)>>,
    fail_on: Option<ProtectedValueKind>,
}

impl RecordingSealer {
    fn new() -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
            fail_on: None,
        }
    }

    fn failing(kind: ProtectedValueKind) -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
            fail_on: Some(kind),
        }
    }

    fn kinds_for(&self, plaintext: &[u8]) -> Vec<ProtectedValueKind> {
        self.calls
            .borrow()
            .iter()
            .filter(|(_, recorded)| recorded == plaintext)
            .map(|(kind, _)| *kind)
            .collect()
    }
}

impl ProtectedValueSealer for RecordingSealer {
    fn seal(
        &self,
        kind: ProtectedValueKind,
        plaintext: &[u8],
    ) -> Result<ProtectedValue, LifecycleError> {
        if self.fail_on == Some(kind) {
            return Err(LifecycleError::IntegrityFailure);
        }
        let mut calls = self.calls.borrow_mut();
        calls.push((kind, plaintext.to_vec()));
        let mut sealed = format!("sealed-{}-", calls.len()).into_bytes();
        sealed.extend(plaintext.iter().map(|byte| !byte));
        Ok(ProtectedValue::new(sealed).expect("the sealed value is bounded"))
    }
}

fn obligation() -> CompletionObligation {
    CompletionObligation::new(
        StateIdentifier::from_bytes([0x5A; 16]).unwrap(),
        WorkflowKind::Restore,
        LogClassification::new("lifecycle.restore").unwrap(),
        CorrelationIdentifier::new("correlation-identifier").unwrap(),
        1_700_000_000_000,
        LogDetail::new("restore completed").unwrap(),
    )
    .unwrap()
}

fn validated() -> ValidatedBackup {
    validate(
        &committed("valid.wlitbackup"),
        &committed_text("valid-identity.txt"),
    )
    .expect("the committed fixture is a valid backup")
}

#[test]
fn every_recovered_secret_is_sealed_under_its_own_kind() {
    let validated = validated();
    let sealer = RecordingSealer::new();

    let state = build_application_state(&validated, &sealer, obligation())
        .expect("a validated backup must transform");

    let backup = validated.backup();
    assert_eq!(
        sealer.calls.borrow().len(),
        backup.protected_secrets().len()
            + backup.mfa_factors().len()
            + backup.service_connections().len(),
        "every recovered secret must be sealed exactly once"
    );
    for secret in backup.protected_secrets() {
        assert_eq!(
            sealer.kinds_for(secret.value.expose()),
            vec![ProtectedValueKind::ComponentSecret]
        );
    }
    for factor in backup.mfa_factors() {
        assert_eq!(
            sealer.kinds_for(factor.factor_data.expose()),
            vec![ProtectedValueKind::MfaFactorData]
        );
    }
    for connection in backup.service_connections() {
        assert_eq!(
            sealer.kinds_for(connection.credential.expose()),
            vec![ProtectedValueKind::ServiceConnectionCredential]
        );
    }
    assert_eq!(
        state.protected_secrets().len(),
        backup.protected_secrets().len()
    );
}

#[test]
fn no_committed_value_carries_the_backups_own_plaintext() {
    let validated = validated();
    let state = build_application_state(&validated, &RecordingSealer::new(), obligation()).unwrap();

    let plaintexts: Vec<&[u8]> = validated
        .backup()
        .protected_secrets()
        .iter()
        .map(|secret| secret.value.expose())
        .chain(
            validated
                .backup()
                .mfa_factors()
                .iter()
                .map(|factor| factor.factor_data.expose()),
        )
        .chain(
            validated
                .backup()
                .service_connections()
                .iter()
                .map(|connection| connection.credential.expose()),
        )
        .collect();

    let committed_values: Vec<&[u8]> = state
        .protected_secrets()
        .iter()
        .map(|secret| secret.value.as_bytes())
        .chain(
            state
                .mfa_factors()
                .iter()
                .map(|factor| factor.protected_factor_data.as_bytes()),
        )
        .chain(
            state
                .service_connections()
                .iter()
                .map(|connection| connection.protected_credential.as_bytes()),
        )
        .collect();

    assert!(!plaintexts.is_empty(), "the fixture must carry secrets");
    for committed_value in &committed_values {
        for plaintext in &plaintexts {
            assert!(
                !committed_value
                    .windows(plaintext.len())
                    .any(|candidate| candidate == *plaintext),
                "a committed value must not contain a recovered plaintext"
            );
        }
    }
    assert_eq!(
        committed_values.iter().collect::<BTreeSet<_>>().len(),
        committed_values.len(),
        "each secret must be sealed independently"
    );
}

#[test]
fn non_secret_state_is_carried_through_unchanged() {
    let validated = validated();
    let state = build_application_state(&validated, &RecordingSealer::new(), obligation()).unwrap();
    let backup = validated.backup();

    assert_eq!(state.accounts(), backup.accounts());
    assert_eq!(state.groups(), backup.groups());
    assert_eq!(state.group_memberships(), backup.group_memberships());
    assert_eq!(state.group_grants(), backup.group_grants());
    assert_eq!(state.configuration(), backup.configuration());
    assert_eq!(state.password_verifiers(), backup.password_verifiers());
    assert_eq!(state.log_assignments(), backup.log_assignments());
    assert_eq!(
        state.log_module_configurations(),
        backup.log_module_configurations()
    );
    assert_eq!(state.recovery_public_key(), backup.recovery_public_key());
    assert_eq!(state.completion_obligation(), &obligation());
}

#[test]
fn secret_identity_and_ownership_survive_the_reseal() {
    let validated = validated();
    let state = build_application_state(&validated, &RecordingSealer::new(), obligation()).unwrap();
    let backup = validated.backup();

    let expected: BTreeMap<_, _> = backup
        .mfa_factors()
        .iter()
        .map(|factor| (factor.identifier, (factor.account, factor.module.clone())))
        .collect();
    let actual: BTreeMap<_, _> = state
        .mfa_factors()
        .iter()
        .map(|factor| (factor.identifier, (factor.account, factor.module.clone())))
        .collect();
    assert_eq!(actual, expected);

    let expected: BTreeMap<_, _> = backup
        .service_connections()
        .iter()
        .map(|connection| {
            (
                connection.identifier,
                (connection.service_module.clone(), connection.name.clone()),
            )
        })
        .collect();
    let actual: BTreeMap<_, _> = state
        .service_connections()
        .iter()
        .map(|connection| {
            (
                connection.identifier,
                (connection.service_module.clone(), connection.name.clone()),
            )
        })
        .collect();
    assert_eq!(actual, expected);
}

#[test]
fn a_seal_failure_stops_the_transformation() {
    let validated = validated();

    for kind in [
        ProtectedValueKind::ComponentSecret,
        ProtectedValueKind::MfaFactorData,
        ProtectedValueKind::ServiceConnectionCredential,
    ] {
        assert_eq!(
            build_application_state(&validated, &RecordingSealer::failing(kind), obligation())
                .unwrap_err(),
            RestoreError::RestoreFailed
        );
    }
}
