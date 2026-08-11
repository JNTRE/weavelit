use std::path::{Path, PathBuf};

use rusqlite::Connection;
use tempfile::TempDir;
use weavelit_server_database::{
    ApplicationDatabase, MfaAcceptance, MfaStore, MfaTimeStep, StateIdentifier,
};
use weavelit_server_database_sqlite::SqliteDatabase;

fn database_path(temporary_directory: &TempDir) -> PathBuf {
    temporary_directory
        .path()
        .canonicalize()
        .unwrap()
        .join("application.db")
}

fn factor(byte: u8) -> StateIdentifier {
    StateIdentifier::from_bytes([byte; 16]).unwrap()
}

fn step(value: u64) -> MfaTimeStep {
    MfaTimeStep::from_step(value).unwrap()
}

fn stored_step(path: &Path, factor: StateIdentifier) -> Option<i64> {
    Connection::open(path)
        .unwrap()
        .query_row(
            "SELECT accepted_step FROM weavelit_mfa_replay_watermark WHERE factor_id = ?1",
            [factor.as_bytes().as_slice()],
            |row| row.get(0),
        )
        .ok()
}

#[test]
fn a_factor_that_has_accepted_nothing_has_no_watermark() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let mut database = SqliteDatabase::open(&database_path(&temporary_directory)).unwrap();

    assert_eq!(database.accepted_step(factor(1)).unwrap(), None);
}

#[test]
fn the_first_step_is_accepted_and_becomes_the_watermark() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let mut database = SqliteDatabase::open(&path).unwrap();

    let acceptance = database.accept_step(factor(1), step(41_152_263)).unwrap();

    assert_eq!(acceptance, MfaAcceptance::Accepted);
    assert_eq!(
        database.accepted_step(factor(1)).unwrap(),
        Some(step(41_152_263))
    );
    assert_eq!(stored_step(&path, factor(1)), Some(41_152_263));
}

#[test]
fn a_step_that_does_not_advance_the_watermark_is_refused_as_a_replay() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let mut database = SqliteDatabase::open(&path).unwrap();
    database.accept_step(factor(1), step(41_152_263)).unwrap();

    for presented in [41_152_263, 41_152_262, 0] {
        assert_eq!(
            database.accept_step(factor(1), step(presented)).unwrap(),
            MfaAcceptance::Replayed
        );
        assert_eq!(stored_step(&path, factor(1)), Some(41_152_263));
    }
}

#[test]
fn only_a_strictly_later_step_advances_the_watermark() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let mut database = SqliteDatabase::open(&path).unwrap();
    database.accept_step(factor(1), step(41_152_263)).unwrap();

    let acceptance = database.accept_step(factor(1), step(41_152_264)).unwrap();

    assert_eq!(acceptance, MfaAcceptance::Accepted);
    assert_eq!(
        database.accepted_step(factor(1)).unwrap(),
        Some(step(41_152_264))
    );
}

#[test]
fn each_factor_keeps_its_own_watermark() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let mut database = SqliteDatabase::open(&database_path(&temporary_directory)).unwrap();
    database.accept_step(factor(1), step(41_152_263)).unwrap();

    assert_eq!(
        database.accept_step(factor(2), step(41_152_263)).unwrap(),
        MfaAcceptance::Accepted
    );
    assert_eq!(
        database.accepted_step(factor(1)).unwrap(),
        Some(step(41_152_263))
    );
    assert_eq!(
        database.accepted_step(factor(2)).unwrap(),
        Some(step(41_152_263))
    );
}

#[test]
fn a_watermark_survives_reopening_the_database() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let mut database = SqliteDatabase::open(&path).unwrap();
    database.accept_step(factor(1), step(66_666_666)).unwrap();
    drop(database);

    let mut reopened = SqliteDatabase::open(&path).unwrap();

    assert_eq!(
        reopened.accepted_step(factor(1)).unwrap(),
        Some(step(66_666_666))
    );
    assert_eq!(
        reopened.accept_step(factor(1), step(66_666_666)).unwrap(),
        MfaAcceptance::Replayed
    );
}

/// The schema, not only the calling code, refuses a reused or rewound step, so
/// no statement reaching this table can make a spent code usable again.
#[test]
fn a_direct_statement_cannot_reuse_or_rewind_an_accepted_step() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let mut database = SqliteDatabase::open(&path).unwrap();
    database.accept_step(factor(1), step(41_152_263)).unwrap();
    drop(database);

    let connection = Connection::open(&path).unwrap();
    for presented in [41_152_263_i64, 41_152_262, 0] {
        let rejected = connection.execute(
            "UPDATE weavelit_mfa_replay_watermark SET accepted_step = ?2 WHERE factor_id = ?1",
            rusqlite::params![factor(1).as_bytes().as_slice(), presented],
        );

        assert!(rejected.is_err(), "the schema must refuse a reused step");
    }
    let negative = connection.execute(
        "INSERT INTO weavelit_mfa_replay_watermark (factor_id, accepted_step) VALUES (?1, -1)",
        [factor(2).as_bytes().as_slice()],
    );

    assert!(negative.is_err(), "the schema must refuse a negative step");
    assert_eq!(stored_step(&path, factor(1)), Some(41_152_263));
}

#[test]
fn the_backend_serves_its_replay_watermarks_through_the_contract() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let mut concrete = SqliteDatabase::open(&database_path(&temporary_directory)).unwrap();
    let database: &mut dyn ApplicationDatabase = &mut concrete;

    let store = database
        .mfa()
        .expect("the SQLite backend serves watermarks");

    assert_eq!(
        store.accept_step(factor(1), step(1)).unwrap(),
        MfaAcceptance::Accepted
    );
    assert_eq!(
        store.accept_step(factor(1), step(1)).unwrap(),
        MfaAcceptance::Replayed
    );
}
