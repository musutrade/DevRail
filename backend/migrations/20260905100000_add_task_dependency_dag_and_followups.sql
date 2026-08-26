-- Task dependency DAG, deterministic propagation, and controlled Agent follow-ups.
-- Existing tasks remain an empty graph and retain their current dispatch behavior.

INSERT INTO permissions (group_id, code, name, type, description, sort_order)
VALUES
    ((SELECT id FROM permission_groups WHERE code = 'devrail'),
     'devrail:task_dependency:read', '查看任务依赖', 'api',
     '允许查看数据范围内任务的前置与下游关系。', 81),
    ((SELECT id FROM permission_groups WHERE code = 'devrail'),
     'devrail:task_dependency:write', '管理任务依赖', 'api',
     '允许在数据范围内创建、替换和删除任务依赖。', 82),
    ((SELECT id FROM permission_groups WHERE code = 'devrail'),
     'devrail:followup:create', '创建后续任务', 'api',
     '允许受控 Agent run 提议后续任务。', 83)
ON CONFLICT (code) DO UPDATE
SET group_id = EXCLUDED.group_id, name = EXCLUDED.name, type = EXCLUDED.type,
    description = EXCLUDED.description, sort_order = EXCLUDED.sort_order;

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code IN (
    'devrail:task_dependency:read',
    'devrail:task_dependency:write',
    'devrail:followup:create'
)
WHERE r.code = 'super_admin'
ON CONFLICT DO NOTHING;

-- Relationship visibility follows the existing DevRail project roles.  Write
-- access is limited to organization/project administrators; the follow-up
-- permission is intentionally not granted to human roles because Supervisor
-- supplies it through a System Actor.
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code = 'devrail:task_dependency:read'
WHERE r.code IN ('organization_admin', 'project_admin', 'developer', 'reviewer', 'observer')
ON CONFLICT DO NOTHING;

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code = 'devrail:task_dependency:write'
WHERE r.code IN ('organization_admin', 'project_admin')
ON CONFLICT DO NOTHING;

DELETE FROM role_permissions
WHERE permission_id = (SELECT id FROM permissions WHERE code = 'devrail:followup:create')
  AND role_id <> (SELECT id FROM roles WHERE code = 'super_admin');

ALTER TABLE devrail_tasks
    DROP CONSTRAINT devrail_tasks_status_check,
    ADD CONSTRAINT devrail_tasks_status_check
        CHECK (status IN (
            'draft', 'queued', 'running', 'awaiting_approval', 'succeeded',
            'failed', 'cancelled', 'skipped', 'archived'
        )),
    ADD COLUMN creation_source VARCHAR(24) NOT NULL DEFAULT 'legacy'
        CHECK (creation_source IN ('legacy', 'manual', 'agent_followup', 'system')),
    ADD COLUMN source_task_id BIGINT,
    ADD COLUMN source_run_id BIGINT,
    ADD COLUMN followup_depth SMALLINT NOT NULL DEFAULT 0
        CHECK (followup_depth BETWEEN 0 AND 16),
    ADD CONSTRAINT fk_devrail_tasks_source_task
        FOREIGN KEY (source_task_id, organization_id)
        REFERENCES devrail_tasks (id, organization_id),
    ADD CONSTRAINT fk_devrail_tasks_source_run
        FOREIGN KEY (source_run_id, organization_id)
        REFERENCES devrail_runs (id, organization_id),
    ADD CONSTRAINT devrail_tasks_followup_source_check CHECK (
        (creation_source = 'agent_followup'
            AND source_task_id IS NOT NULL AND source_run_id IS NOT NULL)
        OR creation_source <> 'agent_followup'
    );

UPDATE devrail_tasks SET creation_source = 'legacy';

