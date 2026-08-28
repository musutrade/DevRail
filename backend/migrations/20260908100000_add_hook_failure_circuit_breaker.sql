ALTER TABLE devrail_tasks
    ADD COLUMN hook_failure_fingerprint VARCHAR(64),
    ADD COLUMN hook_failure_count INTEGER NOT NULL DEFAULT 0
        CHECK (hook_failure_count >= 0);
