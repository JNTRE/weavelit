CREATE TABLE weavelit_migration_ledger (
    sequence_number INTEGER PRIMARY KEY CHECK (sequence_number >= 1),
    identifier TEXT NOT NULL UNIQUE CHECK (length(identifier) > 0),
    checksum BLOB NOT NULL CHECK (length(checksum) = 32)
) STRICT;

CREATE TRIGGER weavelit_migration_ledger_reject_update
BEFORE UPDATE ON weavelit_migration_ledger
BEGIN
    SELECT RAISE(ABORT, 'migration ledger is immutable');
END;

CREATE TRIGGER weavelit_migration_ledger_reject_delete
BEFORE DELETE ON weavelit_migration_ledger
BEGIN
    SELECT RAISE(ABORT, 'migration ledger is immutable');
END;