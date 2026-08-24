use std::path::{Path, PathBuf};

use rusqlite::{Connection, params};
use tempfile::TempDir;
use weavelit_server_database::{
    AccountPublicIdentifier, AccountPublicIdentifierPersistence, AuditReferenceIdentifier,
    AuditReferencePersistence, AuditTerminalRecoveryPersistence, AuditTerminalRecoveryStore,
    AuditTerminalReplayBatchSize, Description, Group, GroupAdministrationAuditTerminalWrites,
    GroupAdministrationMutationError, GroupAdministrationStore, GroupAuditReference,
    GroupCreateMutation, GroupCreateOutcome, GroupDeleteMutation, GroupDeleteOutcome, GroupGrant,
    GroupMutationRecheck, GroupPublicIdentifier, GroupPublicIdentifierPersistence,
    GroupPublicIdentity, GroupUpdateMutation, GroupUpdateOutcome, Name,
    SESSION_ABSOLUTE_LIFETIME_MILLISECONDS, SESSION_DIGEST_LENGTH, SessionInstant,
    SessionTokenHash, StateIdentifier, StoredAuditDestinationBinding,
    ValidatedAuditTerminalObligationWrite,
};
use weavelit_server_database_authority::ServerDatabaseAuthority;
use weavelit_server_database_sqlite::SqliteDatabase;

const ACTOR: u8 = 1;
const SESSION: u8 = 0x31;
const NOW: i64 = 1_001;

struct Surface {
    _directory: TempDir,
    path: PathBuf,
    database: SqliteDatabase,
    account_public_ids: AccountPublicIdentifierPersistence,
    public_ids: GroupPublicIdentifierPersistence,
    audit_refs: AuditReferencePersistence,
    recovery: AuditTerminalRecoveryPersistence,
}

struct Terminals {
    succeeded: ValidatedAuditTerminalObligationWrite,
    conflict: ValidatedAuditTerminalObligationWrite,
    denied: ValidatedAuditTerminalObligationWrite,
}

impl Terminals {
    fn writes(&self) -> GroupAdministrationAuditTerminalWrites<'_> {
        GroupAdministrationAuditTerminalWrites::new(&self.succeeded, &self.conflict, &self.denied)
    }
}

fn surface() -> Surface {
    let directory = tempfile::tempdir().unwrap();
    let path = directory
        .path()
        .canonicalize()
        .unwrap()
        .join("application.db");
    let database = SqliteDatabase::open(&path).unwrap();
    let authority = ServerDatabaseAuthority::new();
    let account_public_ids = AccountPublicIdentifierPersistence::from_server_authority(&authority);
    let public_ids = GroupPublicIdentifierPersistence::from_server_authority(&authority);
    let audit_refs = AuditReferencePersistence::from_server_authority(&authority);
    let recovery = AuditTerminalRecoveryPersistence::from_server_authority(&authority);
    insert_actor(&path, &account_public_ids);
    insert_session(&path);
    Surface {
        _directory: directory,
        path,
        database,
        account_public_ids,
        public_ids,
        audit_refs,
        recovery,
    }
}

fn identifier(byte: u8) -> StateIdentifier {
    StateIdentifier::from_bytes([byte; 16]).unwrap()
}

fn public_id(persistence: &GroupPublicIdentifierPersistence, byte: u8) -> GroupPublicIdentifier {
    persistence.decode([byte; 16]).unwrap()
}

fn account_public_id(
    persistence: &AccountPublicIdentifierPersistence,
    byte: u8,
) -> AccountPublicIdentifier {
    persistence.decode([byte; 16]).unwrap()
}

fn audit_ref(persistence: &AuditReferencePersistence, byte: u8) -> AuditReferenceIdentifier {
    persistence
        .decode(&format!("ar-{}", format!("{byte:02x}").repeat(16)))
        .unwrap()
}

fn insert_actor(path: &Path, persistence: &AccountPublicIdentifierPersistence) {
    let connection = Connection::open(path).unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_account \
             (account_id, username, display_name, active, mfa_required) \
             VALUES (?1, 'administrator', NULL, 1, 0)",
            [identifier(ACTOR).as_bytes().as_slice()],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_account_public_identity \
             (account_id, public_identifier) VALUES (?1, ?2)",
            params![
                identifier(ACTOR).as_bytes().as_slice(),
                persistence
                    .encode(&account_public_id(persistence, ACTOR))
                    .as_slice(),
            ],
        )
        .unwrap();
}

