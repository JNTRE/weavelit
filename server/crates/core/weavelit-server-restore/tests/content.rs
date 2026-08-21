//! Compatibility, reference, component, and domain checks on backup content.

mod support;

use base64::Engine as _;
use support::{
    FIXTURE_TOTP_SECRET, account_public_identifier_persistence, committed, committed_text,
    components, group_public_identifier_persistence, persistence,
};
use weavelit_server_database::{CredentialRevision, MAX_NAME_LENGTH};
use weavelit_server_restore::{
    Account, AvailableComponents, BACKUP_CONTENT_FORMAT_VERSION, BackendIdentifier, ContentError,
    GroupGrant, LogSettingsFormat, MAX_COLLECTION_ENTRIES, MAX_LOG_MODULE_SETTINGS,
    MAX_SENSITIVE_VALUE_BYTES, Name, NormalizedBackup, RestoreError, SensitiveBytes,
    normalize as normalize_with_persistence,
};

fn normalize(
    plaintext: &[u8],
    selected_backend: &BackendIdentifier,
    available_components: &AvailableComponents,
) -> Result<NormalizedBackup, ContentError> {
    normalize_with_persistence(
        plaintext,
        selected_backend,
        &account_public_identifier_persistence(),
        &group_public_identifier_persistence(),
        &persistence(),
        available_components,
    )
}

fn sqlite() -> BackendIdentifier {
    BackendIdentifier::new("sqlite").expect("the backend identifier is valid")
}

fn plaintext() -> String {
    committed_text("valid-plaintext.json")
}

const FIXTURE_ACCOUNT_PUBLIC_ID: &str = "kpOUlZaXmJmam5ydnp-goQ";
const FIXTURE_GROUP_PUBLIC_ID: &str = "MTExMTExMTExMTExMTExMQ";

fn persisted_public_id(backup: &NormalizedBackup) -> [u8; 16] {
    account_public_identifier_persistence()
        .encode(&backup.account_public_identities()[0].public_identifier())
}

fn persisted_group_public_id(backup: &NormalizedBackup) -> [u8; 16] {
    group_public_identifier_persistence()
        .encode(&backup.group_public_identities()[0].public_identifier())
}

fn decoded_public_id(value: &str) -> [u8; 16] {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(value)
        .unwrap()
        .try_into()
        .unwrap()
}

fn replaced(from: &str, to: &str) -> String {
    let plaintext = plaintext();
    assert!(plaintext.contains(from), "the fixture contains {from:?}");
    plaintext.replace(from, to)
}

fn reject(document: &str) -> ContentError {
    normalize(document.as_bytes(), &sqlite(), &components())
        .expect_err("the mutated document must be rejected")
}

/// Prepends accounts, leaving the fixture account every reference resolves to.
fn accounts(added: &[(&str, &str)]) -> String {
    let entries = added
        .iter()
        .map(|(identifier, username)| {
            format!(
                "{{\"identifier\":\"{identifier}\",\"username\":\"{username}\",\"display_name\":null,\"active\":true}},"
            )
        })
        .collect::<String>();
    replaced("\"accounts\":[", &format!("\"accounts\":[{entries}"))
}

#[test]
fn the_committed_plaintext_normalizes() {
    let backup = normalize(&committed("valid-plaintext.json"), &sqlite(), &components())
        .expect("the committed plaintext is valid content");

    let account_reference = backup.account_audit_references()[0]
        .audit_reference()
        .to_string();
    let group_reference = backup.group_audit_references()[0]
        .audit_reference()
        .to_string();
    let configuration_reference = backup.log_configuration_audit_references()[0]
        .audit_reference()
        .to_string();
    assert!(account_reference.starts_with("ar-"));
    assert!(group_reference.starts_with("ar-"));
    assert!(configuration_reference.starts_with("ar-"));
    assert_ne!(account_reference, group_reference);
    assert_ne!(account_reference, configuration_reference);
    assert_ne!(group_reference, configuration_reference);
    assert!(!account_reference.contains(backup.accounts()[0].username.as_str()));
    assert!(!group_reference.contains(backup.groups()[0].name.as_str()));
    assert_eq!(
        backup.accounts()[0].credential_revision,
        CredentialRevision::INITIAL
    );
    assert!(!backup.accounts()[0].must_change_password);
    assert_eq!(backup.accounts()[0].temporary_credential_expiration, None);
}

#[test]
fn supplied_temporary_credential_metadata_survives_normalization() {
    let document = replaced(
        "\"active\":true}",
        "\"active\":true,\"credential_revision\":18446744073709551615,\
         \"must_change_password\":true,\
         \"temporary_credential_expires_at_milliseconds\":9223372036854775807}",
    );

    let backup = normalize(document.as_bytes(), &sqlite(), &components()).unwrap();
    let account = &backup.accounts()[0];
    assert_eq!(
        account.credential_revision,
        CredentialRevision::from_value(u64::MAX).unwrap()
    );
    assert!(account.must_change_password);
    assert_eq!(
        account
            .temporary_credential_expiration
            .unwrap()
            .as_unix_milliseconds(),
        i64::MAX
    );
}

