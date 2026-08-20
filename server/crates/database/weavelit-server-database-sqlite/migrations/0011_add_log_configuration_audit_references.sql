CREATE TABLE weavelit_log_configuration_audit_reference (
    configuration_id BLOB PRIMARY KEY
        REFERENCES weavelit_log_module_configuration (configuration_id),
    audit_reference TEXT NOT NULL CHECK (
        length(CAST(audit_reference AS BLOB)) = 35
        AND substr(audit_reference, 1, 3) = 'ar-'
        AND substr(audit_reference, 4) NOT GLOB '*[^0-9a-f]*'
        AND substr(audit_reference, 4) <> '00000000000000000000000000000000'
    )
) STRICT;

CREATE UNIQUE INDEX weavelit_log_configuration_audit_reference_value
    ON weavelit_log_configuration_audit_reference (audit_reference);

CREATE TRIGGER weavelit_log_configuration_audit_reference_reject_cross_kind_reuse
BEFORE INSERT ON weavelit_log_configuration_audit_reference
WHEN EXISTS (
    SELECT 1 FROM weavelit_account_audit_reference
    WHERE audit_reference = NEW.audit_reference
) OR EXISTS (
    SELECT 1 FROM weavelit_group_audit_reference
    WHERE audit_reference = NEW.audit_reference
)
BEGIN
    SELECT RAISE(ABORT, 'audit reference already belongs to another entity kind');
END;

CREATE TRIGGER weavelit_account_audit_reference_reject_configuration_reuse
BEFORE INSERT ON weavelit_account_audit_reference
WHEN EXISTS (
    SELECT 1 FROM weavelit_log_configuration_audit_reference
    WHERE audit_reference = NEW.audit_reference
)
BEGIN
    SELECT RAISE(ABORT, 'audit reference already belongs to another entity kind');
END;

CREATE TRIGGER weavelit_group_audit_reference_reject_configuration_reuse
BEFORE INSERT ON weavelit_group_audit_reference
WHEN EXISTS (
    SELECT 1 FROM weavelit_log_configuration_audit_reference
    WHERE audit_reference = NEW.audit_reference
)
BEGIN
    SELECT RAISE(ABORT, 'audit reference already belongs to another entity kind');
END;

CREATE TRIGGER weavelit_log_configuration_audit_reference_reject_update
BEFORE UPDATE ON weavelit_log_configuration_audit_reference
BEGIN
    SELECT RAISE(ABORT, 'audit reference association is immutable');
END;

INSERT INTO weavelit_log_configuration_audit_reference (
    configuration_id,
    audit_reference
)
SELECT configuration_id, 'ar-' || lower(hex(randomblob(16)))
FROM weavelit_log_module_configuration;