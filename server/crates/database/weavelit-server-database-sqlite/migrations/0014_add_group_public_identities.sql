CREATE TABLE weavelit_group_public_identity (
    group_id BLOB PRIMARY KEY REFERENCES weavelit_group (group_id) ON DELETE CASCADE,
    public_identifier BLOB NOT NULL
        CHECK (length(public_identifier) = 16 AND public_identifier <> zeroblob(16))
) STRICT;

CREATE UNIQUE INDEX weavelit_group_public_identity_value
    ON weavelit_group_public_identity (public_identifier);

CREATE TRIGGER weavelit_group_public_identity_reject_update
BEFORE UPDATE ON weavelit_group_public_identity
BEGIN
    SELECT RAISE(ABORT, 'group public identity association is immutable');
END;

INSERT INTO weavelit_group_public_identity (group_id, public_identifier)
SELECT group_id, randomblob(16)
FROM weavelit_group;