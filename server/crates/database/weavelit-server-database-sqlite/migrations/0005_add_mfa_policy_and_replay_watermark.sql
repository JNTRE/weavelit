ALTER TABLE weavelit_account
    ADD COLUMN mfa_required INTEGER NOT NULL DEFAULT 0 CHECK (mfa_required IN (0, 1));

CREATE TABLE weavelit_mfa_replay_watermark (
    factor_id BLOB PRIMARY KEY
        CHECK (length(factor_id) = 16 AND factor_id <> zeroblob(16)),
    accepted_step INTEGER NOT NULL CHECK (accepted_step >= 0)
) STRICT;

CREATE TRIGGER weavelit_mfa_replay_watermark_monotonic
BEFORE UPDATE OF accepted_step ON weavelit_mfa_replay_watermark
FOR EACH ROW WHEN NEW.accepted_step <= OLD.accepted_step
BEGIN
    SELECT RAISE(ABORT, 'an accepted time step cannot be reused or moved backwards');
END;
