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

CREATE TRIGGER weavelit_group_public_identity_reject_direct_delete
BEFORE DELETE ON weavelit_group_public_identity
WHEN EXISTS (
    SELECT 1
    FROM weavelit_group
    WHERE group_id = OLD.group_id
)
BEGIN
    SELECT RAISE(ABORT, 'group public identity association is immutable');
END;

WITH RECURSIVE
backfill_attempt(attempt) AS (
    VALUES (1)
    UNION ALL
    SELECT attempt + 1
    FROM backfill_attempt
    WHERE attempt < 8
),
backfill_candidate(group_id, attempt, public_identifier) AS MATERIALIZED (
    SELECT group_id, attempt, randomblob(16)
    FROM weavelit_group
    CROSS JOIN backfill_attempt
),
backfill_eligible(group_id, attempt, public_identifier) AS (
    SELECT candidate.group_id, candidate.attempt, candidate.public_identifier
    FROM backfill_candidate AS candidate
    WHERE candidate.public_identifier <> zeroblob(16)
      AND NOT EXISTS (
          SELECT 1
          FROM backfill_candidate AS collision
          WHERE collision.public_identifier = candidate.public_identifier
            AND (
                collision.group_id <> candidate.group_id
                OR collision.attempt <> candidate.attempt
            )
      )
),
backfill_selection(group_id, public_identifier, candidate_rank) AS (
    SELECT group_id,
           public_identifier,
           row_number() OVER (PARTITION BY group_id ORDER BY attempt)
    FROM backfill_eligible
)
INSERT INTO weavelit_group_public_identity (group_id, public_identifier)
SELECT group_id, public_identifier
FROM backfill_selection
WHERE candidate_rank = 1;

-- Force bounded candidate exhaustion to abort the migration transaction.
INSERT INTO weavelit_group_public_identity (group_id, public_identifier)
SELECT target.group_id, zeroblob(16)
FROM weavelit_group AS target
WHERE NOT EXISTS (
    SELECT 1
    FROM weavelit_group_public_identity AS identity
    WHERE identity.group_id = target.group_id
)
LIMIT 1;