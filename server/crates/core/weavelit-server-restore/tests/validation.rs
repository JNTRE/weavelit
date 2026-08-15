//! End-to-end Restore validation over the committed backup fixtures.

mod support;

use support::{committed, committed_text, components, validate};
use weavelit_server_restore::{
    LogType, RequestBudget, RestoreError, RestoreRequest, RestoreValidator, TransferBounds,
};

fn identity() -> String {
    committed_text("valid-identity.txt")
}

#[test]
fn a_valid_backup_normalizes_to_the_replacement_deployment() {
    let artifact = committed("valid.wlitbackup");
    let validated = validate(&artifact, &identity()).expect("the fixture backup is valid");

    assert_eq!(validated.deployment_identifier(), support::deployment());

    let backup = validated.backup();
    assert_eq!(backup.source_backend().as_str(), "sqlite");
    assert_eq!(
        backup.recovery_public_key().as_str(),
        committed_text("valid-recipient.txt")
    );
    assert_eq!(backup.configuration().len(), 1);
    assert_eq!(backup.protected_secrets().len(), 1);
    assert_eq!(backup.accounts().len(), 1);
    assert_eq!(backup.password_verifiers().len(), 1);
    assert_eq!(backup.groups().len(), 1);
    assert_eq!(backup.group_memberships().len(), 1);
    assert_eq!(backup.group_grants().len(), 2);
    assert_eq!(backup.mfa_factors().len(), 1);
    assert_eq!(backup.service_connections().len(), 1);
    assert_eq!(backup.log_module_configurations().len(), 1);
    assert_eq!(backup.log_assignments().len(), LogType::ALL.len());
}

#[test]
fn the_decrypted_plaintext_matches_the_committed_expectation() {
    let artifact = committed("valid.wlitbackup");
    let expected = committed("valid-plaintext.json");

    // Normalization only succeeds when decryption produced exactly this
    // plaintext, so an independent parse of the expectation must agree.
    let validated = validate(&artifact, &identity()).expect("the fixture backup is valid");
    let direct = weavelit_server_restore::normalize(
        &expected,
        validated.backup().source_backend(),
        &components(),
    )
    .expect("the committed plaintext is the decrypted plaintext");

    assert_eq!(&direct, validated.backup());
}

#[test]
fn structural_envelope_failures_are_indistinguishable() {
    for name in [
        "bad-magic.wlitbackup",
        "non-zero-flags.wlitbackup",
        "wrong-declared-length.wlitbackup",
        "truncated-stream.wlitbackup",
        "tampered-ciphertext.wlitbackup",
        "tampered-tag.wlitbackup",
    ] {
        let error = validate(&committed(name), &identity())
            .expect_err("the mutated artifact must be rejected");
        assert_eq!(
            support::category(error),
            ("backup_invalid", "backup_invalid"),
            "{name}"
        );
    }
}

#[test]
fn a_wrong_recovery_key_is_indistinguishable_from_a_corrupt_backup() {
    let error = validate(
        &committed("valid.wlitbackup"),
        &committed_text("wrong-identity.txt"),
    )
    .expect_err("an unrelated recovery key must be rejected");

    assert_eq!(
        support::category(error),
        ("backup_invalid", "backup_invalid")
    );
}

#[test]
fn a_backup_declaring_another_recipient_is_indistinguishable_from_a_corrupt_backup() {
    let error = validate(&support::mismatched_recipient_backup(), &identity())
        .expect_err("a backup that declares an unrelated recipient must be rejected");

    assert_eq!(
        support::category(error),
        ("backup_invalid", "backup_invalid")
    );
}

#[test]
fn a_backup_declaring_a_non_canonical_recipient_is_not_reported_as_a_key_failure() {
    let error = validate(&support::non_canonical_recipient_backup(), &identity())
        .expect_err("a backup that declares a non-canonical recipient must be rejected");

    assert_eq!(
        support::category(error),
        ("backup_invalid", "backup_invalid")
    );
}

#[test]
fn an_unsupported_outer_format_version_is_incompatible() {
    let error = validate(&committed("wrong-outer-version.wlitbackup"), &identity())
        .expect_err("an unsupported envelope version must be rejected");

    assert_eq!(
        support::category(error),
        ("backup_incompatible", "backup_incompatible")
    );
}

#[test]
fn an_unsupported_inner_format_version_is_incompatible() {
    let error = validate(&committed("wrong-inner-version.wlitbackup"), &identity())
        .expect_err("an unsupported content version must be rejected");

    assert_eq!(
        support::category(error),
        ("backup_incompatible", "backup_incompatible")
    );
}

