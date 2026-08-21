use std::path::{Path, PathBuf};

use rusqlite::{Connection, params};
use tempfile::TempDir;
use weavelit_server_database::{
    AccountPublicIdentifier, AccountPublicIdentifierPersistence, AuditReferenceIdentifier,
    AuditReferencePersistence, AuditTerminalRecoveryPersistence, AuditTerminalRecoveryStore,
    AuditTerminalReplayBatchSize, GroupGrant, GroupMutationAuditTerminalWrites, GroupMutationError,
    GroupMutationOutcome, GroupMutationRecheck, GroupMutationStore, GroupMutationTarget, Name,
    PreparedGroupMutation, SESSION_ABSOLUTE_LIFETIME_MILLISECONDS, SESSION_DIGEST_LENGTH,
    SessionInstant, SessionTokenHash, StateIdentifier, StoredAuditDestinationBinding,
    ValidatedAuditTerminalObligationWrite,
};
use weavelit_server_database_authority::ServerDatabaseAuthority;
use weavelit_server_database_sqlite::SqliteDatabase;

const ACTOR: u8 = 1;
const TARGET: u8 = 2;
const OTHER_ADMIN: u8 = 3;
const GROUP: u8 = 0x11;
const OTHER_GROUP: u8 = 0x12;
const ACTOR_SESSION: u8 = 0x31;
const NOW: i64 = 1_001;

struct Surface {
    _directory: TempDir,
    path: PathBuf,
    database: SqliteDatabase,
    public_identifiers: AccountPublicIdentifierPersistence,
    audit_references: AuditReferencePersistence,
    recovery: AuditTerminalRecoveryPersistence,
}

struct Terminals {
    succeeded: ValidatedAuditTerminalObligationWrite,
    denied: ValidatedAuditTerminalObligationWrite,
    last_administrator_denied: ValidatedAuditTerminalObligationWrite,
}

impl Terminals {
    fn writes(&self) -> GroupMutationAuditTerminalWrites<'_> {
        GroupMutationAuditTerminalWrites::new(
            &self.succeeded,
            &self.denied,
            &self.last_administrator_denied,
        )
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
    let public_identifiers = AccountPublicIdentifierPersistence::from_server_authority(&authority);
    let audit_references = AuditReferencePersistence::from_server_authority(&authority);
    let recovery = AuditTerminalRecoveryPersistence::from_server_authority(&authority);
    for account in [ACTOR, TARGET, OTHER_ADMIN] {
        insert_account(&path, &public_identifiers, &audit_references, account, true);
    }
    for group in [GROUP, OTHER_GROUP] {
        insert_group(&path, &audit_references, group);
    }
    insert_session(&path, ACTOR_SESSION, ACTOR, "web-ui", 1_000);
    Surface {
        _directory: directory,
        path,
        database,
        public_identifiers,
        audit_references,
        recovery,
    }
}

fn identifier(byte: u8) -> StateIdentifier {
    StateIdentifier::from_bytes([byte; 16]).unwrap()
}

fn public_identifier(
    persistence: &AccountPublicIdentifierPersistence,
    account: u8,
) -> AccountPublicIdentifier {
    persistence
        .decode([account.wrapping_add(0x20); 16])
        .unwrap()
}

fn audit_reference(persistence: &AuditReferencePersistence, byte: u8) -> AuditReferenceIdentifier {
    persistence
        .decode(&format!("ar-{}", format!("{byte:02x}").repeat(16)))
        .unwrap()
}

fn insert_account(
    path: &Path,
    public_identifiers: &AccountPublicIdentifierPersistence,
    audit_references: &AuditReferencePersistence,
    account: u8,
    active: bool,
) {
    let connection = Connection::open(path).unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_account \
             (account_id, username, display_name, active, mfa_required) \
             VALUES (?1, ?2, NULL, ?3, 0)",
            params![
                identifier(account).as_bytes().as_slice(),
                format!("user-{account}"),
                i64::from(active)
            ],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_account_public_identity \
             (account_id, public_identifier) VALUES (?1, ?2)",
            params![
                identifier(account).as_bytes().as_slice(),
                public_identifiers
                    .encode(&public_identifier(public_identifiers, account))
                    .as_slice()
            ],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_account_audit_reference \
             (account_id, audit_reference) VALUES (?1, ?2)",
            params![
                identifier(account).as_bytes().as_slice(),
                audit_reference(audit_references, account.wrapping_add(0x40)).to_string()
            ],
        )
        .unwrap();
}