#[test]
fn invalid_temporary_credential_metadata_is_uniformly_backup_invalid() {
    let mutations = [
        (
            "zero revision",
            "\"active\":true,\"credential_revision\":0}",
        ),
        (
            "malformed revision",
            "\"active\":true,\"credential_revision\":\"secret-revision\"}",
        ),
        (
            "negative expiration",
            "\"active\":true,\"must_change_password\":true,\
             \"temporary_credential_expires_at_milliseconds\":-1}",
        ),
        (
            "flag without expiration",
            "\"active\":true,\"must_change_password\":true}",
        ),
        (
            "expiration without flag",
            "\"active\":true,\
             \"temporary_credential_expires_at_milliseconds\":1}",
        ),
    ];

    for (label, replacement) in mutations {
        let document = replaced("\"active\":true}", replacement);
        let error = reject(&document);
        assert_eq!(
            RestoreError::from(error).category_reason(),
            ("backup_invalid", "backup_invalid"),
            "{label}"
        );
        assert!(!error.to_string().contains("secret-revision"), "{label}");
    }

    let temporary_without_verifier = replaced(
        "\"active\":true}",
        "\"active\":true,\"must_change_password\":true,\
         \"temporary_credential_expires_at_milliseconds\":1}",
    )
    .replace(
        "\"password_verifiers\":[{\"account\":\"AgMEBQYHCAkKCwwNDg8QEQ\",\"verifier\":\"$argon2id$v=19$m=65536,t=3,p=1$sbKztLW2t7i5uru8vb6/wA$gyw90gCqVs5nwE+ZFbfoD7UW6DPxegGqJR5JSFbObDQ\"}]",
        "\"password_verifiers\":[]",
    );
    let error = reject(&temporary_without_verifier);
    assert_eq!(
        RestoreError::from(error).category_reason(),
        ("backup_invalid", "backup_invalid")
    );
}

#[test]
fn supplied_account_public_id_survives_normalization_exactly() {
    let backup = normalize(plaintext().as_bytes(), &sqlite(), &components()).unwrap();

    assert_eq!(backup.account_public_identities().len(), 1);
    assert_eq!(
        backup.account_public_identities()[0].account(),
        backup.accounts()[0].identifier
    );
    assert_eq!(
        persisted_public_id(&backup),
        decoded_public_id(FIXTURE_ACCOUNT_PUBLIC_ID)
    );
}

#[test]
fn omitted_account_public_id_generates_a_fresh_nonzero_value() {
    let legacy = replaced(
        &format!("\"public_id\":\"{FIXTURE_ACCOUNT_PUBLIC_ID}\","),
        "",
    );
    let first = normalize(legacy.as_bytes(), &sqlite(), &components()).unwrap();
    let second = normalize(legacy.as_bytes(), &sqlite(), &components()).unwrap();

    assert_ne!(persisted_public_id(&first), [0; 16]);
    assert_ne!(persisted_public_id(&second), [0; 16]);
    assert_ne!(persisted_public_id(&first), persisted_public_id(&second));
}

#[test]
fn invalid_supplied_account_public_ids_are_rejected_without_payloads() {
    let wrong_length = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([0x51_u8; 15]);
    for (candidate, expected) in [
        ("not_base64url", ContentError::EncodingInvalid),
        ("kpOUlZaXmJmam5ydnp-goQ=", ContentError::Malformed),
        (wrong_length.as_str(), ContentError::EncodingInvalid),
        ("AAAAAAAAAAAAAAAAAAAAAA", ContentError::DomainInvalid),
    ] {
        let document = replaced(FIXTURE_ACCOUNT_PUBLIC_ID, candidate);
        let error = reject(&document);
        assert_eq!(error, expected);
        assert!(!error.to_string().contains(candidate));
        assert_eq!(
            RestoreError::from(error).category_reason(),
            ("backup_invalid", "backup_invalid")
        );
    }

    let explicit_null = replaced(
        &format!("\"public_id\":\"{FIXTURE_ACCOUNT_PUBLIC_ID}\""),
        "\"public_id\":null",
    );
    assert_eq!(reject(&explicit_null), ContentError::Malformed);
}

#[test]
fn duplicate_account_public_ids_are_rejected() {
    let duplicate = format!(
        "{{\"identifier\":\"AAAAAAAAAAAAAAAAAAAAAQ\",\"public_id\":\"{FIXTURE_ACCOUNT_PUBLIC_ID}\",\"username\":\"second\",\"display_name\":null,\"active\":true}},"
    );
    let document = replaced("\"accounts\":[", &format!("\"accounts\":[{duplicate}"));

    assert_eq!(reject(&document), ContentError::DuplicateEntry);
}

#[test]
fn supplied_group_public_id_survives_normalization_exactly() {
    let document = replaced(
        "\"name\":\"Administrators\"",
        &format!("\"public_id\":\"{FIXTURE_GROUP_PUBLIC_ID}\",\"name\":\"Administrators\""),
    );
    let backup = normalize(document.as_bytes(), &sqlite(), &components()).unwrap();

    assert_eq!(backup.group_public_identities().len(), 1);
    assert_eq!(
        backup.group_public_identities()[0].group(),
        backup.groups()[0].identifier
    );
    assert_eq!(
        persisted_group_public_id(&backup),
        decoded_public_id(FIXTURE_GROUP_PUBLIC_ID)
    );
}

#[test]
fn omitted_group_public_id_generates_a_fresh_nonzero_value() {
    let first = normalize(plaintext().as_bytes(), &sqlite(), &components()).unwrap();
    let second = normalize(plaintext().as_bytes(), &sqlite(), &components()).unwrap();

    assert_ne!(persisted_group_public_id(&first), [0; 16]);
    assert_ne!(persisted_group_public_id(&second), [0; 16]);
    assert_ne!(
        persisted_group_public_id(&first),
        persisted_group_public_id(&second)
    );
}

