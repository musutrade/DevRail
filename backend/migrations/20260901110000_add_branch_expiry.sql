ALTER TABLE devrail_runs
  ADD COLUMN branch_expires_at TIMESTAMPTZ;

CREATE INDEX idx_devrail_runs_expired_branches
  ON devrail_runs (branch_expires_at)
  WHERE branch_name IS NOT NULL AND branch_expires_at IS NOT NULL;