fn insert_group(path: &Path, audit_references: &AuditReferencePersistence, group: u8) {
    let connection = Connection::open(path).unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_group (group_id, name, description) VALUES (?1, ?2, NULL)",
            params![
                identifier(group).as_bytes().as_slice(),
                format!("group-{group}")
            ],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_group_audit_reference \
             (group_id, audit_reference) VALUES (?1, ?2)",
            params![
                identifier(group).as_bytes().as_slice(),
                audit_reference(audit_references, group.wrapping_add(0x60)).to_string()
            ],
        )
        .unwrap();
}

fn insert_session(path: &Path, token: u8, account: u8, client_module: &str, issued: i64) {
    Connection::open(path)
        .unwrap()
        .execute(
            "INSERT INTO weavelit_session \
             (token_hash, csrf_hash, account_id, client_module, issued_at_milliseconds, \
              last_seen_at_milliseconds, absolute_expires_at_milliseconds) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6)",
            params![
                [token; SESSION_DIGEST_LENGTH].as_slice(),
                [token.wrapping_add(1); SESSION_DIGEST_LENGTH].as_slice(),
                identifier(account).as_bytes().as_slice(),
                client_module,
                issued,
                issued + SESSION_ABSOLUTE_LIFETIME_MILLISECONDS
            ],
        )
        .unwrap();
}

fn set_membership(path: &Path, group: u8, account: u8, present: bool) {
    let connection = Connection::open(path).unwrap();
    let sql = if present {
        "INSERT OR IGNORE INTO weavelit_group_membership (group_id, account_id) VALUES (?1, ?2)"
    } else {
        "DELETE FROM weavelit_group_membership WHERE group_id = ?1 AND account_id = ?2"
    };
    connection
        .execute(
            sql,
            params![
                identifier(group).as_bytes().as_slice(),
                identifier(account).as_bytes().as_slice()
            ],
        )
        .unwrap();
}

fn set_grant(path: &Path, group: u8, grant: &GroupGrant, present: bool) {
    let connection = Connection::open(path).unwrap();
    let (kind, value) = match grant {
        GroupGrant::ClientModule(name) => ("client_module", name.as_str()),
        GroupGrant::ServiceModule(name) => ("service_module", name.as_str()),
        GroupGrant::Operation(name) => ("operation", name.as_str()),
        GroupGrant::ServerAdministration => ("server_administration", ""),
    };
    let sql = if present {
        "INSERT OR IGNORE INTO weavelit_group_grant \
         (group_id, grant_kind, grant_value) VALUES (?1, ?2, ?3)"
    } else {
        "DELETE FROM weavelit_group_grant \
         WHERE group_id = ?1 AND grant_kind = ?2 AND grant_value = ?3"
    };
    connection
        .execute(
            sql,
            params![identifier(group).as_bytes().as_slice(), kind, value],
        )
        .unwrap();
}

fn association_count(path: &Path, table: &str) -> i64 {
    Connection::open(path)
        .unwrap()
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
}

fn recheck() -> GroupMutationRecheck {
    GroupMutationRecheck::new(
        identifier(ACTOR),
        SessionTokenHash::from_bytes([ACTOR_SESSION; SESSION_DIGEST_LENGTH]).unwrap(),
        Name::new("web-ui").unwrap(),
        SessionInstant::from_unix_milliseconds(NOW).unwrap(),
    )
}

