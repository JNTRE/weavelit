use rusqlite::Connection;
use weavelit_module_log_sqlite::{MODULE_IDENTIFIER, registration};
use weavelit_server_audit::{
    ActionOutcome, AuditActor, AuditEvent, AuditOutcomeDetail, ServerAudit,
};
use weavelit_server_database::{
    AccountAuditReference, AuditReferenceIdentifier, GroupAuditReference, StateIdentifier,
};
use weavelit_server_log::{
    CorrelationId, EventTime, LogModuleCatalog, LogModuleIdentifier, LogRecordType,
    TrustedLogModuleContext, TrustedRecordIssuer,
};
use weavelit_server_log_authority::ServerLogAuthority;

#[test]
fn sqlite_catalog_dispatch_persists_attempt_and_terminal_relationship() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let canonical = temporary_directory.path().canonicalize().unwrap();
    let local_root = canonical.join("configured-log-root");
    std::fs::create_dir(&local_root).unwrap();
    let authority = ServerLogAuthority::new();
    let catalog = LogModuleCatalog::new(vec![registration()]).unwrap();
    let destination = catalog
        .create_destination(
            &LogModuleIdentifier::new(MODULE_IDENTIFIER).unwrap(),
            &TrustedLogModuleContext::from_server_authority(
                &authority,
                local_root.clone(),
                [0x71; 16],
            ),
        )
        .unwrap();
    destination.preflight(LogRecordType::Audit).unwrap();
    let database_path = local_root.join("log.sqlite3");
    assert!(database_path.is_file());
    assert!(!temporary_directory.path().join("log.sqlite3").exists());

    let producer = ServerAudit::new(TrustedRecordIssuer::from_server_authority(&authority));
    let account = AccountAuditReference::new(
        StateIdentifier::from_bytes([0x11; 16]).unwrap(),
        AuditReferenceIdentifier::generate().unwrap(),
    );
    let group = GroupAuditReference::new(
        StateIdentifier::from_bytes([0x22; 16]).unwrap(),
        AuditReferenceIdentifier::generate().unwrap(),
    );
    let attempt = producer
        .prepare_attempt(
            EventTime::from_unix_milliseconds(1_800_000_000_000),
            CorrelationId::new("workflow-correlation-01").unwrap(),
            AuditActor::Human(account),
            AuditEvent::AuthorizationGroupCreated { group },
        )
        .unwrap();
    let attempt_id = *attempt.record().record_id().as_bytes();
    let attempt = attempt.deliver(&destination).unwrap();
    let completion = producer
        .prepare_completion(
            &attempt,
            EventTime::from_unix_milliseconds(1_800_000_000_001),
            AuditOutcomeDetail::AuthorizationGroupCreated(ActionOutcome::Succeeded),
        )
        .unwrap();
    let completion_id = *completion.record().record_id().as_bytes();
    completion.deliver(&destination).unwrap();
    completion.deliver(&destination).unwrap();
    drop(destination);

    let connection = Connection::open(database_path).unwrap();
    let attempt_row: (String, Option<i64>, Option<Vec<u8>>, String) = connection
        .query_row(
            "SELECT phase, result, attempt_record_id, correlation_id FROM weavelit_log_audit_records WHERE record_id = ?1",
            [attempt_id.as_slice()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    let completion_row: (String, Option<i64>, Option<Vec<u8>>, String) = connection
        .query_row(
            "SELECT phase, result, attempt_record_id, correlation_id FROM weavelit_log_audit_records WHERE record_id = ?1",
            [completion_id.as_slice()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    let system_count: i64 = connection
        .query_row(
            "SELECT count(*) FROM weavelit_log_system_records",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let completion_count: i64 = connection
        .query_row(
            "SELECT count(*) FROM weavelit_log_audit_records WHERE record_id = ?1",
            [completion_id.as_slice()],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(
        attempt_row,
        (
            "attempt".into(),
            None,
            None,
            "workflow-correlation-01".into()
        )
    );
    assert_eq!(
        completion_row,
        (
            "completion".into(),
            Some(1),
            Some(attempt_id.to_vec()),
            "workflow-correlation-01".into()
        )
    );
    assert_eq!(completion_count, 1);
    assert_eq!(system_count, 0);
}