#[test]
fn invalid_supplied_group_public_ids_are_rejected_without_payloads() {
    let wrong_length = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([0x51_u8; 15]);
    for (candidate, expected) in [
        ("not_base64url", ContentError::EncodingInvalid),
        ("MTExMTExMTExMTExMTExMQ=", ContentError::Malformed),
        (wrong_length.as_str(), ContentError::EncodingInvalid),
        ("AAAAAAAAAAAAAAAAAAAAAA", ContentError::DomainInvalid),
    ] {
        let document = replaced(
            "\"name\":\"Administrators\"",
            &format!("\"public_id\":\"{candidate}\",\"name\":\"Administrators\""),
        );
        let error = reject(&document);
        assert_eq!(error, expected);
        assert!(!error.to_string().contains(candidate));
        assert_eq!(
            RestoreError::from(error).category_reason(),
            ("backup_invalid", "backup_invalid")
        );
    }

    let explicit_null = replaced(
        "\"name\":\"Administrators\"",
        "\"public_id\":null,\"name\":\"Administrators\"",
    );
    assert_eq!(reject(&explicit_null), ContentError::Malformed);
}

#[test]
fn duplicate_group_public_ids_are_rejected() {
    let duplicate = format!(
        "{{\"identifier\":\"AAAAAAAAAAAAAAAAAAAAAQ\",\"public_id\":\"{FIXTURE_GROUP_PUBLIC_ID}\",\"name\":\"Operators\",\"description\":null}},"
    );
    let document = replaced("\"groups\":[", &format!("\"groups\":[{duplicate}"));
    let document = document.replace(
        "\"name\":\"Administrators\"",
        &format!("\"public_id\":\"{FIXTURE_GROUP_PUBLIC_ID}\",\"name\":\"Administrators\""),
    );

    assert_eq!(reject(&document), ContentError::DuplicateEntry);
}

#[test]
fn totp_enablement_normalizes_to_one_canonical_entry() {
    for (entries, expected) in [
        (
            r#"{"component":"mfa.totp","key":"enabled","value":"true"}"#,
            "true",
        ),
        (
            r#"{"component":"mfa.totp","key":"enabled","value":"false"}"#,
            "false",
        ),
        (
            r#"{"component":"mfa.totp","key":"enabled","value":"yes"}"#,
            "false",
        ),
        (
            r#"{"component":"totp","key":"mfa-module.enabled","value":"true"}"#,
            "true",
        ),
        (
            r#"{"component":"mfa.totp","key":"enabled","value":"false"},{"component":"totp","key":"mfa-module.enabled","value":"true"}"#,
            "false",
        ),
        ("", "false"),
    ] {
        let document = replaced(
            r#"{"component":"weavelit-server","key":"site-name","value":"Example"}"#,
            &format!(
                r#"{{"component":"weavelit-server","key":"site-name","value":"Example"}}{}{}"#,
                if entries.is_empty() { "" } else { "," },
                entries
            ),
        );

        let backup = normalize(document.as_bytes(), &sqlite(), &components()).unwrap();
        let totp = backup
            .configuration()
            .iter()
            .filter(|entry| {
                entry.component.as_str() == "totp" && entry.key.as_str() == "mfa-module.enabled"
            })
            .collect::<Vec<_>>();

        assert_eq!(totp.len(), 1);
        assert_eq!(totp[0].value.as_str(), expected);
        assert!(!backup.configuration().iter().any(|entry| {
            entry.component.as_str() == "mfa.totp" && entry.key.as_str() == "enabled"
        }));
    }
}

#[test]
fn supplied_audit_references_survive_normalization_exactly() {
    const ACCOUNT_REFERENCE: &str = "ar-11111111111111111111111111111111";
    const GROUP_REFERENCE: &str = "ar-22222222222222222222222222222222";
    const CONFIGURATION_REFERENCE: &str = "ar-44444444444444444444444444444444";
    let document = replaced(
        "\"username\":\"administrator\"",
        &format!("\"audit_reference\":\"{ACCOUNT_REFERENCE}\",\"username\":\"administrator\""),
    );
    let document = document.replace(
        "\"name\":\"Administrators\"",
        &format!("\"audit_reference\":\"{GROUP_REFERENCE}\",\"name\":\"Administrators\""),
    );
    let document = document.replace(
        "\"module\":\"sqlite\"",
        &format!("\"audit_reference\":\"{CONFIGURATION_REFERENCE}\",\"module\":\"sqlite\""),
    );

    let backup = normalize(document.as_bytes(), &sqlite(), &components()).unwrap();

    assert_eq!(
        backup.account_audit_references()[0]
            .audit_reference()
            .to_string(),
        ACCOUNT_REFERENCE
    );
    assert_eq!(
        backup.group_audit_references()[0]
            .audit_reference()
            .to_string(),
        GROUP_REFERENCE
    );
    assert_eq!(
        backup.log_configuration_audit_references()[0]
            .audit_reference()
            .to_string(),
        CONFIGURATION_REFERENCE
    );
}

#[test]
fn malformed_or_reused_supplied_audit_references_are_invalid() {
    let malformed = replaced(
        "\"username\":\"administrator\"",
        "\"audit_reference\":\"ar-00000000000000000000000000000000\",\"username\":\"administrator\"",
    );
    assert_eq!(reject(&malformed), ContentError::DomainInvalid);

    let malformed_configuration = replaced(
        "\"module\":\"sqlite\"",
        "\"audit_reference\":\"ar-00000000000000000000000000000000\",\"module\":\"sqlite\"",
    );
    assert_eq!(
        reject(&malformed_configuration),
        ContentError::DomainInvalid
    );

    const SHARED: &str = "ar-33333333333333333333333333333333";
    let duplicate = replaced(
        "\"username\":\"administrator\"",
        &format!("\"audit_reference\":\"{SHARED}\",\"username\":\"administrator\""),
    );
    let duplicate = duplicate.replace(
        "\"name\":\"Administrators\"",
        &format!("\"audit_reference\":\"{SHARED}\",\"name\":\"Administrators\""),
    );
    assert_eq!(reject(&duplicate), ContentError::DuplicateEntry);

    let cross_kind = replaced(
        "\"username\":\"administrator\"",
        &format!("\"audit_reference\":\"{SHARED}\",\"username\":\"administrator\""),
    );
    let cross_kind = cross_kind.replace(
        "\"module\":\"sqlite\"",
        &format!("\"audit_reference\":\"{SHARED}\",\"module\":\"sqlite\""),
    );
    assert_eq!(reject(&cross_kind), ContentError::DuplicateEntry);
}

