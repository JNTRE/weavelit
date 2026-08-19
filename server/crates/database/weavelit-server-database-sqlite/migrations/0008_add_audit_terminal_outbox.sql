CREATE TABLE weavelit_audit_terminal_outbox (
    sequence_number INTEGER PRIMARY KEY AUTOINCREMENT,
    obligation_identifier BLOB NOT NULL UNIQUE
        CHECK (
            typeof(obligation_identifier) = 'blob'
            AND length(obligation_identifier) = 16
            AND obligation_identifier != zeroblob(16)
        ),
    projection BLOB NOT NULL
        CHECK (
            typeof(projection) = 'blob'
            AND length(projection) BETWEEN 1 AND 50176
        ),
    binding_identifier BLOB NOT NULL
        CHECK (
            typeof(binding_identifier) = 'blob'
            AND length(binding_identifier) = 16
            AND binding_identifier != zeroblob(16)
        ),
    binding_version BLOB NOT NULL
        CHECK (
            typeof(binding_version) = 'blob'
            AND length(binding_version) = 8
            AND binding_version != zeroblob(8)
        )
) STRICT;

CREATE TRIGGER weavelit_audit_terminal_outbox_reject_update
BEFORE UPDATE ON weavelit_audit_terminal_outbox
BEGIN
    SELECT RAISE(ABORT, 'audit terminal obligations are immutable');
END;

CREATE TABLE weavelit_audit_terminal_supersession (
    original_obligation_identifier BLOB PRIMARY KEY NOT NULL
        CHECK (
            typeof(original_obligation_identifier) = 'blob'
            AND length(original_obligation_identifier) = 16
            AND original_obligation_identifier != zeroblob(16)
        ),
    disposition BLOB NOT NULL
        CHECK (
            typeof(disposition) = 'blob'
            AND length(disposition) BETWEEN 1 AND 1024
        ),
    replacement_obligation_identifier BLOB NOT NULL UNIQUE
        CHECK (
            typeof(replacement_obligation_identifier) = 'blob'
            AND length(replacement_obligation_identifier) = 16
            AND replacement_obligation_identifier != zeroblob(16)
            AND replacement_obligation_identifier != original_obligation_identifier
        )
) STRICT;

CREATE TRIGGER weavelit_audit_terminal_supersession_reject_update
BEFORE UPDATE ON weavelit_audit_terminal_supersession
BEGIN
    SELECT RAISE(ABORT, 'audit terminal supersession dispositions are immutable');
END;

CREATE TRIGGER weavelit_audit_terminal_supersession_reject_delete
BEFORE DELETE ON weavelit_audit_terminal_supersession
BEGIN
    SELECT RAISE(ABORT, 'audit terminal supersession dispositions are append-only');
END;