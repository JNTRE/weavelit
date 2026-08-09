CREATE TABLE weavelit_configuration (
    component TEXT NOT NULL CHECK (length(CAST(component AS BLOB)) BETWEEN 1 AND 256),
    setting_key TEXT NOT NULL CHECK (length(CAST(setting_key AS BLOB)) BETWEEN 1 AND 256),
    setting_value TEXT NOT NULL CHECK (length(CAST(setting_value AS BLOB)) BETWEEN 1 AND 4096),
    PRIMARY KEY (component, setting_key)
) STRICT;

CREATE TABLE weavelit_protected_secret (
    component TEXT NOT NULL CHECK (length(CAST(component AS BLOB)) BETWEEN 1 AND 256),
    secret_key TEXT NOT NULL CHECK (length(CAST(secret_key AS BLOB)) BETWEEN 1 AND 256),
    protected_value BLOB NOT NULL CHECK (length(protected_value) BETWEEN 1 AND 65536),
    PRIMARY KEY (component, secret_key)
) STRICT;

CREATE TABLE weavelit_account (
    account_id BLOB PRIMARY KEY
        CHECK (length(account_id) = 16 AND account_id <> zeroblob(16)),
    username TEXT NOT NULL UNIQUE CHECK (length(CAST(username AS BLOB)) BETWEEN 1 AND 256),
    display_name TEXT
        CHECK (display_name IS NULL OR length(CAST(display_name AS BLOB)) BETWEEN 1 AND 256),
    active INTEGER NOT NULL CHECK (active IN (0, 1))
) STRICT;

CREATE TABLE weavelit_password_verifier (
    account_id BLOB PRIMARY KEY REFERENCES weavelit_account (account_id),
    encoded_verifier TEXT NOT NULL CHECK (
        length(CAST(encoded_verifier AS BLOB)) BETWEEN 1 AND 512
        AND substr(encoded_verifier, 1, 1) = '$'
    )
) STRICT;

CREATE TABLE weavelit_group (
    group_id BLOB PRIMARY KEY
        CHECK (length(group_id) = 16 AND group_id <> zeroblob(16)),
    name TEXT NOT NULL UNIQUE CHECK (length(CAST(name AS BLOB)) BETWEEN 1 AND 256),
    description TEXT
        CHECK (description IS NULL OR length(CAST(description AS BLOB)) BETWEEN 1 AND 1024)
) STRICT;

CREATE TABLE weavelit_group_membership (
    group_id BLOB NOT NULL REFERENCES weavelit_group (group_id),
    account_id BLOB NOT NULL REFERENCES weavelit_account (account_id),
    PRIMARY KEY (group_id, account_id)
) STRICT;

CREATE TABLE weavelit_group_grant (
    group_id BLOB NOT NULL REFERENCES weavelit_group (group_id),
    grant_kind TEXT NOT NULL CHECK (
        grant_kind IN ('client_module', 'service_module', 'operation', 'server_administration')
    ),
    grant_value TEXT NOT NULL CHECK (
        (grant_kind = 'server_administration' AND grant_value = '')
        OR (
            grant_kind <> 'server_administration'
            AND length(CAST(grant_value AS BLOB)) BETWEEN 1 AND 256
        )
    ),
    PRIMARY KEY (group_id, grant_kind, grant_value)
) STRICT;

CREATE TABLE weavelit_mfa_factor (
    factor_id BLOB PRIMARY KEY
        CHECK (length(factor_id) = 16 AND factor_id <> zeroblob(16)),
    account_id BLOB NOT NULL REFERENCES weavelit_account (account_id),
    module TEXT NOT NULL CHECK (length(CAST(module AS BLOB)) BETWEEN 1 AND 256),
    protected_factor_data BLOB NOT NULL
        CHECK (length(protected_factor_data) BETWEEN 1 AND 65536),
    UNIQUE (account_id, module)
) STRICT;

CREATE TABLE weavelit_service_connection (
    connection_id BLOB PRIMARY KEY
        CHECK (length(connection_id) = 16 AND connection_id <> zeroblob(16)),
    service_module TEXT NOT NULL CHECK (length(CAST(service_module AS BLOB)) BETWEEN 1 AND 256),
    name TEXT NOT NULL CHECK (length(CAST(name AS BLOB)) BETWEEN 1 AND 256),
    protected_credential BLOB NOT NULL
        CHECK (length(protected_credential) BETWEEN 1 AND 65536),
    UNIQUE (service_module, name)
) STRICT;

CREATE TABLE weavelit_recovery_public_key (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    public_key TEXT NOT NULL CHECK (
        length(CAST(public_key AS BLOB)) BETWEEN 5 AND 128
        AND substr(public_key, 1, 4) = 'age1'
    )
) STRICT;

CREATE TABLE weavelit_log_module_configuration (
    configuration_id BLOB PRIMARY KEY
        CHECK (length(configuration_id) = 16 AND configuration_id <> zeroblob(16)),
    module TEXT NOT NULL CHECK (length(CAST(module AS BLOB)) BETWEEN 1 AND 256),
    name TEXT NOT NULL UNIQUE CHECK (length(CAST(name AS BLOB)) BETWEEN 1 AND 256),
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1))
) STRICT;

CREATE TABLE weavelit_log_module_setting (
    configuration_id BLOB NOT NULL
        REFERENCES weavelit_log_module_configuration (configuration_id),
    setting_key TEXT NOT NULL CHECK (length(CAST(setting_key AS BLOB)) BETWEEN 1 AND 256),
    setting_value TEXT NOT NULL CHECK (length(CAST(setting_value AS BLOB)) BETWEEN 1 AND 4096),
    PRIMARY KEY (configuration_id, setting_key)
) STRICT;

CREATE TABLE weavelit_log_assignment (
    log_type TEXT PRIMARY KEY CHECK (log_type IN ('system', 'audit')),
    configuration_id BLOB NOT NULL
        REFERENCES weavelit_log_module_configuration (configuration_id)
) STRICT;

CREATE TABLE weavelit_completion_obligation (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    record_id BLOB NOT NULL CHECK (length(record_id) = 16 AND record_id <> zeroblob(16)),
    workflow_kind TEXT NOT NULL CHECK (workflow_kind IN ('init', 'restore')),
    classification TEXT NOT NULL CHECK (length(CAST(classification AS BLOB)) BETWEEN 1 AND 128),
    correlation_identifier TEXT NOT NULL
        CHECK (length(CAST(correlation_identifier AS BLOB)) BETWEEN 1 AND 64),
    event_time_milliseconds INTEGER NOT NULL CHECK (event_time_milliseconds >= 0),
    detail TEXT NOT NULL CHECK (length(CAST(detail AS BLOB)) BETWEEN 1 AND 4096),
    acknowledged INTEGER NOT NULL CHECK (acknowledged IN (0, 1))
) STRICT;
