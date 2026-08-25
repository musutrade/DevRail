ALTER TABLE devrail_runs
  ADD COLUMN branch_name VARCHAR(256);

CREATE INDEX idx_devrail_runs_branch ON devrail_runs (organization_id, branch_name)
  WHERE branch_name IS NOT NULL;
