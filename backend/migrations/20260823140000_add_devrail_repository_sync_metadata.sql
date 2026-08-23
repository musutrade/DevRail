ALTER TABLE devrail_repositories
    ADD COLUMN last_remote_branch VARCHAR(128),
    ADD COLUMN last_remote_branch_count BIGINT;
