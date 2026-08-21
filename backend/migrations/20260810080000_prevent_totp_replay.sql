-- Reject a TOTP time step after it has already completed an authentication check.

ALTER TABLE user_mfa_settings
    ADD COLUMN last_reauth_totp_counter BIGINT,
    ADD COLUMN last_reauth_totp_used_at TIMESTAMPTZ,
    ADD CONSTRAINT chk_user_mfa_last_reauth_totp_counter
        CHECK (last_reauth_totp_counter IS NULL OR last_reauth_totp_counter >= 0),
    ADD CONSTRAINT chk_user_mfa_last_reauth_totp_usage_pair
        CHECK (
            (last_reauth_totp_counter IS NULL) = (last_reauth_totp_used_at IS NULL)
        );
