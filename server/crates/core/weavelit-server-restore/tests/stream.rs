//! Multi-chunk age STREAM decryption beyond the committed one-chunk fixtures.
//!
//! The committed fixtures are all a single STREAM chunk, so these tests
//! generate larger artifacts at run time to exercise chunk counting, the
//! final-chunk flag, and truncation.

mod support;

use support::{
    CHUNK_PLAINTEXT_LENGTH, committed_text, generated_backup, generated_backup_without_final_flag,
    truncate_artifact, validate,
};

fn identity() -> String {
    committed_text("valid-identity.txt")
}

#[test]
fn a_multi_chunk_stream_decrypts_every_chunk_in_order() {
    let generated = generated_backup(CHUNK_PLAINTEXT_LENGTH * 2 + 1_000);

    let validated =
        validate(&generated.artifact, &identity()).expect("a multi-chunk backup is valid");

    assert_eq!(
        validated.backup().configuration().len(),
        generated.configuration_entries
    );
    assert_eq!(validated.backup().source_backend().as_str(), "sqlite");
}

#[test]
fn a_plaintext_that_ends_on_a_chunk_boundary_decrypts() {
    // A plaintext that is an exact multiple of the chunk size ends in a full
    // final chunk, which is the case a reader is most likely to mis-flag.
    let generated = generated_backup(CHUNK_PLAINTEXT_LENGTH * 2);
    assert_eq!(generated.plaintext.len() % CHUNK_PLAINTEXT_LENGTH, 0);

    let validated =
        validate(&generated.artifact, &identity()).expect("a chunk-aligned backup is valid");

    assert_eq!(
        validated.backup().configuration().len(),
        generated.configuration_entries
    );
}

#[test]
fn a_final_chunk_without_the_final_flag_is_rejected() {
    for length in [
        CHUNK_PLAINTEXT_LENGTH / 2,
        CHUNK_PLAINTEXT_LENGTH * 2,
        CHUNK_PLAINTEXT_LENGTH * 2 + 1_000,
    ] {
        let artifact = generated_backup_without_final_flag(length);
        let error = validate(&artifact, &identity())
            .expect_err("an unflagged final chunk must be rejected");
        assert_eq!(
            support::category(error),
            ("backup_invalid", "backup_invalid"),
            "{length}"
        );
    }
}

#[test]
fn a_truncated_multi_chunk_stream_is_rejected() {
    let generated = generated_backup(CHUNK_PLAINTEXT_LENGTH * 2 + 1_000);
    let truncated = truncate_artifact(&generated.artifact);

    let error = validate(&truncated, &identity()).expect_err("a truncated stream must be rejected");

    assert_eq!(
        support::category(error),
        ("backup_invalid", "backup_invalid")
    );
}

#[test]
fn a_stream_missing_its_whole_final_chunk_is_rejected() {
    let generated = generated_backup(CHUNK_PLAINTEXT_LENGTH * 2 + 1_000);
    let mut cut = generated.artifact;
    cut.truncate(cut.len() - (1_000 + 16));
    let declared = (cut.len() - 20) as u64;
    cut[12..20].copy_from_slice(&declared.to_be_bytes());

    let error = validate(&cut, &identity()).expect_err("a dropped final chunk must be rejected");

    assert_eq!(
        support::category(error),
        ("backup_invalid", "backup_invalid")
    );
}

#[test]
fn a_tampered_interior_chunk_is_rejected() {
    let generated = generated_backup(CHUNK_PLAINTEXT_LENGTH * 2 + 1_000);
    let mut tampered = generated.artifact;
    // The outer header is 20 bytes and the age header precedes the payload
    // nonce, so this byte falls inside the first STREAM chunk's ciphertext.
    let index = tampered.len() - (CHUNK_PLAINTEXT_LENGTH * 2);
    tampered[index] ^= 0x01;

    let error =
        validate(&tampered, &identity()).expect_err("an altered interior chunk must be rejected");

    assert_eq!(
        support::category(error),
        ("backup_invalid", "backup_invalid")
    );
}

#[test]
fn an_oversize_multi_chunk_plaintext_is_rejected_before_it_is_fully_decrypted() {
    let generated = generated_backup(CHUNK_PLAINTEXT_LENGTH * 3);
    let bounds = weavelit_server_restore::TransferBounds {
        max_authenticated_plaintext_bytes: CHUNK_PLAINTEXT_LENGTH,
        ..weavelit_server_restore::TransferBounds::APPROVED
    };

    let error =
        weavelit_server_restore::RestoreValidator::with_bounds(support::components(), bounds)
            .validate(
                &support::TestAuthority::eligible("sqlite"),
                &weavelit_server_restore::RequestBudget::start(),
                weavelit_server_restore::RestoreRequest {
                    artifact: &generated.artifact,
                    recovery_key: &identity(),
                },
            )
            .expect_err("a plaintext over the bound must be rejected");

    assert_eq!(
        support::category(error),
        ("backup_invalid", "backup_invalid")
    );
}
