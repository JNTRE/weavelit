//! Compatibility, reference, component, and domain checks on backup content.

mod support;

use support::{committed, committed_text, components};
use weavelit_server_restore::{
    AvailableComponents, BACKUP_CONTENT_FORMAT_VERSION, BackendIdentifier, ContentError,
    MAX_COLLECTION_ENTRIES, MAX_LOG_MODULE_SETTINGS, MAX_SENSITIVE_VALUE_BYTES, Name,
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