fn membership_mutation(
    surface: &mut Surface,
    group: u8,
    account: u8,
    desired: bool,
) -> Result<PreparedGroupMutation, GroupMutationError> {
    let target = surface
        .database
        .prepare_group_membership_target(
            &surface.public_identifiers,
            &surface.audit_references,
            identifier(group),
            public_identifier(&surface.public_identifiers, account),
        )
        .unwrap()
        .unwrap();
    PreparedGroupMutation::new(recheck(), GroupMutationTarget::Membership(target), desired)
}

fn grant_mutation(
    surface: &mut Surface,
    group: u8,
    grant: GroupGrant,
    desired: bool,
) -> Result<PreparedGroupMutation, GroupMutationError> {
    let target = surface
        .database
        .prepare_group_grant_target(&surface.audit_references, identifier(group), grant)
        .unwrap()
        .unwrap();
    PreparedGroupMutation::new(recheck(), GroupMutationTarget::Grant(target), desired)
}

fn terminal(
    persistence: &AuditTerminalRecoveryPersistence,
    identifier: u8,
) -> ValidatedAuditTerminalObligationWrite {
    let binding =
        StoredAuditDestinationBinding::from_persisted(persistence, [0x71; 16], 1).unwrap();
    ValidatedAuditTerminalObligationWrite::from_server_audit(
        persistence,
        [identifier; 16],
        vec![identifier; 32],
        binding,
    )
    .unwrap()
}

fn terminals(persistence: &AuditTerminalRecoveryPersistence, base: u8) -> Terminals {
    Terminals {
        succeeded: terminal(persistence, base),
        denied: terminal(persistence, base + 1),
        last_administrator_denied: terminal(persistence, base + 2),
    }
}

fn pending_identifiers(surface: &mut Surface) -> Vec<[u8; 16]> {
    surface
        .database
        .list_pending_audit_terminal_obligations(
            &surface.recovery,
            AuditTerminalReplayBatchSize::new(16).unwrap(),
        )
        .unwrap()
        .into_iter()
        .map(|obligation| *obligation.identifier().as_bytes())
        .collect()
}

fn commit(
    surface: &mut Surface,
    mutation: &PreparedGroupMutation,
    terminals: &Terminals,
) -> Result<GroupMutationOutcome, weavelit_server_database::DatabaseError> {
    surface.database.commit_group_mutation(
        &surface.public_identifiers,
        &surface.audit_references,
        mutation,
        &terminals.writes(),
    )
}

#[test]
fn group_mutation_membership_changes_one_row_and_detects_noop_and_drift() {
    let mut surface = surface();
    let add = membership_mutation(&mut surface, GROUP, TARGET, true).unwrap();
    let first_terminals = terminals(&surface.recovery, 0x21);
    assert_eq!(
        commit(&mut surface, &add, &first_terminals).unwrap(),
        GroupMutationOutcome::Changed
    );
    assert_eq!(
        association_count(&surface.path, "weavelit_group_membership"),
        1
    );
    assert_eq!(pending_identifiers(&mut surface), vec![[0x21; 16]]);
    assert_eq!(
        membership_mutation(&mut surface, GROUP, TARGET, true).unwrap_err(),
        GroupMutationError::Unchanged
    );

    let stale = membership_mutation(&mut surface, OTHER_GROUP, TARGET, true).unwrap();
    set_membership(&surface.path, OTHER_GROUP, TARGET, true);
    let stale_terminals = terminals(&surface.recovery, 0x31);
    assert_eq!(
        commit(&mut surface, &stale, &stale_terminals).unwrap(),
        GroupMutationOutcome::Stale
    );
    assert_eq!(
        pending_identifiers(&mut surface),
        vec![[0x21; 16], [0x32; 16]]
    );
}

