-- TaskTracker dispatch snapshots and repository workflow versions.
-- The migration is additive so rolling workers can keep using legacy defaults.

ALTER TABLE devrail_tasks
    ADD COLUMN revision BIGINT NOT NULL DEFAULT 1 CHECK (revision > 0),
    ADD COLUMN dispatch_snapshot JSONB NOT NULL DEFAULT '{"schemaVersion":1,"source":"legacy","workflow":{"source":"legacy","version":"legacy-v1","digest":"0000000000000000000000000000000000000000000000000000000000000000"}}'::jsonb,
    ADD COLUMN dispatch_snapshot_digest CHAR(64) NOT NULL DEFAULT repeat('0', 64),
    ADD COLUMN workflow_source VARCHAR(16) NOT NULL DEFAULT 'legacy'
        CHECK (workflow_source IN ('default', 'repository', 'legacy')),
    ADD COLUMN workflow_version VARCHAR(64) NOT NULL DEFAULT 'legacy-v1',
    ADD COLUMN workflow_digest CHAR(64) NOT NULL DEFAULT repeat('0', 64),
    ADD CONSTRAINT devrail_tasks_dispatch_snapshot_size
        CHECK (octet_length(dispatch_snapshot::text) <= 524288),
    ADD CONSTRAINT devrail_tasks_dispatch_digest_format
        CHECK (dispatch_snapshot_digest ~ '^[0-9a-f]{64}$'),
    ADD CONSTRAINT devrail_tasks_workflow_digest_format
        CHECK (workflow_digest ~ '^[0-9a-f]{64}$');

UPDATE devrail_tasks
SET dispatch_snapshot = jsonb_build_object(
        'schemaVersion', 1,
        'taskRevision', revision,
        'taskId', id,
        'projectId', project_id,
        'repositoryId', repository_id,
        'environmentId', environment_id,
        'title', title,
        'goal', goal,
        'background', background,
        'acceptanceCriteria', acceptance_criteria,
        'constraints', constraints,
        'labels', labels,
        'workflow', jsonb_build_object(
            'source', 'legacy',
            'version', 'legacy-v1',
            'digest', repeat('0', 64)
        )
    );

ALTER TABLE devrail_runs
    ADD COLUMN task_revision BIGINT NOT NULL DEFAULT 1 CHECK (task_revision > 0),
    ADD COLUMN workflow_source VARCHAR(16) NOT NULL DEFAULT 'legacy'
        CHECK (workflow_source IN ('default', 'repository', 'legacy')),
    ADD COLUMN workflow_version VARCHAR(64) NOT NULL DEFAULT 'legacy-v1',
    ADD COLUMN workflow_digest CHAR(64) NOT NULL DEFAULT repeat('0', 64),
    ADD COLUMN workflow_snapshot JSONB NOT NULL DEFAULT '{"schemaVersion":1,"source":"legacy","version":"legacy-v1","digest":"0000000000000000000000000000000000000000000000000000000000000000"}'::jsonb,
    ADD CONSTRAINT devrail_runs_workflow_digest_format
        CHECK (workflow_digest ~ '^[0-9a-f]{64}$'),
    ADD CONSTRAINT devrail_runs_workflow_snapshot_size
        CHECK (octet_length(workflow_snapshot::text) <= 524288);

UPDATE devrail_runs AS r
SET task_revision = t.revision,
    workflow_source = t.workflow_source,
    workflow_version = t.workflow_version,
    workflow_digest = t.workflow_digest,
    workflow_snapshot = COALESCE(t.dispatch_snapshot -> 'workflow', r.workflow_snapshot)
FROM devrail_tasks AS t
WHERE t.id = r.task_id AND t.organization_id = r.organization_id;

CREATE TABLE devrail_workflow_versions (
    id BIGSERIAL PRIMARY KEY,
    organization_id BIGINT NOT NULL REFERENCES organizations (id),
    department_id BIGINT,
    owner_user_id BIGINT NOT NULL REFERENCES users (id),
    environment_id BIGINT NOT NULL,
    source VARCHAR(16) NOT NULL CHECK (source IN ('default', 'repository')),
    declared_version VARCHAR(64) NOT NULL,
    digest CHAR(64) NOT NULL CHECK (digest ~ '^[0-9a-f]{64}$'),
    normalized_snapshot JSONB NOT NULL,
    prompt_body TEXT NOT NULL,
    accepted_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (id, organization_id),
    UNIQUE (organization_id, environment_id, source, digest),
    FOREIGN KEY (environment_id, organization_id)
        REFERENCES devrail_environments (id, organization_id),
    FOREIGN KEY (department_id, organization_id)
        REFERENCES departments (id, organization_id),
    CHECK (octet_length(normalized_snapshot::text) <= 262144),
    CHECK (octet_length(prompt_body) <= 262144)
);

CREATE INDEX idx_devrail_workflow_versions_latest
    ON devrail_workflow_versions (organization_id, environment_id, accepted_at DESC, id DESC);

