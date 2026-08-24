ALTER TABLE devrail_tasks
    ADD COLUMN repository_id BIGINT,
    ADD COLUMN environment_id BIGINT;

ALTER TABLE devrail_tasks
    ADD CONSTRAINT fk_devrail_tasks_repository
        FOREIGN KEY (repository_id, organization_id)
        REFERENCES devrail_repositories (id, organization_id),
    ADD CONSTRAINT fk_devrail_tasks_environment
        FOREIGN KEY (environment_id, organization_id)
        REFERENCES devrail_environments (id, organization_id);

CREATE INDEX idx_devrail_tasks_resources
    ON devrail_tasks (organization_id, project_id, repository_id, environment_id);