#[test]
fn explicit_null_audit_references_are_rejected_while_legacy_omission_is_accepted() {
    normalize(plaintext().as_bytes(), &sqlite(), &components())
        .expect("a legacy omission generates independent references");

    for document in [
        replaced(
            "\"username\":\"administrator\"",
            "\"audit_reference\":null,\"username\":\"administrator\"",
        ),
        replaced(
            "\"name\":\"Administrators\"",
            "\"audit_reference\":null,\"name\":\"Administrators\"",
        ),
        replaced(
            "\"module\":\"sqlite\"",
            "\"audit_reference\":null,\"module\":\"sqlite\"",
        ),
    ] {
        assert_eq!(reject(&document), ContentError::Malformed);
    }
}

#[test]
fn the_content_bounds_are_fixed() {
    assert_eq!(BACKUP_CONTENT_FORMAT_VERSION, 1);
    assert_eq!(MAX_COLLECTION_ENTRIES, 100_000);
    assert_eq!(MAX_LOG_MODULE_SETTINGS, 256);
    assert_eq!(MAX_SENSITIVE_VALUE_BYTES, 32_768);
}

#[test]
fn compatibility_is_an_exact_match_on_version_and_backend() {
    assert_eq!(
        reject(&replaced("\"format_version\":1", "\"format_version\":2")),
        ContentError::UnsupportedFormatVersion
    );
    assert_eq!(
        reject(&replaced(
            "\"source_backend\":\"sqlite\"",
            "\"source_backend\":\"postgresql\""
        )),
        ContentError::BackendMismatch
    );
}

#[test]
fn malformed_or_unknown_content_is_rejected_without_detail() {
    assert_eq!(reject("not json"), ContentError::Malformed);
    assert_eq!(reject("{}"), ContentError::Malformed);
    assert_eq!(
        reject(&replaced(
            "\"accounts\":[",
            "\"unexpected\":1,\"accounts\":["
        )),
        ContentError::Malformed
    );
    assert_eq!(
        reject(&replaced("\"active\":true", "\"active\":true,\"extra\":1")),
        ContentError::Malformed
    );
    assert_eq!(
        reject(&replaced(
            "\"format_version\":1",
            "\"format_version\":1,\"format_version\":1"
        )),
        ContentError::Malformed
    );
}

#[test]
fn backup_content_cannot_carry_session_data() {
    assert_eq!(
        reject(&replaced(
            "\"accounts\":[",
            "\"sessions\":[],\"accounts\":["
        )),
        ContentError::Malformed
    );
    assert_eq!(
        reject(&replaced(
            "\"accounts\":[",
            "\"sessions\":[{\"token_hash\":\"AgMEBQYHCAkKCwwNDg8QEQ\"}],\"accounts\":["
        )),
        ContentError::Malformed
    );
    assert!(
        !plaintext().to_ascii_lowercase().contains("session"),
        "the committed backup fixture must contain no session data"
    );
    assert!(!plaintext().to_ascii_lowercase().contains("csrf"));
}

#[test]
fn a_reference_to_a_missing_account_is_unresolved() {
    // Rewrite only the account record's own identifier so every reference to it
    // becomes unresolved.
    let document = replaced(
        "\"accounts\":[{\"identifier\":\"AgMEBQYHCAkKCwwNDg8QEQ\"",
        "\"accounts\":[{\"identifier\":\"AAAAAAAAAAAAAAAAAAAAAQ\"",
    );
    assert_eq!(reject(&document), ContentError::UnresolvedReference);
}

#[test]
fn a_reference_to_a_missing_group_is_unresolved() {
    let document = replaced(
        "\"groups\":[{\"identifier\":\"AwQFBgcICQoLDA0ODxAREg\"",
        "\"groups\":[{\"identifier\":\"AAAAAAAAAAAAAAAAAAAAAg\"",
    );
    assert_eq!(reject(&document), ContentError::UnresolvedReference);
}

#[test]
fn a_duplicate_entry_is_rejected() {
    let document = replaced(
        "\"configuration\":[{\"component\":\"weavelit-server\",\"key\":\"site-name\",\"value\":\"Example\"}]",
        "\"configuration\":[{\"component\":\"weavelit-server\",\"key\":\"site-name\",\"value\":\"Example\"},{\"component\":\"weavelit-server\",\"key\":\"site-name\",\"value\":\"Other\"}]",
    );
    assert_eq!(reject(&document), ContentError::DuplicateEntry);
}

#[test]
fn a_duplicate_username_is_rejected_when_the_accounts_are_adjacent() {
    let document = accounts(&[
        ("AAAAAAAAAAAAAAAAAAAAAQ", "shared"),
        ("AAAAAAAAAAAAAAAAAAAAAg", "shared"),
    ]);
    assert_eq!(reject(&document), ContentError::DuplicateEntry);
    assert_eq!(
        RestoreError::from(reject(&document)).category_reason(),
        ("backup_invalid", "backup_invalid")
    );
}