CREATE TABLE devrail_workflow_reload_failures (
    id BIGSERIAL PRIMARY KEY,
    organization_id BIGINT NOT NULL REFERENCES organizations (id),
    department_id BIGINT,
    owner_user_id BIGINT NOT NULL REFERENCES users (id),
    environment_id BIGINT NOT NULL,
    candidate_digest CHAR(64) NOT NULL CHECK (candidate_digest ~ '^[0-9a-f]{64}$'),
    error_kind VARCHAR(32) NOT NULL CHECK (error_kind IN (
        'path', 'size', 'front_matter', 'schema', 'template', 'policy', 'io'
    )),
    occurrence_count BIGINT NOT NULL DEFAULT 1 CHECK (occurrence_count > 0),
    first_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (id, organization_id),
    UNIQUE (organization_id, environment_id, candidate_digest, error_kind),
    FOREIGN KEY (environment_id, organization_id)
        REFERENCES devrail_environments (id, organization_id),
    FOREIGN KEY (department_id, organization_id)
        REFERENCES departments (id, organization_id)
);

CREATE TABLE devrail_task_status_history (
    id BIGSERIAL PRIMARY KEY,
    organization_id BIGINT NOT NULL REFERENCES organizations (id),
    department_id BIGINT,
    owner_user_id BIGINT NOT NULL REFERENCES users (id),
    task_id BIGINT NOT NULL,
    task_revision BIGINT NOT NULL CHECK (task_revision > 0),
    from_status VARCHAR(24),
    to_status VARCHAR(24) NOT NULL,
    actor_type VARCHAR(16) NOT NULL CHECK (actor_type IN ('user', 'system')),
    actor_user_id BIGINT REFERENCES users (id),
    reason VARCHAR(256) NOT NULL,
    trace_id VARCHAR(128),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (id, organization_id),
    FOREIGN KEY (task_id, organization_id) REFERENCES devrail_tasks (id, organization_id),
    FOREIGN KEY (department_id, organization_id) REFERENCES departments (id, organization_id)
);

CREATE INDEX idx_devrail_task_status_history_task
    ON devrail_task_status_history (organization_id, task_id, created_at, id);

CREATE FUNCTION devrail_guard_task_dispatch_snapshot()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    input_changed BOOLEAN;
BEGIN
    input_changed := ROW(
        NEW.title, NEW.goal, NEW.background, NEW.acceptance_criteria,
        NEW.constraints, NEW.repository_id, NEW.environment_id
    ) IS DISTINCT FROM ROW(
        OLD.title, OLD.goal, OLD.background, OLD.acceptance_criteria,
        OLD.constraints, OLD.repository_id, OLD.environment_id
    );

    IF OLD.status IN ('queued', 'running', 'awaiting_approval') AND input_changed THEN
        RAISE EXCEPTION 'queued task dispatch inputs are immutable'
            USING ERRCODE = '23514';
    END IF;

    IF input_changed THEN
        NEW.revision := OLD.revision + 1;
    END IF;

    IF OLD.status IN ('queued', 'running', 'awaiting_approval') AND (
        NEW.dispatch_snapshot IS DISTINCT FROM OLD.dispatch_snapshot
        OR NEW.dispatch_snapshot_digest IS DISTINCT FROM OLD.dispatch_snapshot_digest
        OR NEW.workflow_source IS DISTINCT FROM OLD.workflow_source
        OR NEW.workflow_version IS DISTINCT FROM OLD.workflow_version
        OR NEW.workflow_digest IS DISTINCT FROM OLD.workflow_digest
    ) THEN
        RAISE EXCEPTION 'queued task workflow snapshot is immutable'
            USING ERRCODE = '23514';
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER trg_devrail_guard_task_dispatch_snapshot
BEFORE UPDATE ON devrail_tasks
FOR EACH ROW
EXECUTE FUNCTION devrail_guard_task_dispatch_snapshot();

CREATE FUNCTION devrail_record_task_status_history()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    configured_actor_type TEXT;
    configured_actor_user_id BIGINT;
    configured_reason TEXT;
    configured_trace TEXT;
BEGIN
    IF OLD.status IS NOT DISTINCT FROM NEW.status THEN
        RETURN NEW;
    END IF;

    configured_actor_type := NULLIF(current_setting('devrail.actor_type', true), '');
    configured_actor_user_id := NULLIF(current_setting('devrail.actor_user_id', true), '')::BIGINT;
    configured_reason := NULLIF(current_setting('devrail.transition_reason', true), '');
    configured_trace := NULLIF(current_setting('devrail.trace_id', true), '');

    INSERT INTO devrail_task_status_history (
        organization_id, department_id, owner_user_id, task_id, task_revision,
        from_status, to_status, actor_type, actor_user_id, reason, trace_id
    ) VALUES (
        NEW.organization_id, NEW.department_id, NEW.owner_user_id, NEW.id, NEW.revision,
        OLD.status, NEW.status,
        COALESCE(configured_actor_type,
            CASE WHEN NEW.scheduler_claim_token IS NOT NULL THEN 'system' ELSE 'user' END),
        COALESCE(configured_actor_user_id, NEW.owner_user_id),
        COALESCE(configured_reason, 'task_status_updated'),
        configured_trace
    );
    RETURN NEW;
END;
$$;

CREATE TRIGGER trg_devrail_record_task_status_history
AFTER UPDATE OF status ON devrail_tasks
FOR EACH ROW
EXECUTE FUNCTION devrail_record_task_status_history();