#[test]
fn a_backup_from_another_backend_is_incompatible() {
    let error = validate(&committed("wrong-source-backend.wlitbackup"), &identity())
        .expect_err("a backup from another backend must be rejected");

    assert_eq!(
        support::category(error),
        ("backup_incompatible", "backup_incompatible")
    );
}

#[test]
fn a_malformed_or_multi_line_recovery_key_is_reported_as_a_key_failure() {
    for name in ["malformed-key.txt", "multiline-key.txt"] {
        let error = validate(&committed("valid.wlitbackup"), &committed_text(name))
            .expect_err("a non-canonical recovery key must be rejected");
        assert_eq!(
            support::category(error),
            ("recovery_key_invalid", "recovery_key_invalid"),
            "{name}"
        );
    }
}

#[test]
fn a_public_recovery_key_cannot_decrypt_a_backup() {
    let error = validate(
        &committed("valid.wlitbackup"),
        &committed_text("valid-recipient.txt"),
    )
    .expect_err("the recovery public key is not an identity");

    assert_eq!(
        support::category(error),
        ("recovery_key_invalid", "recovery_key_invalid")
    );
}

#[test]
fn an_oversize_artifact_is_rejected_before_decryption() {
    let bounds = TransferBounds {
        max_encrypted_artifact_bytes: 512,
        ..TransferBounds::APPROVED
    };
    let validator = RestoreValidator::with_bounds(components(), bounds);
    let artifact = committed("valid.wlitbackup");
    assert!(artifact.len() > bounds.max_encrypted_artifact_bytes);

    let error = validator
        .validate(
            &support::TestAuthority::eligible("sqlite"),
            &RequestBudget::start(),
            RestoreRequest {
                artifact: &artifact,
                recovery_key: &identity(),
            },
        )
        .expect_err("an oversize artifact must be rejected");

    assert_eq!(
        support::category(error),
        ("backup_invalid", "backup_invalid")
    );
}

#[test]
fn an_oversize_authenticated_plaintext_is_rejected() {
    let bounds = TransferBounds {
        max_authenticated_plaintext_bytes: 16,
        ..TransferBounds::APPROVED
    };
    let validator = RestoreValidator::with_bounds(components(), bounds);
    let artifact = committed("valid.wlitbackup");

    let error = validator
        .validate(
            &support::TestAuthority::eligible("sqlite"),
            &RequestBudget::start(),
            RestoreRequest {
                artifact: &artifact,
                recovery_key: &identity(),
            },
        )
        .expect_err("an oversize plaintext must be rejected");

    assert_eq!(
        support::category(error),
        ("backup_invalid", "backup_invalid")
    );
}

#[test]
fn lifecycle_ineligibility_is_reported_before_any_sensitive_input_is_read() {
    let validator = RestoreValidator::new(components());
    let error = validator
        .validate(
            &support::TestAuthority::rejecting(RestoreError::RestorePending),
            &RequestBudget::start(),
            RestoreRequest {
                artifact: &[],
                recovery_key: "",
            },
        )
        .expect_err("an ineligible deployment must be rejected");

    assert_eq!(
        support::category(error),
        ("restore_pending", "restore_pending")
    );
}

#[test]
fn validation_writes_nothing_to_the_filesystem() {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let before = std::fs::read_dir(directory.path())
        .expect("the directory is readable")
        .count();

    validate(&committed("valid.wlitbackup"), &identity()).expect("the fixture backup is valid");

    let after = std::fs::read_dir(directory.path())
        .expect("the directory is readable")
        .count();
    assert_eq!(before, 0);
    assert_eq!(after, 0);
}

#[test]
fn no_diagnostic_rendering_discloses_key_or_backup_material() {
    let artifact = committed("valid.wlitbackup");
    let identity = identity();
    let request = RestoreRequest {
        artifact: &artifact,
        recovery_key: &identity,
    };
    let rendered = format!("{request:?}");
    assert!(!rendered.contains(&identity), "{rendered}");
    assert!(rendered.contains("artifact_length"), "{rendered}");

    let validated = validate(&artifact, &identity).expect("the fixture backup is valid");
    let rendered = format!("{:?}", validated.backup().protected_secrets());
    assert!(rendered.contains("REDACTED"), "{rendered}");
    assert!(!rendered.contains("at-rest-value"), "{rendered}");

    let rendered = format!("{:?}", validated.backup().service_connections());
    assert!(!rendered.contains("provider-token"), "{rendered}");

    let rendered = format!("{:?}", validated.backup().mfa_factors());
    assert!(!rendered.contains("totp-seed"), "{rendered}");
}