#[test]
fn a_duplicate_username_is_rejected_when_the_accounts_are_not_adjacent() {
    // Accounts are ordered by identifier, so the middle account exists only to
    // sort between the two accounts that share a username. The duplicate is
    // therefore reachable only by a check independent of identifier order.
    let document = accounts(&[
        ("AAAAAAAAAAAAAAAAAAAAAQ", "shared"),
        ("AAAAAAAAAAAAAAAAAAAAAg", "between"),
        ("AAAAAAAAAAAAAAAAAAAAAw", "shared"),
    ]);
    assert_eq!(reject(&document), ContentError::DuplicateEntry);
    assert_eq!(
        RestoreError::from(reject(&document)).category_reason(),
        ("backup_invalid", "backup_invalid")
    );
}

#[test]
fn every_log_type_needs_exactly_one_enabled_assignment() {
    let missing = replaced(
        ",{\"log_type\":\"audit\",\"configuration\":\"BgcICQoLDA0ODxAREhMUFQ\"}",
        "",
    );
    assert_eq!(reject(&missing), ContentError::AssignmentInvalid);

    let duplicated = replaced(
        ",{\"log_type\":\"audit\",\"configuration\":\"BgcICQoLDA0ODxAREhMUFQ\"}",
        ",{\"log_type\":\"audit\",\"configuration\":\"BgcICQoLDA0ODxAREhMUFQ\"},{\"log_type\":\"system\",\"configuration\":\"BgcICQoLDA0ODxAREhMUFQ\"}",
    );
    assert_eq!(reject(&duplicated), ContentError::AssignmentInvalid);

    assert_eq!(
        reject(&replaced("\"enabled\":true", "\"enabled\":false")),
        ContentError::AssignmentInvalid
    );
}

#[test]
fn a_component_the_deployment_does_not_offer_is_unavailable() {
    let document = plaintext();
    for missing in [
        AvailableComponents {
            log_modules: Default::default(),
            ..components()
        },
        AvailableComponents {
            mfa_modules: Default::default(),
            ..components()
        },
        AvailableComponents {
            service_modules: Default::default(),
            ..components()
        },
        AvailableComponents {
            client_modules: Default::default(),
            ..components()
        },
    ] {
        assert_eq!(
            normalize(document.as_bytes(), &sqlite(), &missing)
                .expect_err("an unavailable component must be rejected"),
            ContentError::ComponentUnavailable
        );
    }
}

/// Returns the fixture document with the MFA factor carrying `bytes`.
fn with_factor_data(bytes: &[u8]) -> String {
    fn encoded(bytes: &[u8]) -> String {
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    }

    replaced(
        &format!("\"factor_data\":\"{}\"", encoded(&FIXTURE_TOTP_SECRET)),
        &format!("\"factor_data\":\"{}\"", encoded(bytes)),
    )
}

/// Factor data the named MFA Module could not open is an invalid backup.
///
/// The module is compiled in, so this is not a compatibility refusal: the
/// deployment can serve the named module and the backup carries a value that
/// module cannot read. Sealing it would activate a deployment whose account
/// fails every later second-factor attempt, which for the only required
/// Administrator leaves the deployment permanently unreachable.
#[test]
fn factor_data_a_known_mfa_module_cannot_open_is_rejected_as_an_invalid_backup() {
    let declared = components()
        .mfa_factor_format(&Name::new("totp").expect("the module name is valid"))
        .expect("the fixture inventory carries the TOTP Module")
        .factor_data_bytes;

    for length in [declared - 1, declared + 1] {
        let document = with_factor_data(&vec![0x5a; length]);
        assert_eq!(
            reject(&document),
            ContentError::FactorDataInvalid,
            "{length}"
        );
        assert_eq!(
            RestoreError::from(reject(&document)).category_reason(),
            ("backup_invalid", "backup_invalid"),
            "{length}"
        );
    }

    // The same substitution at the declared length still normalizes, so the
    // refusal above is the format rather than the rewritten document.
    normalize(
        with_factor_data(&vec![0x5a; declared]).as_bytes(),
        &sqlite(),
        &components(),
    )
    .expect("factor data of the declared length is valid content");
}

#[test]
fn non_canonical_binary_encoding_is_rejected() {
    for encoded in [
        "cHJvdmlkZXItdG9rZW4=", // padded
        "cHJvdmlkZXI+dG9rZW4",  // standard alphabet
        "cHJvdmlkZXI/dG9rZW4",  // standard alphabet
        "cHJvdmlkZXItdG9rZW5",  // trailing bits set
        "not base64!",
    ] {
        let document = replaced(
            "\"credential\":\"cHJvdmlkZXItdG9rZW4\"",
            &format!("\"credential\":\"{encoded}\""),
        );
        assert_eq!(
            normalize(document.as_bytes(), &sqlite(), &components()).err(),
            Some(ContentError::EncodingInvalid),
            "{encoded}"
        );
    }
}

#[test]
fn an_out_of_domain_value_is_rejected() {
    assert_eq!(
        reject(&replaced(
            "\"username\":\"administrator\"",
            "\"username\":\"\""
        )),
        ContentError::DomainInvalid
    );
    assert_eq!(
        reject(&replaced(
            "\"source_backend\":\"sqlite\"",
            "\"source_backend\":\"SQLite\""
        )),
        ContentError::DomainInvalid
    );
    assert_eq!(
        reject(&replaced(
            "\"verifier\":\"$argon2id",
            "\"verifier\":\"argon2id"
        )),
        ContentError::DomainInvalid
    );
}