fn insert_session(path: &Path) {
    Connection::open(path)
        .unwrap()
        .execute(
            "INSERT INTO weavelit_session \
             (token_hash, csrf_hash, account_id, client_module, issued_at_milliseconds, \
              last_seen_at_milliseconds, absolute_expires_at_milliseconds) \
             VALUES (?1, ?2, ?3, 'web-ui', 1000, 1000, ?4)",
            params![
                [SESSION; SESSION_DIGEST_LENGTH].as_slice(),
                [SESSION + 1; SESSION_DIGEST_LENGTH].as_slice(),
                identifier(ACTOR).as_bytes().as_slice(),
                1_000 + SESSION_ABSOLUTE_LIFETIME_MILLISECONDS,
            ],
        )
        .unwrap();
}

fn insert_group(surface: &Surface, byte: u8, name: &str, description: Option<&str>) {
    let connection = Connection::open(&surface.path).unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_group (group_id, name, description) VALUES (?1, ?2, ?3)",
            params![identifier(byte).as_bytes().as_slice(), name, description],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_group_public_identity \
             (group_id, public_identifier) VALUES (?1, ?2)",
            params![
                identifier(byte).as_bytes().as_slice(),
                surface
                    .public_ids
                    .encode(&public_id(&surface.public_ids, byte))
                    .as_slice(),
            ],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_group_audit_reference \
             (group_id, audit_reference) VALUES (?1, ?2)",
            params![
                identifier(byte).as_bytes().as_slice(),
                audit_ref(&surface.audit_refs, byte + 0x40).to_string(),
            ],
        )
        .unwrap();
}

fn recheck() -> GroupMutationRecheck {
    GroupMutationRecheck::new(
        identifier(ACTOR),
        SessionTokenHash::from_bytes([SESSION; SESSION_DIGEST_LENGTH]).unwrap(),
        Name::new("web-ui").unwrap(),
        SessionInstant::from_unix_milliseconds(NOW).unwrap(),
    )
}

fn terminal(
    persistence: &AuditTerminalRecoveryPersistence,
    byte: u8,
) -> ValidatedAuditTerminalObligationWrite {
    let binding =
        StoredAuditDestinationBinding::from_persisted(persistence, [0x71; 16], 1).unwrap();
    ValidatedAuditTerminalObligationWrite::from_server_audit(
        persistence,
        [byte; 16],
        vec![byte; 32],
        binding,
    )
    .unwrap()
}

fn terminals(persistence: &AuditTerminalRecoveryPersistence, base: u8) -> Terminals {
    Terminals {
        succeeded: terminal(persistence, base),
        conflict: terminal(persistence, base + 1),
        denied: terminal(persistence, base + 2),
    }
}

fn pending(surface: &mut Surface) -> Vec<[u8; 16]> {
    surface
        .database
        .list_pending_audit_terminal_obligations(
            &surface.recovery,
            AuditTerminalReplayBatchSize::new(16).unwrap(),
        )
        .unwrap()
        .into_iter()
        .map(|value| *value.identifier().as_bytes())
        .collect()
}

fn target(surface: &mut Surface, byte: u8) -> weavelit_server_database::GroupAdministrationTarget {
    surface
        .database
        .prepare_group_administration_target(
            &surface.public_ids,
            &surface.audit_refs,
            public_id(&surface.public_ids, byte),
        )
        .unwrap()
        .unwrap()
}

#[test]
fn group_reads_are_ordered_exact_persistent_and_fail_closed_for_identity_damage() {
    let mut surface = surface();
    insert_group(&surface, 0x11, "Zulu", None);
    insert_group(&surface, 0x12, "Alpha", Some("Operators"));

    let listed = surface
        .database
        .list_group_administration_projections(&surface.public_ids)
        .unwrap();
    assert_eq!(
        listed
            .iter()
            .map(|value| value.name().as_str())
            .collect::<Vec<_>>(),
        ["Alpha", "Zulu"]
    );
    assert_eq!(listed[0].description().unwrap().as_str(), "Operators");
    let exact = surface
        .database
        .load_group_administration_projection(
            &surface.public_ids,
            public_id(&surface.public_ids, 0x11),
        )
        .unwrap()
        .unwrap();
    assert_eq!(exact.name().as_str(), "Zulu");
    assert_eq!(exact.description(), None);
    assert!(
        surface
            .database
            .load_group_administration_projection(
                &surface.public_ids,
                public_id(&surface.public_ids, 0x19),
            )
            .unwrap()
            .is_none()
    );

    drop(surface.database);
    surface.database = SqliteDatabase::open(&surface.path).unwrap();
    assert_eq!(
        surface
            .database
            .list_group_administration_projections(&surface.public_ids)
            .unwrap()
            .len(),
        2
    );

    let connection = Connection::open(&surface.path).unwrap();
    connection
        .execute_batch(
            "DROP TRIGGER weavelit_group_public_identity_reject_direct_delete; \
             DELETE FROM weavelit_group_public_identity \
             WHERE group_id = x'11111111111111111111111111111111';",
        )
        .unwrap();
    assert!(
        surface
            .database
            .list_group_administration_projections(&surface.public_ids)
            .is_err()
    );
}

