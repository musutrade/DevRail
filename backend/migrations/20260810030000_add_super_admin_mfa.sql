-- Add server-side MFA enrollment, one-time challenges, recovery codes, and passkeys.

CREATE TABLE user_mfa_settings (
    user_id                 BIGINT PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    webauthn_user_id        UUID NOT NULL UNIQUE,
    totp_secret_ciphertext  BYTEA,
    totp_enabled_at         TIMESTAMPTZ,
    recovery_codes_issued_at TIMESTAMPTZ,
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK ((totp_secret_ciphertext IS NULL) = (totp_enabled_at IS NULL))
);

CREATE TABLE user_mfa_recovery_codes (
    id          BIGSERIAL PRIMARY KEY,
    user_id     BIGINT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    code_hash   VARCHAR(255) NOT NULL,
    used_at     TIMESTAMPTZ,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_user_mfa_recovery_codes_active
    ON user_mfa_recovery_codes (user_id, id)
    WHERE used_at IS NULL;

CREATE TABLE user_passkeys (
    id              BIGSERIAL PRIMARY KEY,
    user_id         BIGINT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name            VARCHAR(80) NOT NULL,
    credential_id   TEXT NOT NULL UNIQUE,
    credential      JSONB NOT NULL,
    last_used_at    TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (length(trim(name)) BETWEEN 1 AND 80)
);

CREATE INDEX idx_user_passkeys_user ON user_passkeys (user_id, created_at, id);

CREATE TABLE auth_mfa_challenges (
    id              BIGSERIAL PRIMARY KEY,
    token_hash      CHAR(64) NOT NULL UNIQUE,
    user_id         BIGINT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    kind            VARCHAR(32) NOT NULL CHECK (kind IN (
        'login', 'totp_enrollment', 'passkey_authentication', 'passkey_registration'
    )),
    persistent      BOOLEAN NOT NULL DEFAULT FALSE,
    state           JSONB NOT NULL DEFAULT '{}'::jsonb,
    attempt_count   INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    max_attempts    INTEGER NOT NULL DEFAULT 5 CHECK (max_attempts > 0),
    expires_at      TIMESTAMPTZ NOT NULL,
    consumed_at     TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_auth_mfa_challenges_active
    ON auth_mfa_challenges (token_hash, expires_at)
    WHERE consumed_at IS NULL;

CREATE UNIQUE INDEX uq_auth_mfa_challenges_user_login
    ON auth_mfa_challenges (user_id)
    WHERE consumed_at IS NULL
      AND kind IN ('login', 'totp_enrollment', 'passkey_authentication');

CREATE INDEX idx_auth_mfa_challenges_cleanup
    ON auth_mfa_challenges (expires_at);