/// The encoded verifier the committed fixture carries, at the approved profile.
///
/// Each mutation below changes exactly one encoded field of this string, so the
/// rejection it causes can only come from that field falling outside the closed
/// allowlist rather than from anything else in the fixture.
const APPROVED_VERIFIER: &str = "$argon2id$v=19$m=65536,t=3,p=1$sbKztLW2t7i5uru8vb6/wA$gyw90gCqVs5nwE+ZFbfoD7UW6DPxegGqJR5JSFbObDQ";

/// Encoded verifiers the Server's password decision would never attempt.
///
/// Each is a bounded ASCII PHC-shaped string the Application Database contract
/// accepts on its own, so nothing but the profile allowlist rejects it.
const OFF_PROFILE_VERIFIERS: [&str; 8] = [
    // Memory cost above the approved verification ceiling.
    "$argon2id$v=19$m=1048576,t=3,p=1$sbKztLW2t7i5uru8vb6/wA$gyw90gCqVs5nwE+ZFbfoD7UW6DPxegGqJR5JSFbObDQ",
    // Memory cost below the approved profile.
    "$argon2id$v=19$m=8,t=3,p=1$sbKztLW2t7i5uru8vb6/wA$gyw90gCqVs5nwE+ZFbfoD7UW6DPxegGqJR5JSFbObDQ",
    // Iteration count outside the approved profile.
    "$argon2id$v=19$m=65536,t=1,p=1$sbKztLW2t7i5uru8vb6/wA$gyw90gCqVs5nwE+ZFbfoD7UW6DPxegGqJR5JSFbObDQ",
    // Degree of parallelism outside the approved profile.
    "$argon2id$v=19$m=65536,t=3,p=4$sbKztLW2t7i5uru8vb6/wA$gyw90gCqVs5nwE+ZFbfoD7UW6DPxegGqJR5JSFbObDQ",
    // Another Argon2 variant.
    "$argon2i$v=19$m=65536,t=3,p=1$sbKztLW2t7i5uru8vb6/wA$gyw90gCqVs5nwE+ZFbfoD7UW6DPxegGqJR5JSFbObDQ",
    // An older Argon2 version.
    "$argon2id$v=16$m=65536,t=3,p=1$sbKztLW2t7i5uru8vb6/wA$gyw90gCqVs5nwE+ZFbfoD7UW6DPxegGqJR5JSFbObDQ",
    // No explicit version, which would otherwise default to the library's.
    "$argon2id$m=65536,t=3,p=1$sbKztLW2t7i5uru8vb6/wA$gyw90gCqVs5nwE+ZFbfoD7UW6DPxegGqJR5JSFbObDQ",
    // A salt and an output outside the approved lengths.
    "$argon2id$v=19$m=65536,t=3,p=1$sbKztLW2t7i5uru8vb6/wKur$gyw90gCqVs5nwE+ZFbfoD7UW6DPxegGq",
];

#[test]
fn a_password_verifier_at_the_approved_profile_is_accepted() {
    let backup = normalize(plaintext().as_bytes(), &sqlite(), &components())
        .expect("the committed plaintext carries an accepted verifier");

    let verifiers = backup.password_verifiers();
    assert_eq!(verifiers.len(), 1);
    assert_eq!(verifiers[0].verifier.as_str(), APPROVED_VERIFIER);
}

#[test]
fn a_password_verifier_outside_the_approved_profile_is_rejected() {
    for verifier in OFF_PROFILE_VERIFIERS {
        let document = replaced(APPROVED_VERIFIER, verifier);
        assert_eq!(
            normalize(document.as_bytes(), &sqlite(), &components()).err(),
            Some(ContentError::DomainInvalid),
            "{verifier}"
        );
    }
}

#[test]
fn an_off_profile_password_verifier_is_indistinguishable_from_any_other_invalid_backup() {
    let other = RestoreError::from(reject(&replaced(
        "\"username\":\"administrator\"",
        "\"username\":\"\"",
    )));

    for verifier in OFF_PROFILE_VERIFIERS {
        let rejected = RestoreError::from(reject(&replaced(APPROVED_VERIFIER, verifier)));

        assert_eq!(
            rejected.category_reason(),
            ("backup_invalid", "backup_invalid"),
            "{verifier}"
        );
        assert_eq!(rejected.category_reason(), other.category_reason());
        assert_eq!(rejected.to_string(), other.to_string());
        assert_eq!(format!("{rejected:?}"), format!("{other:?}"), "{verifier}");
    }
}

/// The fixture account that holds Server Administration through its Group.
const FIXTURE_ADMINISTRATOR: &str = "AgMEBQYHCAkKCwwNDg8QEQ";

/// Returns the active accounts holding Server Administration through a Group
/// that carry no password verifier.
fn administrators_without_a_verifier(backup: &NormalizedBackup) -> Vec<&Account> {
    let administering = backup
        .group_grants()
        .iter()
        .filter(|record| record.grant == GroupGrant::ServerAdministration)
        .map(|record| record.group)
        .collect::<Vec<_>>();

    backup
        .accounts()
        .iter()
        .filter(|account| account.active)
        .filter(|account| {
            backup.group_memberships().iter().any(|membership| {
                membership.account == account.identifier
                    && administering.contains(&membership.group)
            })
        })
        .filter(|account| {
            !backup
                .password_verifiers()
                .iter()
                .any(|entry| entry.account == account.identifier)
        })
        .collect()
}

