//! Compatibility, reference, component, and domain checks on backup content.

mod support;

use support::{committed, committed_text, components};
use weavelit_server_restore::{
    AvailableComponents, BACKUP_CONTENT_FORMAT_VERSION, BackendIdentifier, ContentError,
    MAX_COLLECTION_ENTRIES, MAX_LOG_MODULE_SETTINGS, MAX_SENSITIVE_VALUE_BYTES, Name, RestoreError,
    SensitiveBytes, normalize,
};

fn sqlite() -> BackendIdentifier {
    BackendIdentifier::new("sqlite").expect("the backend identifier is valid")
}

fn plaintext() -> String {
    committed_text("valid-plaintext.json")
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
    normalize(&committed("valid-plaintext.json"), &sqlite(), &components())
        .expect("the committed plaintext is valid content");
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
        log_modules: [Name::new("SQLite").expect("the name is valid")]
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
    assert_eq!(factor.factor_data.expose(), b"totp-seed");

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
        "\"factor_data\":\"dG90cC1zZWVk\"",
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