CREATE TABLE devrail_task_dependency_mutations (
    id BIGSERIAL PRIMARY KEY,
    organization_id BIGINT NOT NULL REFERENCES organizations (id),
    department_id BIGINT,
    owner_user_id BIGINT NOT NULL REFERENCES users (id),
    task_id BIGINT NOT NULL,
    idempotency_key VARCHAR(128) NOT NULL,
    request_digest CHAR(64) NOT NULL CHECK (request_digest ~ '^[0-9a-f]{64}$'),
    result_revision BIGINT NOT NULL CHECK (result_revision > 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (id, organization_id),
    UNIQUE (organization_id, task_id, idempotency_key),
    FOREIGN KEY (task_id, organization_id)
        REFERENCES devrail_tasks (id, organization_id),
    FOREIGN KEY (department_id, organization_id)
        REFERENCES departments (id, organization_id)
);

CREATE TABLE devrail_task_dependencies (
    id BIGSERIAL PRIMARY KEY,
    organization_id BIGINT NOT NULL REFERENCES organizations (id),
    department_id BIGINT,
    owner_user_id BIGINT NOT NULL REFERENCES users (id),
    task_id BIGINT NOT NULL,
    prerequisite_task_id BIGINT NOT NULL,
    failure_action VARCHAR(8) NOT NULL DEFAULT 'wait'
        CHECK (failure_action IN ('wait', 'skip', 'fail')),
    cancelled_action VARCHAR(8) NOT NULL DEFAULT 'wait'
        CHECK (cancelled_action IN ('wait', 'skip', 'fail')),
    timeout_action VARCHAR(8) NOT NULL DEFAULT 'wait'
        CHECK (timeout_action IN ('wait', 'skip', 'fail')),
    creation_source VARCHAR(24) NOT NULL DEFAULT 'manual'
        CHECK (creation_source IN ('manual', 'agent_followup', 'system')),
    created_by_type VARCHAR(16) NOT NULL
        CHECK (created_by_type IN ('user', 'system', 'agent')),
    created_by_user_id BIGINT REFERENCES users (id),
    mutation_id BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (id, organization_id),
    UNIQUE (organization_id, task_id, prerequisite_task_id),
    FOREIGN KEY (task_id, organization_id)
        REFERENCES devrail_tasks (id, organization_id) ON DELETE CASCADE,
    FOREIGN KEY (prerequisite_task_id, organization_id)
        REFERENCES devrail_tasks (id, organization_id) ON DELETE CASCADE,
    FOREIGN KEY (mutation_id, organization_id)
        REFERENCES devrail_task_dependency_mutations (id, organization_id),
    FOREIGN KEY (department_id, organization_id)
        REFERENCES departments (id, organization_id),
    CHECK (task_id <> prerequisite_task_id),
    CHECK (
        (created_by_type = 'user' AND created_by_user_id IS NOT NULL)
        OR created_by_type <> 'user'
    )
);

CREATE INDEX idx_devrail_task_dependencies_downstream
    ON devrail_task_dependencies (organization_id, task_id, prerequisite_task_id);
CREATE INDEX idx_devrail_task_dependencies_upstream
    ON devrail_task_dependencies (organization_id, prerequisite_task_id, task_id);

CREATE TABLE devrail_task_events (
    id BIGSERIAL PRIMARY KEY,
    organization_id BIGINT NOT NULL REFERENCES organizations (id),
    department_id BIGINT,
    owner_user_id BIGINT NOT NULL REFERENCES users (id),
    task_id BIGINT NOT NULL,
    cursor BIGINT NOT NULL,
    event_type VARCHAR(64) NOT NULL,
    idempotency_key VARCHAR(256) NOT NULL,
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    summary TEXT,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (id, organization_id),
    UNIQUE (task_id, cursor),
    UNIQUE (task_id, idempotency_key),
    FOREIGN KEY (task_id, organization_id)
        REFERENCES devrail_tasks (id, organization_id) ON DELETE CASCADE,
    FOREIGN KEY (department_id, organization_id)
        REFERENCES departments (id, organization_id),
    CHECK (octet_length(payload::text) <= 65536),
    CHECK (summary IS NULL OR octet_length(summary) <= 2048)
);

CREATE INDEX idx_devrail_task_events_cursor
    ON devrail_task_events (organization_id, task_id, cursor);

CREATE TABLE devrail_followup_requests (
    id BIGSERIAL PRIMARY KEY,
    organization_id BIGINT NOT NULL REFERENCES organizations (id),
    department_id BIGINT,
    owner_user_id BIGINT NOT NULL REFERENCES users (id),
    source_task_id BIGINT NOT NULL,
    source_run_id BIGINT NOT NULL,
    idempotency_key VARCHAR(128) NOT NULL,
    request_digest CHAR(64) NOT NULL CHECK (request_digest ~ '^[0-9a-f]{64}$'),
    status VARCHAR(16) NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'succeeded', 'failed')),
    result_task_id BIGINT,
    error_code VARCHAR(64),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at TIMESTAMPTZ,
    UNIQUE (id, organization_id),
    UNIQUE (organization_id, source_run_id, idempotency_key),
    FOREIGN KEY (source_task_id, organization_id)
        REFERENCES devrail_tasks (id, organization_id),
    FOREIGN KEY (source_run_id, organization_id)
        REFERENCES devrail_runs (id, organization_id),
    FOREIGN KEY (result_task_id, organization_id)
        REFERENCES devrail_tasks (id, organization_id),
    FOREIGN KEY (department_id, organization_id)
        REFERENCES departments (id, organization_id),
    CHECK (
        (status = 'succeeded' AND result_task_id IS NOT NULL AND completed_at IS NOT NULL)
        OR status <> 'succeeded'
    )
);

