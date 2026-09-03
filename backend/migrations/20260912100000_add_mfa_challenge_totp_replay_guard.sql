-- Track accepted TOTP time steps on the challenge that consumed them.  The
-- partial unique index prevents replay through a newly issued challenge while
-- keeping login and enrollment counters in separate purpose domains.
ALTER TABLE auth_mfa_challenges
  ADD COLUMN accepted_totp_counter BIGINT,
  ADD COLUMN accepted_totp_at TIMESTAMPTZ,
  ADD CONSTRAINT chk_auth_mfa_challenges_totp_counter
    CHECK (accepted_totp_counter IS NULL OR accepted_totp_counter >= 0),
  ADD CONSTRAINT chk_auth_mfa_challenges_totp_counter_pair
    CHECK (
      (accepted_totp_counter IS NULL) = (accepted_totp_at IS NULL)
    ),
  ADD CONSTRAINT chk_auth_mfa_challenges_totp_counter_kind
    CHECK (
      accepted_totp_counter IS NULL
      OR kind IN ('login', 'totp_enrollment')
    );

CREATE UNIQUE INDEX uq_auth_mfa_challenges_totp_counter_per_kind
  ON auth_mfa_challenges (user_id, kind, accepted_totp_counter)
  WHERE accepted_totp_counter IS NOT NULL;
