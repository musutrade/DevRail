-- Symphony scheduler reliability metadata.
-- The migration is additive and keeps historical run/event/audit records intact.

ALTER TABLE devrail_tasks
    ADD COLUMN scheduler_attempt INTEGER NOT NULL DEFAULT 0
        CHECK (scheduler_attempt >= 0),
    ADD COLUMN scheduler_retry_count INTEGER NOT NULL DEFAULT 0
        CHECK (scheduler_retry_count >= 0),
    ADD COLUMN scheduler_max_attempts INTEGER NOT NULL DEFAULT 3
        CHECK (scheduler_max_attempts BETWEEN 1 AND 10),
    ADD COLUMN scheduler_retry_at TIMESTAMPTZ,
    ADD COLUMN scheduler_last_error TEXT;

ALTER TABLE devrail_runs
    ADD COLUMN attempt INTEGER;

WITH ranked_runs AS (
    SELECT id,
           ROW_NUMBER() OVER (PARTITION BY task_id ORDER BY created_at, id) AS run_attempt
    FROM devrail_runs
)
UPDATE devrail_runs AS runs
SET attempt = ranked_runs.run_attempt
FROM ranked_runs
WHERE runs.id = ranked_runs.id;

ALTER TABLE devrail_runs
    ALTER COLUMN attempt SET DEFAULT 0,
    ALTER COLUMN attempt SET NOT NULL,
    ADD CONSTRAINT devrail_runs_attempt_positive CHECK (attempt > 0),
    ADD COLUMN actor_type VARCHAR(16) NOT NULL DEFAULT 'user'
        CHECK (actor_type IN ('user', 'system')),
    ADD COLUMN last_heartbeat_at TIMESTAMPTZ,
    ADD COLUMN last_event_at TIMESTAMPTZ,
    ADD COLUMN retry_reason TEXT,
    ADD COLUMN parent_run_id BIGINT,
    ADD COLUMN parent_turn_id VARCHAR(256),
    ADD COLUMN cleanup_status VARCHAR(16) NOT NULL DEFAULT 'pending'
        CHECK (cleanup_status IN ('pending', 'completed', 'failed'));

ALTER TABLE devrail_runs
    ADD CONSTRAINT fk_devrail_runs_parent
        FOREIGN KEY (parent_run_id) REFERENCES devrail_runs (id);

-- During a rolling deployment, the previous application version does not
-- provide attempt explicitly. The sentinel default is rewritten before
-- constraints are checked; current workers always provide a positive attempt
-- and therefore retain deterministic conflict semantics.
CREATE FUNCTION devrail_assign_legacy_run_attempt()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.attempt = 0 THEN
        PERFORM 1
        FROM devrail_tasks
        WHERE id = NEW.task_id AND organization_id = NEW.organization_id
        FOR UPDATE;

        SELECT COALESCE(MAX(attempt), 0) + 1
        INTO NEW.attempt
        FROM devrail_runs
        WHERE organization_id = NEW.organization_id AND task_id = NEW.task_id;
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER trg_devrail_assign_legacy_run_attempt
BEFORE INSERT ON devrail_runs
FOR EACH ROW
EXECUTE FUNCTION devrail_assign_legacy_run_attempt();

CREATE UNIQUE INDEX uq_devrail_run_task_attempt
    ON devrail_runs (organization_id, task_id, attempt);

CREATE INDEX idx_devrail_runs_reconciliation
    ON devrail_runs (status, updated_at, attempt);

CREATE INDEX idx_devrail_tasks_scheduler_retry
    ON devrail_tasks (status, scheduler_retry_at, scheduler_attempt)
    WHERE status = 'queued' AND archived_at IS NULL;
