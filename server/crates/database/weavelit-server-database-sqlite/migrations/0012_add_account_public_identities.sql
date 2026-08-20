CREATE TABLE weavelit_account_public_identity (
    account_id BLOB PRIMARY KEY REFERENCES weavelit_account (account_id),
    public_identifier BLOB NOT NULL
        CHECK (length(public_identifier) = 16 AND public_identifier <> zeroblob(16))
) STRICT;

CREATE UNIQUE INDEX weavelit_account_public_identity_value
    ON weavelit_account_public_identity (public_identifier);

CREATE TRIGGER weavelit_account_public_identity_reject_update
BEFORE UPDATE ON weavelit_account_public_identity
BEGIN
    SELECT RAISE(ABORT, 'account public identity association is immutable');
END;

CREATE TRIGGER weavelit_account_public_identity_reject_delete
BEFORE DELETE ON weavelit_account_public_identity
BEGIN
    SELECT RAISE(ABORT, 'account public identity association is immutable');
END;

INSERT INTO weavelit_account_public_identity (account_id, public_identifier)
SELECT account_id, randomblob(16)
FROM weavelit_account;
