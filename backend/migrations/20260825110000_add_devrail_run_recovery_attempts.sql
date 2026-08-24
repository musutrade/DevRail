ALTER TABLE devrail_runs
    ADD COLUMN recovery_attempts INTEGER NOT NULL DEFAULT 0;
