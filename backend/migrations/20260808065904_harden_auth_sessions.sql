-- Replace browser-readable JWTs with revocable server-side sessions and persist login throttles.

CREATE TABLE auth_sessions (
    id                    BIGSERIAL PRIMARY KEY,
    user_id               BIGINT      NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    session_token_hash    CHAR(64)    NOT NULL UNIQUE,
    csrf_token_hash       CHAR(64)    NOT NULL,
    token_version         BIGINT      NOT NULL,
    idle_timeout_secs     BIGINT      NOT NULL CHECK (idle_timeout_secs > 0),
    persistent            BOOLEAN     NOT NULL DEFAULT FALSE,
    expires_at            TIMESTAMPTZ NOT NULL,
    last_seen_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at            TIMESTAMPTZ,
    created_at            TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_auth_sessions_user_active
    ON auth_sessions (user_id, created_at DESC)
    WHERE revoked_at IS NULL;
CREATE INDEX idx_auth_sessions_expiry
    ON auth_sessions (expires_at)
    WHERE revoked_at IS NULL;

CREATE TABLE auth_login_attempts (
    identifier_hash   CHAR(64)    PRIMARY KEY,
    failure_count     INTEGER     NOT NULL CHECK (failure_count > 0),
    window_started_at TIMESTAMPTZ NOT NULL,
    locked_until      TIMESTAMPTZ,
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_auth_login_attempts_cleanup
    ON auth_login_attempts (updated_at);