#[test]
fn association_reads_are_safe_ordered_persistent_and_distinguish_missing_groups() {
    let mut surface = surface();
    insert_group(&surface, 0x11, "Operators", None);
    let connection = Connection::open(&surface.path).unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_group_membership (group_id, account_id) VALUES (?1, ?2)",
            params![
                identifier(0x11).as_bytes().as_slice(),
                identifier(ACTOR).as_bytes().as_slice(),
            ],
        )
        .unwrap();
    for (kind, value) in [("server_administration", ""), ("client_module", "web-ui")] {
        connection
            .execute(
                "INSERT INTO weavelit_group_grant (group_id, grant_kind, grant_value) \
                 VALUES (?1, ?2, ?3)",
                params![identifier(0x11).as_bytes().as_slice(), kind, value],
            )
            .unwrap();
    }

    let members = surface
        .database
        .list_group_member_administration_projections(
            &surface.account_public_ids,
            &surface.public_ids,
            public_id(&surface.public_ids, 0x11),
        )
        .unwrap()
        .unwrap();
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].username().as_str(), "administrator");
    assert_eq!(
        members[0].public_identifier(),
        account_public_id(&surface.account_public_ids, ACTOR)
    );
    let grants = surface
        .database
        .list_group_grant_administration_projections(
            &surface.public_ids,
            public_id(&surface.public_ids, 0x11),
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        grants,
        [
            GroupGrant::ClientModule(Name::new("web-ui").unwrap()),
            GroupGrant::ServerAdministration,
        ]
    );
    assert!(
        surface
            .database
            .list_group_member_administration_projections(
                &surface.account_public_ids,
                &surface.public_ids,
                public_id(&surface.public_ids, 0x19),
            )
            .unwrap()
            .is_none()
    );

    drop(surface.database);
    surface.database = SqliteDatabase::open(&surface.path).unwrap();
    assert_eq!(
        surface
            .database
            .list_group_grant_administration_projections(
                &surface.public_ids,
                public_id(&surface.public_ids, 0x11),
            )
            .unwrap()
            .unwrap(),
        grants
    );
}

