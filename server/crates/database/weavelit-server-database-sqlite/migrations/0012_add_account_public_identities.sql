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

WITH RECURSIVE
backfill_attempt(attempt) AS (
    VALUES (1)
    UNION ALL
    SELECT attempt + 1
    FROM backfill_attempt
    WHERE attempt < 8
),
backfill_candidate(account_id, attempt, public_identifier) AS MATERIALIZED (
    SELECT account_id, attempt, randomblob(16)
    FROM weavelit_account
    CROSS JOIN backfill_attempt
),
backfill_eligible(account_id, attempt, public_identifier) AS (
    SELECT candidate.account_id, candidate.attempt, candidate.public_identifier
    FROM backfill_candidate AS candidate
    WHERE candidate.public_identifier <> zeroblob(16)
      AND NOT EXISTS (
          SELECT 1
          FROM backfill_candidate AS collision
          WHERE collision.public_identifier = candidate.public_identifier
            AND (
                collision.account_id <> candidate.account_id
                OR collision.attempt <> candidate.attempt
            )
      )
),
backfill_selection(account_id, public_identifier, candidate_rank) AS (
    SELECT account_id,
           public_identifier,
           row_number() OVER (PARTITION BY account_id ORDER BY attempt)
    FROM backfill_eligible
)
INSERT INTO weavelit_account_public_identity (account_id, public_identifier)
SELECT account_id, public_identifier
FROM backfill_selection
WHERE candidate_rank = 1;

-- Force bounded candidate exhaustion to abort the migration transaction.
INSERT INTO weavelit_account_public_identity (account_id, public_identifier)
SELECT account.account_id, zeroblob(16)
FROM weavelit_account AS account
WHERE NOT EXISTS (
    SELECT 1
    FROM weavelit_account_public_identity AS identity
    WHERE identity.account_id = account.account_id
)
LIMIT 1;
