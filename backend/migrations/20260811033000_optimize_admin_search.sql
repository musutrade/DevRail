CREATE EXTENSION IF NOT EXISTS pg_trgm;

CREATE INDEX idx_users_username_trgm
    ON users USING GIN (username gin_trgm_ops);
CREATE INDEX idx_users_display_name_trgm
    ON users USING GIN (display_name gin_trgm_ops);
CREATE INDEX idx_users_email_trgm
    ON users USING GIN (email gin_trgm_ops);

CREATE INDEX idx_audit_logs_action_trgm
    ON audit_logs USING GIN (action gin_trgm_ops);
CREATE INDEX idx_audit_logs_target_type_trgm
    ON audit_logs USING GIN (target_type gin_trgm_ops);