#[test]
fn group_create_commits_empty_group_or_conflict_with_one_terminal() {
    let mut surface = surface();
    let group = Group {
        identifier: identifier(0x21),
        name: Name::new("Operators").unwrap(),
        description: Some(Description::new("Operational staff").unwrap()),
    };
    let mutation = GroupCreateMutation::new(
        recheck(),
        group,
        GroupPublicIdentity::new(identifier(0x21), public_id(&surface.public_ids, 0x21)),
        GroupAuditReference::new(identifier(0x21), audit_ref(&surface.audit_refs, 0x61)),
    )
    .unwrap();
    let success = terminals(&surface.recovery, 0x11);
    assert_eq!(
        surface
            .database
            .create_group(&surface.public_ids, &mutation, &success.writes())
            .unwrap(),
        GroupCreateOutcome::Created
    );
    assert_eq!(pending(&mut surface), vec![[0x11; 16]]);
    assert_eq!(
        Connection::open(&surface.path)
            .unwrap()
            .query_row(
                "SELECT count(*) FROM weavelit_group_membership \
                 WHERE group_id = x'21212121212121212121212121212121'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );

    let conflict = GroupCreateMutation::new(
        recheck(),
        Group {
            identifier: identifier(0x22),
            name: Name::new("Operators").unwrap(),
            description: None,
        },
        GroupPublicIdentity::new(identifier(0x22), public_id(&surface.public_ids, 0x22)),
        GroupAuditReference::new(identifier(0x22), audit_ref(&surface.audit_refs, 0x62)),
    )
    .unwrap();
    let conflict_terminals = terminals(&surface.recovery, 0x21);
    assert_eq!(
        surface
            .database
            .create_group(&surface.public_ids, &conflict, &conflict_terminals.writes())
            .unwrap(),
        GroupCreateOutcome::Conflict
    );
    assert_eq!(pending(&mut surface), vec![[0x11; 16], [0x22; 16]]);
}

#[test]
fn group_update_rejects_noop_conflict_and_stale_target() {
    let mut surface = surface();
    insert_group(&surface, 0x11, "Operators", Some("Old"));
    insert_group(&surface, 0x12, "Auditors", None);
    assert_eq!(
        GroupUpdateMutation::new(
            recheck(),
            target(&mut surface, 0x11),
            Name::new("Operators").unwrap(),
            Some(Description::new("Old").unwrap()),
        )
        .unwrap_err(),
        GroupAdministrationMutationError::Unchanged
    );

    let update = GroupUpdateMutation::new(
        recheck(),
        target(&mut surface, 0x11),
        Name::new("Support").unwrap(),
        None,
    )
    .unwrap();
    let success = terminals(&surface.recovery, 0x31);
    assert_eq!(
        surface
            .database
            .update_group(
                &surface.public_ids,
                &surface.audit_refs,
                &update,
                &success.writes(),
            )
            .unwrap(),
        GroupUpdateOutcome::Changed
    );

    let conflict = GroupUpdateMutation::new(
        recheck(),
        target(&mut surface, 0x11),
        Name::new("Auditors").unwrap(),
        None,
    )
    .unwrap();
    let conflict_terminals = terminals(&surface.recovery, 0x41);
    assert_eq!(
        surface
            .database
            .update_group(
                &surface.public_ids,
                &surface.audit_refs,
                &conflict,
                &conflict_terminals.writes(),
            )
            .unwrap(),
        GroupUpdateOutcome::Conflict
    );

    let stale = GroupUpdateMutation::new(
        recheck(),
        target(&mut surface, 0x11),
        Name::new("Escalations").unwrap(),
        None,
    )
    .unwrap();
    Connection::open(&surface.path)
        .unwrap()
        .execute(
            "UPDATE weavelit_group SET description = 'drift' \
             WHERE group_id = x'11111111111111111111111111111111'",
            [],
        )
        .unwrap();
    let stale_terminals = terminals(&surface.recovery, 0x51);
    assert_eq!(
        surface
            .database
            .update_group(
                &surface.public_ids,
                &surface.audit_refs,
                &stale,
                &stale_terminals.writes(),
            )
            .unwrap(),
        GroupUpdateOutcome::Stale
    );
}

#[test]
fn group_delete_requires_empty_current_target_and_rolls_back_on_terminal_failure() {
    let mut surface = surface();
    insert_group(&surface, 0x11, "Operators", None);
    Connection::open(&surface.path)
        .unwrap()
        .execute(
            "INSERT INTO weavelit_group_grant (group_id, grant_kind, grant_value) \
             VALUES (x'11111111111111111111111111111111', 'server_administration', '')",
            [],
        )
        .unwrap();
    let deletion = GroupDeleteMutation::new(recheck(), target(&mut surface, 0x11));
    let nonempty = terminals(&surface.recovery, 0x61);
    assert_eq!(
        surface
            .database
            .delete_group(
                &surface.public_ids,
                &surface.audit_refs,
                &deletion,
                &nonempty.writes(),
            )
            .unwrap(),
        GroupDeleteOutcome::Nonempty
    );
    Connection::open(&surface.path)
        .unwrap()
        .execute("DELETE FROM weavelit_group_grant", [])
        .unwrap();

    let failed = terminals(&surface.recovery, 0x71);
    Connection::open(&surface.path)
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER reject_group_crud_terminal BEFORE INSERT \
             ON weavelit_audit_terminal_obligation BEGIN SELECT RAISE(ABORT, 'rejected'); END;",
        )
        .unwrap();
    assert!(
        surface
            .database
            .delete_group(
                &surface.public_ids,
                &surface.audit_refs,
                &deletion,
                &failed.writes(),
            )
            .is_err()
    );
    assert!(target(&mut surface, 0x11).projection().name().as_str() == "Operators");
    Connection::open(&surface.path)
        .unwrap()
        .execute("DROP TRIGGER reject_group_crud_terminal", [])
        .unwrap();

    let success = terminals(&surface.recovery, 0x81);
    assert_eq!(
        surface
            .database
            .delete_group(
                &surface.public_ids,
                &surface.audit_refs,
                &deletion,
                &success.writes(),
            )
            .unwrap(),
        GroupDeleteOutcome::Deleted
    );
    assert!(
        surface
            .database
            .load_group_administration_projection(
                &surface.public_ids,
                public_id(&surface.public_ids, 0x11),
            )
            .unwrap()
            .is_none()
    );
}

#[test]
fn final_issuer_denial_changes_no_group_state() {
    let mut surface = surface();
    insert_group(&surface, 0x11, "Operators", None);
    let update = GroupUpdateMutation::new(
        recheck(),
        target(&mut surface, 0x11),
        Name::new("Support").unwrap(),
        None,
    )
    .unwrap();
    Connection::open(&surface.path)
        .unwrap()
        .execute("DELETE FROM weavelit_session", [])
        .unwrap();
    let denied = terminals(&surface.recovery, 0x91);
    assert_eq!(
        surface
            .database
            .update_group(
                &surface.public_ids,
                &surface.audit_refs,
                &update,
                &denied.writes(),
            )
            .unwrap(),
        GroupUpdateOutcome::Denied
    );
    assert_eq!(
        surface
            .database
            .load_group_administration_projection(
                &surface.public_ids,
                public_id(&surface.public_ids, 0x11),
            )
            .unwrap()
            .unwrap()
            .name()
            .as_str(),
        "Operators"
    );
    assert_eq!(pending(&mut surface), vec![[0x93; 16]]);
}
