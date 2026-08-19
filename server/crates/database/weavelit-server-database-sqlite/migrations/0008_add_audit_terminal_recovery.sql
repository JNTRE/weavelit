CREATE TABLE weavelit_audit_terminal_obligation (
    sequence_number INTEGER PRIMARY KEY AUTOINCREMENT,
    record_identifier BLOB NOT NULL UNIQUE
        CHECK(length(record_identifier) = 16 AND record_identifier != zeroblob(16)),
    projection BLOB NOT NULL
        CHECK(length(projection) BETWEEN 1 AND 50176),
    binding_identifier BLOB NOT NULL
        CHECK(length(binding_identifier) = 16 AND binding_identifier != zeroblob(16)),
    binding_version BLOB NOT NULL
        CHECK(length(binding_version) = 8 AND binding_version != zeroblob(8)),
    acknowledged INTEGER NOT NULL DEFAULT 0
        CHECK(acknowledged IN (0, 1))
) STRICT;

CREATE INDEX weavelit_audit_terminal_obligation_pending_sequence_idx
    ON weavelit_audit_terminal_obligation (acknowledged, sequence_number);

CREATE TRIGGER weavelit_audit_terminal_obligation_reject_rewrite
BEFORE UPDATE ON weavelit_audit_terminal_obligation
WHEN NEW.sequence_number != OLD.sequence_number
    OR NEW.record_identifier != OLD.record_identifier
    OR NEW.projection != OLD.projection
    OR NEW.binding_identifier != OLD.binding_identifier
    OR NEW.binding_version != OLD.binding_version
    OR OLD.acknowledged != 0
    OR NEW.acknowledged != 1
BEGIN
    SELECT RAISE(ABORT, 'audit terminal obligation is immutable');
END;

CREATE TRIGGER weavelit_audit_terminal_obligation_reject_delete
BEFORE DELETE ON weavelit_audit_terminal_obligation
BEGIN
    SELECT RAISE(ABORT, 'audit terminal obligation cannot be deleted');
END;

CREATE TABLE weavelit_audit_terminal_supersession (
    original_record_identifier BLOB PRIMARY KEY
        REFERENCES weavelit_audit_terminal_obligation(record_identifier),
    disposition BLOB NOT NULL
        CHECK(length(disposition) BETWEEN 1 AND 1024),
    original_binding_identifier BLOB NOT NULL
        CHECK(length(original_binding_identifier) = 16
            AND original_binding_identifier != zeroblob(16)),
    original_binding_version BLOB NOT NULL
        CHECK(length(original_binding_version) = 8
            AND original_binding_version != zeroblob(8)),
    replacement_record_identifier BLOB NOT NULL UNIQUE
        REFERENCES weavelit_audit_terminal_obligation(record_identifier),
    replacement_binding_identifier BLOB NOT NULL
        CHECK(length(replacement_binding_identifier) = 16
            AND replacement_binding_identifier != zeroblob(16)),
    replacement_binding_version BLOB NOT NULL
        CHECK(length(replacement_binding_version) = 8
            AND replacement_binding_version != zeroblob(8)),
    CHECK(original_record_identifier != replacement_record_identifier)
) STRICT;

CREATE TRIGGER weavelit_audit_terminal_supersession_reject_update
BEFORE UPDATE ON weavelit_audit_terminal_supersession
BEGIN
    SELECT RAISE(ABORT, 'audit terminal supersession is immutable');
END;

CREATE TRIGGER weavelit_audit_terminal_supersession_reject_delete
BEFORE DELETE ON weavelit_audit_terminal_supersession
BEGIN
    SELECT RAISE(ABORT, 'audit terminal supersession cannot be deleted');
END;