CREATE INDEX idx_devrail_followup_requests_quota
    ON devrail_followup_requests (organization_id, source_run_id, status, created_at);

CREATE TABLE devrail_dependency_propagations (
    id BIGSERIAL PRIMARY KEY,
    organization_id BIGINT NOT NULL REFERENCES organizations (id),
    department_id BIGINT,
    owner_user_id BIGINT NOT NULL REFERENCES users (id),
    dependency_id BIGINT NOT NULL,
    source_status_history_id BIGINT NOT NULL,
    action VARCHAR(8) NOT NULL CHECK (action IN ('wait', 'skip', 'fail')),
    result_status VARCHAR(24) NOT NULL,
    trace_id VARCHAR(128) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (id, organization_id),
    UNIQUE (organization_id, dependency_id, source_status_history_id, action),
    FOREIGN KEY (dependency_id, organization_id)
        REFERENCES devrail_task_dependencies (id, organization_id) ON DELETE CASCADE,
    FOREIGN KEY (source_status_history_id, organization_id)
        REFERENCES devrail_task_status_history (id, organization_id),
    FOREIGN KEY (department_id, organization_id)
        REFERENCES departments (id, organization_id)
);

CREATE INDEX idx_devrail_dependency_propagations_source
    ON devrail_dependency_propagations
       (organization_id, source_status_history_id, dependency_id);

INSERT INTO devrail_task_status_history (
    organization_id, department_id, owner_user_id, task_id, task_revision,
    from_status, to_status, actor_type, actor_user_id, reason, trace_id
)
SELECT t.organization_id, t.department_id, t.owner_user_id, t.id, t.revision,
       NULL, t.status, 'system', NULL, 'dependency_migration_baseline', NULL
FROM devrail_tasks t
WHERE NOT EXISTS (
    SELECT 1 FROM devrail_task_status_history h
    WHERE h.organization_id = t.organization_id AND h.task_id = t.id
);

-- A Scheduler/System Actor has no impersonated user.  The original trigger
-- predates dependency propagation and therefore fell back to the task owner.
CREATE OR REPLACE FUNCTION devrail_record_task_status_history()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    configured_actor_type TEXT;
    configured_actor_user_id BIGINT;
    configured_reason TEXT;
    configured_trace TEXT;
    effective_actor_type TEXT;
BEGIN
    IF OLD.status IS NOT DISTINCT FROM NEW.status THEN
        RETURN NEW;
    END IF;

    configured_actor_type := NULLIF(current_setting('devrail.actor_type', true), '');
    configured_actor_user_id := NULLIF(current_setting('devrail.actor_user_id', true), '')::BIGINT;
    configured_reason := NULLIF(current_setting('devrail.transition_reason', true), '');
    configured_trace := NULLIF(current_setting('devrail.trace_id', true), '');
    effective_actor_type := COALESCE(
        configured_actor_type,
        CASE WHEN NEW.scheduler_claim_token IS NOT NULL THEN 'system' ELSE 'user' END
    );

    INSERT INTO devrail_task_status_history (
        organization_id, department_id, owner_user_id, task_id, task_revision,
        from_status, to_status, actor_type, actor_user_id, reason, trace_id
    ) VALUES (
        NEW.organization_id, NEW.department_id, NEW.owner_user_id, NEW.id, NEW.revision,
        OLD.status, NEW.status,
        effective_actor_type,
        CASE
            WHEN effective_actor_type = 'system' THEN configured_actor_user_id
            ELSE COALESCE(configured_actor_user_id, NEW.owner_user_id)
        END,
        COALESCE(configured_reason, 'task_status_updated'),
        configured_trace
    );
    RETURN NEW;
END;
$$;