/// Pins that a backup whose only Administrator has no password verifier is
/// valid Restore content, both when the collection is empty and when it is
/// non-empty but omits that account.
///
/// This test records accepted behavior; it is not a missing rejection. The
/// [Technical Specification](../../../../../docs/spec.md) states that if no
/// Administrator can authenticate "the deployment MUST remain inaccessible
/// through supported application interfaces", that "this fail-closed condition
/// is an accepted outcome", and that Restore "MAY reproduce unusable passwords
/// or MFA enrollments and MUST NOT claim to guarantee renewed administrative
/// access". An account with no verifier is a modeled credential state, not
/// invalid content, and a backup may legitimately carry accounts that are
/// intentionally passwordless, disabled, or still pending enrollment.
///
/// Requiring a reachable Administrator to carry a verifier would make Restore
/// assert exactly the continuity guarantee the specification forbids it from
/// claiming. Changing this test therefore requires changing the specification
/// first. It is distinct from
/// [`a_password_verifier_outside_the_approved_profile_is_rejected`], which
/// rejects a *supplied* verifier whose content no policy-conforming Weavelit
/// could have written.
#[test]
fn a_backup_whose_only_administrator_has_no_password_verifier_is_accepted_content() {
    let verifier_entry =
        format!("{{\"account\":\"{FIXTURE_ADMINISTRATOR}\",\"verifier\":\"{APPROVED_VERIFIER}\"}}");

    let empty = replaced(&verifier_entry, "");
    let backup = normalize(empty.as_bytes(), &sqlite(), &components())
        .expect("a backup carrying no password verifier is valid content");
    assert!(backup.password_verifiers().is_empty());
    assert_eq!(administrators_without_a_verifier(&backup).len(), 1);

    // The same acceptance holds when the collection is non-empty and simply
    // omits the only account that can administer the deployment.
    let document = accounts(&[("AAAAAAAAAAAAAAAAAAAAAQ", "operator")]);
    assert!(document.contains(&verifier_entry), "the fixture verifier");
    let document = document.replace(
        &verifier_entry,
        &format!("{{\"account\":\"AAAAAAAAAAAAAAAAAAAAAQ\",\"verifier\":\"{APPROVED_VERIFIER}\"}}"),
    );
    let backup = normalize(document.as_bytes(), &sqlite(), &components())
        .expect("a backup whose Administrator has no password verifier is valid content");
    assert_eq!(backup.password_verifiers().len(), 1);
    let unverified = administrators_without_a_verifier(&backup);
    assert_eq!(unverified.len(), 1);
    assert_eq!(unverified[0].username.as_str(), "administrator");
}

/// Builds one collection body of `count` copies of `entry` plus one surplus
/// entry that cannot deserialize as the collection's wire type.
///
/// The surplus entry is valid JSON, so it can still be consumed and counted
/// without being parsed into the wire model.
fn overflowing(entry: &str, count: usize, surplus: &str) -> String {
    let mut entries = String::with_capacity((entry.len() + 1) * count + surplus.len());
    for _ in 0..count {
        entries.push_str(entry);
        entries.push(',');
    }
    entries.push_str(surplus);
    entries
}

#[test]
fn a_top_level_collection_past_its_limit_is_rejected_without_parsing_the_surplus_entry() {
    const ENTRY: &str = r#"{"component":"weavelit-server","key":"site-name","value":"Example"}"#;

    let document = replaced(
        &format!("\"configuration\":[{ENTRY}]"),
        &format!(
            "\"configuration\":[{}]",
            overflowing(
                ENTRY,
                MAX_COLLECTION_ENTRIES,
                r#"{"component":1,"key":2,"value":3}"#
            )
        ),
    );

    assert_eq!(reject(&document), ContentError::CollectionTooLarge);
    assert_eq!(
        RestoreError::from(reject(&document)).category_reason(),
        ("backup_invalid", "backup_invalid")
    );
}

