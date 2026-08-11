use rusqlite::{Error, ErrorCode};
use weavelit_server_database::DatabaseError;

pub(super) enum ErrorContext {
    Checkpoint,
    Completion,
    Open,
    Configure,
    Health,
    Inspect,
    Migration,
    Session,
    State,
}

pub(super) fn map_sqlite_error(error: Error, context: ErrorContext) -> DatabaseError {
    match error {
        Error::InvalidPath(_) => DatabaseError::ConfigurationInvalid,
        Error::SqliteFailure(failure, _) => map_error_code(failure.code),
        _ => match context {
            ErrorContext::Open => DatabaseError::ConfigurationInvalid,
            ErrorContext::Checkpoint
            | ErrorContext::Completion
            | ErrorContext::Configure
            | ErrorContext::Health
            | ErrorContext::Inspect
            | ErrorContext::Migration
            | ErrorContext::Session
            | ErrorContext::State => DatabaseError::IntegrityFailure,
        },
    }
}

fn map_error_code(code: ErrorCode) -> DatabaseError {
    match code {
        ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase => DatabaseError::IntegrityFailure,
        ErrorCode::DatabaseBusy
        | ErrorCode::DatabaseLocked
        | ErrorCode::ReadOnly
        | ErrorCode::PermissionDenied
        | ErrorCode::SystemIoFailure
        | ErrorCode::DiskFull
        | ErrorCode::CannotOpen
        | ErrorCode::FileLockingProtocolFailed => DatabaseError::Unavailable,
        _ => DatabaseError::IntegrityFailure,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_integrity_codes_to_integrity_failure() {
        for code in [ErrorCode::DatabaseCorrupt, ErrorCode::NotADatabase] {
            assert_eq!(map_error_code(code), DatabaseError::IntegrityFailure);
        }
    }

    #[test]
    fn maps_unavailable_codes_to_unavailable() {
        let codes = [
            ErrorCode::DatabaseBusy,
            ErrorCode::DatabaseLocked,
            ErrorCode::ReadOnly,
            ErrorCode::PermissionDenied,
            ErrorCode::SystemIoFailure,
            ErrorCode::DiskFull,
            ErrorCode::CannotOpen,
            ErrorCode::FileLockingProtocolFailed,
        ];

        for code in codes {
            assert_eq!(map_error_code(code), DatabaseError::Unavailable);
        }
    }
}
