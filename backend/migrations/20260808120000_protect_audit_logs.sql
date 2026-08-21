-- Keep the audit trail append-only. Retention deletes require a transaction-local maintenance flag.

CREATE OR REPLACE FUNCTION guard_audit_log_row_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE'
       AND current_setting('arc_admin.audit_maintenance', true) = 'on' THEN
        RETURN OLD;
    END IF;

    RAISE EXCEPTION 'audit_logs is append-only; archive before retention deletion'
        USING ERRCODE = '55000';
END;
$$;

CREATE OR REPLACE FUNCTION guard_audit_log_truncate()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'audit_logs cannot be truncated'
        USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER audit_logs_append_only
BEFORE UPDATE OR DELETE ON audit_logs
FOR EACH ROW
EXECUTE FUNCTION guard_audit_log_row_mutation();

CREATE TRIGGER audit_logs_no_truncate
BEFORE TRUNCATE ON audit_logs
FOR EACH STATEMENT
EXECUTE FUNCTION guard_audit_log_truncate();

COMMENT ON TABLE audit_logs IS
    'Append-only security audit trail. Expired rows may only be removed after verified export.';
