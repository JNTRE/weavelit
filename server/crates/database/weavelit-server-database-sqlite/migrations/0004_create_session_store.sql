CREATE TABLE weavelit_session (
    token_hash BLOB PRIMARY KEY
        CHECK (length(token_hash) = 32 AND token_hash <> zeroblob(32)),
    csrf_hash BLOB NOT NULL
        CHECK (length(csrf_hash) = 32 AND csrf_hash <> zeroblob(32)),
    account_id BLOB NOT NULL
        CHECK (length(account_id) = 16 AND account_id <> zeroblob(16)),
    client_module TEXT NOT NULL CHECK (length(CAST(client_module AS BLOB)) BETWEEN 1 AND 256),
    issued_at_milliseconds INTEGER NOT NULL CHECK (issued_at_milliseconds >= 0),
    last_seen_at_milliseconds INTEGER NOT NULL
        CHECK (last_seen_at_milliseconds >= issued_at_milliseconds),
    absolute_expires_at_milliseconds INTEGER NOT NULL
        CHECK (absolute_expires_at_milliseconds = issued_at_milliseconds + 43200000)
) STRICT;

CREATE INDEX weavelit_session_account ON weavelit_session (account_id);

CREATE TRIGGER weavelit_session_immutable_identity
BEFORE UPDATE ON weavelit_session
FOR EACH ROW WHEN
    NEW.token_hash <> OLD.token_hash
    OR NEW.account_id <> OLD.account_id
    OR NEW.client_module <> OLD.client_module
    OR NEW.issued_at_milliseconds <> OLD.issued_at_milliseconds
    OR NEW.absolute_expires_at_milliseconds <> OLD.absolute_expires_at_milliseconds
BEGIN
    SELECT RAISE(ABORT, 'session identity and lifetime are immutable');
END;

CREATE TRIGGER weavelit_session_monotonic_activity
BEFORE UPDATE OF last_seen_at_milliseconds ON weavelit_session
FOR EACH ROW WHEN NEW.last_seen_at_milliseconds < OLD.last_seen_at_milliseconds
BEGIN
    SELECT RAISE(ABORT, 'session activity cannot move backwards');
END;
