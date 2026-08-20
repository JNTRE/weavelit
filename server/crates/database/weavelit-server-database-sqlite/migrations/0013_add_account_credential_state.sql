ALTER TABLE weavelit_account ADD COLUMN credential_revision BLOB NOT NULL
    DEFAULT X'0000000000000001'
    CHECK (
        typeof(credential_revision) = 'blob'
        AND length(credential_revision) = 8
        AND credential_revision <> zeroblob(8)
    );

ALTER TABLE weavelit_account ADD COLUMN must_change_password INTEGER NOT NULL DEFAULT 0
    CHECK (must_change_password IN (0, 1));

ALTER TABLE weavelit_account ADD COLUMN temporary_credential_expires_at_milliseconds INTEGER
    CHECK (
        (must_change_password = 0 AND temporary_credential_expires_at_milliseconds IS NULL)
        OR (
            must_change_password = 1
            AND temporary_credential_expires_at_milliseconds >= 0
        )
    );

CREATE TRIGGER weavelit_account_credential_state_reject_insert
BEFORE INSERT ON weavelit_account
FOR EACH ROW WHEN NOT (
    (NEW.must_change_password = 0
        AND NEW.temporary_credential_expires_at_milliseconds IS NULL)
    OR (
        NEW.must_change_password = 1
        AND NEW.temporary_credential_expires_at_milliseconds IS NOT NULL
        AND NEW.temporary_credential_expires_at_milliseconds >= 0
    )
)
BEGIN
    SELECT RAISE(ABORT, 'temporary credential state is inconsistent');
END;

CREATE TRIGGER weavelit_account_credential_state_reject_update
BEFORE UPDATE OF must_change_password, temporary_credential_expires_at_milliseconds
ON weavelit_account
FOR EACH ROW WHEN NOT (
    (NEW.must_change_password = 0
        AND NEW.temporary_credential_expires_at_milliseconds IS NULL)
    OR (
        NEW.must_change_password = 1
        AND NEW.temporary_credential_expires_at_milliseconds IS NOT NULL
        AND NEW.temporary_credential_expires_at_milliseconds >= 0
    )
)
BEGIN
    SELECT RAISE(ABORT, 'temporary credential state is inconsistent');
END;