#[test]
fn group_mutation_membership_removal_preserves_one_active_effective_administrator() {
    let mut surface = surface();
    set_membership(&surface.path, GROUP, TARGET, true);
    set_grant(
        &surface.path,
        GROUP,
        &GroupGrant::ServerAdministration,
        true,
    );
    let remove = membership_mutation(&mut surface, GROUP, TARGET, false).unwrap();
    let denied_terminals = terminals(&surface.recovery, 0x41);
    assert_eq!(
        commit(&mut surface, &remove, &denied_terminals).unwrap(),
        GroupMutationOutcome::LastAdministratorDenied
    );
    assert_eq!(
        association_count(&surface.path, "weavelit_group_membership"),
        1
    );
    assert_eq!(pending_identifiers(&mut surface), vec![[0x43; 16]]);

    set_membership(&surface.path, OTHER_GROUP, OTHER_ADMIN, true);
    set_grant(
        &surface.path,
        OTHER_GROUP,
        &GroupGrant::ServerAdministration,
        true,
    );
    let allowed_terminals = terminals(&surface.recovery, 0x51);
    assert_eq!(
        commit(&mut surface, &remove, &allowed_terminals).unwrap(),
        GroupMutationOutcome::Changed
    );
    assert_eq!(
        association_count(&surface.path, "weavelit_group_membership"),
        1
    );
    assert_eq!(
        pending_identifiers(&mut surface),
        vec![[0x43; 16], [0x51; 16]]
    );
}

#[test]
fn group_mutation_grant_removal_uses_the_same_effective_last_administrator_guard() {
    let mut surface = surface();
    set_membership(&surface.path, GROUP, TARGET, true);
    set_grant(
        &surface.path,
        GROUP,
        &GroupGrant::ServerAdministration,
        true,
    );
    let remove =
        grant_mutation(&mut surface, GROUP, GroupGrant::ServerAdministration, false).unwrap();
    let denied_terminals = terminals(&surface.recovery, 0x61);
    assert_eq!(
        commit(&mut surface, &remove, &denied_terminals).unwrap(),
        GroupMutationOutcome::LastAdministratorDenied
    );
    assert_eq!(association_count(&surface.path, "weavelit_group_grant"), 1);
    assert_eq!(pending_identifiers(&mut surface), vec![[0x63; 16]]);

    set_membership(&surface.path, OTHER_GROUP, OTHER_ADMIN, true);
    set_grant(
        &surface.path,
        OTHER_GROUP,
        &GroupGrant::ServerAdministration,
        true,
    );
    let allowed_terminals = terminals(&surface.recovery, 0x71);
    assert_eq!(
        commit(&mut surface, &remove, &allowed_terminals).unwrap(),
        GroupMutationOutcome::Changed
    );
    assert_eq!(association_count(&surface.path, "weavelit_group_grant"), 1);
}

#[test]
fn group_mutation_issuer_denial_and_terminal_failure_change_no_business_row() {
    let mut denied = surface();
    let add = grant_mutation(
        &mut denied,
        GROUP,
        GroupGrant::Operation(Name::new("ticket.read").unwrap()),
        true,
    )
    .unwrap();
    Connection::open(&denied.path)
        .unwrap()
        .execute("DELETE FROM weavelit_session", [])
        .unwrap();
    let denied_terminals = terminals(&denied.recovery, 0x21);
    assert_eq!(
        commit(&mut denied, &add, &denied_terminals).unwrap(),
        GroupMutationOutcome::Denied
    );
    assert_eq!(association_count(&denied.path, "weavelit_group_grant"), 0);
    assert_eq!(pending_identifiers(&mut denied), vec![[0x22; 16]]);

    let mut failed = surface();
    let add = grant_mutation(
        &mut failed,
        GROUP,
        GroupGrant::ClientModule(Name::new("web-ui").unwrap()),
        true,
    )
    .unwrap();
    Connection::open(&failed.path)
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER reject_group_terminal BEFORE INSERT \
             ON weavelit_audit_terminal_obligation BEGIN SELECT RAISE(ABORT, 'rejected'); END;",
        )
        .unwrap();
    let failed_terminals = terminals(&failed.recovery, 0x31);
    assert!(commit(&mut failed, &add, &failed_terminals).is_err());
    assert_eq!(association_count(&failed.path, "weavelit_group_grant"), 0);
    assert!(pending_identifiers(&mut failed).is_empty());
}
