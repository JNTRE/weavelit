CREATE TABLE weavelit_lifecycle_state (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    deployment_identifier BLOB NOT NULL CHECK (length(deployment_identifier) = 16),
    state TEXT NOT NULL CHECK (state IN ('pending', 'initialized')),
    workflow_kind TEXT,
    checkpoint_metadata BLOB,
    CHECK (
        (
            state = 'pending'
            AND workflow_kind IN ('init', 'restore')
            AND checkpoint_metadata IS NOT NULL
            AND length(checkpoint_metadata) <= 4096
        )
        OR
        (
            state = 'initialized'
            AND workflow_kind IS NULL
            AND checkpoint_metadata IS NULL
        )
    )
) STRICT;