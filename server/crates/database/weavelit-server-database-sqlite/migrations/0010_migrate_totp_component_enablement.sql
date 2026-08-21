INSERT INTO weavelit_configuration (component, setting_key, setting_value)
SELECT
    'totp',
    'mfa-module.enabled',
    CASE
        WHEN (
            SELECT setting_value
            FROM weavelit_configuration
            WHERE component = 'mfa.totp' AND setting_key = 'enabled'
        ) = 'true' THEN 'true'
        ELSE 'false'
    END
WHERE EXISTS (
    SELECT 1 FROM weavelit_lifecycle_state WHERE singleton = 1 AND state = 'initialized'
)
ON CONFLICT (component, setting_key) DO UPDATE
SET setting_value = CASE
    WHEN EXISTS (
        SELECT 1
        FROM weavelit_configuration
        WHERE component = 'mfa.totp' AND setting_key = 'enabled'
    ) THEN excluded.setting_value
    ELSE weavelit_configuration.setting_value
END;

DELETE FROM weavelit_configuration
WHERE component = 'mfa.totp'
  AND setting_key = 'enabled'
  AND EXISTS (
      SELECT 1 FROM weavelit_lifecycle_state WHERE singleton = 1 AND state = 'initialized'
  );