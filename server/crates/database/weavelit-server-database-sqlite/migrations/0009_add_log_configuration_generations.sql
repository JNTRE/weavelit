CREATE TABLE weavelit_log_configuration_generation (
    configuration_id BLOB NOT NULL
        CHECK (length(configuration_id) = 16 AND configuration_id <> zeroblob(16)),
    generation_version BLOB NOT NULL
        CHECK (length(generation_version) = 8 AND generation_version <> zeroblob(8)),
    module TEXT NOT NULL CHECK (length(CAST(module AS BLOB)) BETWEEN 1 AND 256),
    name TEXT NOT NULL CHECK (length(CAST(name AS BLOB)) BETWEEN 1 AND 256),
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    PRIMARY KEY (configuration_id, generation_version)
) STRICT;

CREATE TABLE weavelit_log_configuration_generation_setting (
    configuration_id BLOB NOT NULL,
    generation_version BLOB NOT NULL,
    setting_key TEXT NOT NULL CHECK (length(CAST(setting_key AS BLOB)) BETWEEN 1 AND 256),
    setting_value TEXT NOT NULL CHECK (length(CAST(setting_value AS BLOB)) BETWEEN 1 AND 4096),
    PRIMARY KEY (configuration_id, generation_version, setting_key),
    FOREIGN KEY (configuration_id, generation_version)
        REFERENCES weavelit_log_configuration_generation (configuration_id, generation_version)
) STRICT;

CREATE TABLE weavelit_log_configuration_generation_log_type (
    configuration_id BLOB NOT NULL,
    generation_version BLOB NOT NULL,
    log_type TEXT NOT NULL CHECK (log_type IN ('system', 'audit')),
    PRIMARY KEY (configuration_id, generation_version, log_type),
    FOREIGN KEY (configuration_id, generation_version)
        REFERENCES weavelit_log_configuration_generation (configuration_id, generation_version)
) STRICT;

CREATE TABLE weavelit_log_configuration_current_generation (
    configuration_id BLOB PRIMARY KEY
        CHECK (length(configuration_id) = 16 AND configuration_id <> zeroblob(16)),
    generation_version BLOB NOT NULL
        CHECK (length(generation_version) = 8 AND generation_version <> zeroblob(8)),
    FOREIGN KEY (configuration_id, generation_version)
        REFERENCES weavelit_log_configuration_generation (configuration_id, generation_version)
) STRICT;

INSERT INTO weavelit_log_configuration_generation (
    configuration_id,
    generation_version,
    module,
    name,
    enabled
)
SELECT configuration_id, X'0000000000000001', module, name, enabled
FROM weavelit_log_module_configuration;

INSERT INTO weavelit_log_configuration_generation_setting (
    configuration_id,
    generation_version,
    setting_key,
    setting_value
)
SELECT configuration_id, X'0000000000000001', setting_key, setting_value
FROM weavelit_log_module_setting;

INSERT INTO weavelit_log_configuration_generation_log_type (
    configuration_id,
    generation_version,
    log_type
)
SELECT configuration_id, X'0000000000000001', log_type
FROM weavelit_log_assignment;

INSERT INTO weavelit_log_configuration_current_generation (
    configuration_id,
    generation_version
)
SELECT configuration_id, X'0000000000000001'
FROM weavelit_log_module_configuration;

CREATE TRIGGER weavelit_log_configuration_generation_reject_update
BEFORE UPDATE ON weavelit_log_configuration_generation
BEGIN
    SELECT RAISE(ABORT, 'log configuration generations are immutable');
END;

CREATE TRIGGER weavelit_log_configuration_generation_reject_delete
BEFORE DELETE ON weavelit_log_configuration_generation
BEGIN
    SELECT RAISE(ABORT, 'log configuration generations are immutable');
END;

CREATE TRIGGER weavelit_log_configuration_generation_setting_reject_update
BEFORE UPDATE ON weavelit_log_configuration_generation_setting
BEGIN
    SELECT RAISE(ABORT, 'log configuration generation settings are immutable');
END;

CREATE TRIGGER weavelit_log_configuration_generation_setting_reject_delete
BEFORE DELETE ON weavelit_log_configuration_generation_setting
BEGIN
    SELECT RAISE(ABORT, 'log configuration generation settings are immutable');
END;

CREATE TRIGGER weavelit_log_configuration_generation_log_type_reject_update
BEFORE UPDATE ON weavelit_log_configuration_generation_log_type
BEGIN
    SELECT RAISE(ABORT, 'log configuration generation Log Types are immutable');
END;

CREATE TRIGGER weavelit_log_configuration_generation_log_type_reject_delete
BEFORE DELETE ON weavelit_log_configuration_generation_log_type
BEGIN
    SELECT RAISE(ABORT, 'log configuration generation Log Types are immutable');
END;