#[test]
fn a_log_module_settings_collection_past_its_limit_is_rejected_without_parsing_the_surplus_entry() {
    const ENTRY: &str = r#"{"key":"retention-days","value":"30"}"#;

    let document = replaced(
        "\"settings\":[]",
        &format!(
            "\"settings\":[{}]",
            overflowing(ENTRY, MAX_LOG_MODULE_SETTINGS, r#"{"key":1,"value":2}"#)
        ),
    );

    assert_eq!(reject(&document), ContentError::CollectionTooLarge);
    assert_eq!(
        RestoreError::from(reject(&document)).category_reason(),
        ("backup_invalid", "backup_invalid")
    );
}

/// A setting the named Log Module does not accept is an invalid backup.
///
/// The module is compiled in, so this is not a compatibility refusal: the
/// deployment can serve the named module and the backup carries a configuration
/// that module refuses to open. Sealing it would activate a deployment whose
/// System Log destination silently ignores a setting the operator committed, or
/// fails to open at all after the point of no return. The check compares
/// declared keys only, so it reaches this verdict without opening a destination.
#[test]
fn log_module_settings_a_known_module_does_not_accept_are_rejected_as_an_invalid_backup() {
    let document = replaced(
        "\"settings\":[]",
        r#""settings":[{"key":"retention-days","value":"30"}]"#,
    );

    assert_eq!(reject(&document), ContentError::SettingUnsupported);
    assert_eq!(
        RestoreError::from(reject(&document)).category_reason(),
        ("backup_invalid", "backup_invalid")
    );

    // The same document is valid content for a deployment whose `sqlite` Log
    // Module declares that key, so the refusal is the declaration rather than
    // the rewritten document.
    let accepting = AvailableComponents {
        log_modules: [(
            Name::new("sqlite").expect("the name is valid"),
            LogSettingsFormat {
                accepted_keys: ["retention-days".to_owned()].into_iter().collect(),
            },
        )]
        .into_iter()
        .collect(),
        ..components()
    };
    normalize(document.as_bytes(), &sqlite(), &accepting)
        .expect("a declared setting is valid content");
}

#[test]
fn a_wire_string_past_its_domain_bound_is_rejected() {
    let document = replaced(
        "\"username\":\"administrator\"",
        &format!("\"username\":\"{}\"", "a".repeat(MAX_NAME_LENGTH + 1)),
    );
    assert_eq!(reject(&document), ContentError::Malformed);
    assert_eq!(
        RestoreError::from(reject(&document)).category_reason(),
        ("backup_invalid", "backup_invalid")
    );

    let document = replaced(
        "\"credential\":\"cHJvdmlkZXItdG9rZW4\"",
        &format!(
            "\"credential\":\"{}\"",
            "a".repeat(MAX_SENSITIVE_VALUE_BYTES * 2)
        ),
    );
    assert_eq!(reject(&document), ContentError::Malformed);
}

#[test]
fn a_sensitive_value_is_bounded_and_never_rendered() {
    assert_eq!(
        SensitiveBytes::new(Vec::new()),
        Err(ContentError::DomainInvalid)
    );
    assert_eq!(
        SensitiveBytes::new(vec![0; MAX_SENSITIVE_VALUE_BYTES + 1]),
        Err(ContentError::DomainInvalid)
    );

    let value = SensitiveBytes::new(b"provider-token".to_vec()).expect("the value is in bounds");
    assert_eq!(value.expose(), b"provider-token");
    let rendered = format!("{value:?}");
    assert!(rendered.contains("REDACTED"), "{rendered}");
    assert!(!rendered.contains("provider-token"), "{rendered}");
}

#[test]
fn content_failures_render_uniformly() {
    let rendered: Vec<String> = [
        ContentError::PlaintextTooLarge,
        ContentError::Malformed,
        ContentError::UnsupportedFormatVersion,
        ContentError::BackendMismatch,
        ContentError::ComponentUnavailable,
        ContentError::CollectionTooLarge,
        ContentError::DomainInvalid,
        ContentError::EncodingInvalid,
        ContentError::DuplicateEntry,
        ContentError::UnresolvedReference,
        ContentError::AssignmentInvalid,
        ContentError::FactorDataInvalid,
        ContentError::SettingUnsupported,
        ContentError::RandomnessUnavailable,
    ]
    .iter()
    .map(|error| error.to_string())
    .collect();

    assert!(
        rendered.iter().all(|text| text == &rendered[0]),
        "{rendered:?}"
    );
}

#[test]
fn component_names_are_matched_exactly() {
    let alternative = AvailableComponents {
        log_modules: [(
            Name::new("SQLite").expect("the name is valid"),
            LogSettingsFormat::default(),
        )]
        .into_iter()
        .collect(),
        ..components()
    };
    assert_eq!(
        normalize(plaintext().as_bytes(), &sqlite(), &alternative)
            .expect_err("component matching is exact"),
        ContentError::ComponentUnavailable
    );
}

/// The secret-bearing fields changed from `String` to `Zeroizing<String>` in the
/// wire model. These pin the parse and rejection behavior across that change;
/// they observe decoded values and errors, not the wipe itself, which is not
/// observable in safe Rust.
#[test]
fn every_secret_bearing_collection_still_decodes() {
    let backup = normalize(&committed("valid-plaintext.json"), &sqlite(), &components())
        .expect("the committed plaintext is valid content");

    let secret = backup
        .protected_secrets()
        .first()
        .expect("the fixture carries a protected secret");
    assert_eq!(secret.value.expose(), b"at-rest-value");

    let factor = backup
        .mfa_factors()
        .first()
        .expect("the fixture carries an MFA factor");
    assert_eq!(factor.factor_data.expose(), FIXTURE_TOTP_SECRET);

    let connection = backup
        .service_connections()
        .first()
        .expect("the fixture carries a Service Connection");
    assert_eq!(connection.credential.expose(), b"provider-token");
}

#[test]
fn an_invalid_secret_bearing_field_is_rejected_with_an_unchanged_error() {
    for field in [
        "\"value\":\"YXQtcmVzdC12YWx1ZQ\"",
        "\"factor_data\":\"dG90cC1zZWVkLTAxMjM0NTY3ODk\"",
        "\"credential\":\"cHJvdmlkZXItdG9rZW4\"",
    ] {
        let name = field.split('"').nth(1).expect("the field name is quoted");

        let non_canonical = replaced(field, &format!("\"{name}\":\"cHJvdmlkZXI+dG9rZW4\""));
        assert_eq!(
            reject(&non_canonical),
            ContentError::EncodingInvalid,
            "{name}"
        );
        assert_eq!(
            RestoreError::from(reject(&non_canonical)).category_reason(),
            ("backup_invalid", "backup_invalid"),
            "{name}"
        );

        let empty = replaced(field, &format!("\"{name}\":\"\""));
        assert_eq!(reject(&empty), ContentError::DomainInvalid, "{name}");
        assert_eq!(
            RestoreError::from(reject(&empty)).category_reason(),
            ("backup_invalid", "backup_invalid"),
            "{name}"
        );

        let wrong_type = replaced(field, &format!("\"{name}\":1"));
        assert_eq!(reject(&wrong_type), ContentError::Malformed, "{name}");
        assert_eq!(
            RestoreError::from(reject(&wrong_type)).category_reason(),
            ("backup_invalid", "backup_invalid"),
            "{name}"
        );
    }
}
