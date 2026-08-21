-- Store short-lived, single-use re-authentication grants bound to one session.

CREATE TABLE auth_step_up_tokens (
    id              BIGSERIAL PRIMARY KEY,
    token_hash      CHAR(64) NOT NULL UNIQUE,
    session_id      BIGINT NOT NULL REFERENCES auth_sessions (id) ON DELETE CASCADE,
    user_id         BIGINT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    scope           VARCHAR(64) NOT NULL,
    expires_at      TIMESTAMPTZ NOT NULL,
    consumed_at     TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_auth_step_up_tokens_cleanup
    ON auth_step_up_tokens (expires_at);

CREATE INDEX idx_auth_step_up_tokens_active
    ON auth_step_up_tokens (session_id, user_id, scope, expires_at)
    WHERE consumed_at IS NULL;
