-- Store short-lived module unlocks bound to the authenticated session.

CREATE TABLE auth_module_unlocks (
    session_id      BIGINT NOT NULL REFERENCES auth_sessions (id) ON DELETE CASCADE,
    user_id         BIGINT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    module_scope    VARCHAR(32) NOT NULL CHECK (module_scope IN ('users', 'roles')),
    issued_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at      TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (session_id, module_scope)
);

CREATE INDEX idx_auth_module_unlocks_active
    ON auth_module_unlocks (session_id, user_id, module_scope, expires_at);

CREATE INDEX idx_auth_module_unlocks_cleanup
    ON auth_module_unlocks (expires_at);
