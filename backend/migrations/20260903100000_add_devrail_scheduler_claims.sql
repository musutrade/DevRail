ALTER TABLE devrail_tasks
    ADD COLUMN scheduler_claim_token UUID,
    ADD COLUMN scheduler_claimed_at TIMESTAMPTZ;

CREATE INDEX idx_devrail_tasks_scheduler_claim
    ON devrail_tasks (status, scheduler_claimed_at, priority, due_at, created_at)
    WHERE status = 'queued' AND archived_at IS NULL;
