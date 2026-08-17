CREATE TABLE weavelit_lifecycle_reconciliation (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    digest BLOB NOT NULL CHECK (length(digest) = 32)
) STRICT;