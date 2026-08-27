-- Persistent task/attempt workspaces.  Historical runs remain readable because
-- the run foreign key is nullable and no backfill is performed.

INSERT INTO permissions (group_id, code, name, type, description, sort_order)
VALUES
    ((SELECT id FROM permission_groups WHERE code = 'devrail'),
     'devrail:workspace:read', '查看任务工作区', 'api',
     '允许查看数据范围内任务工作区状态和诊断。', 130),
    ((SELECT id FROM permission_groups WHERE code = 'devrail'),
     'devrail:workspace:write', '管理任务工作区', 'api',
     '允许重建或清理数据范围内任务工作区。', 140)
ON CONFLICT (code) DO UPDATE
SET group_id = EXCLUDED.group_id, name = EXCLUDED.name, type = EXCLUDED.type,
    description = EXCLUDED.description, sort_order = EXCLUDED.sort_order;

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code IN ('devrail:workspace:read', 'devrail:workspace:write')
WHERE r.code = 'super_admin'
ON CONFLICT DO NOTHING;

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code = 'devrail:workspace:read'
WHERE r.code IN ('organization_admin', 'project_admin', 'developer', 'reviewer', 'observer')
ON CONFLICT DO NOTHING;

CREATE TABLE devrail_task_workspaces (
    id BIGSERIAL PRIMARY KEY,
    organization_id BIGINT NOT NULL REFERENCES organizations (id),
    department_id BIGINT,
    owner_user_id BIGINT NOT NULL REFERENCES users (id),
    task_id BIGINT NOT NULL,
    run_id BIGINT,
    attempt INTEGER NOT NULL CHECK (attempt > 0),
    workspace_key VARCHAR(160) NOT NULL,
    relative_path VARCHAR(512) NOT NULL,
    path_digest CHAR(64) NOT NULL CHECK (path_digest ~ '^[0-9a-f]{64}$'),
    repository_id BIGINT,
    environment_id BIGINT,
    base_commit VARCHAR(128),
    branch_name VARCHAR(256),
    workflow_version VARCHAR(128),
    workflow_digest CHAR(64),
    environment_version VARCHAR(128),
    tool_versions JSONB NOT NULL DEFAULT '{}'::jsonb,
    snapshot_digest CHAR(64),
    lifecycle_status VARCHAR(24) NOT NULL DEFAULT 'preparing'
        CHECK (lifecycle_status IN ('preparing', 'ready', 'running', 'cleanup_pending', 'cleanup_failed', 'cleaned', 'orphaned')),
    cleanup_status VARCHAR(16) NOT NULL DEFAULT 'pending'
        CHECK (cleanup_status IN ('pending', 'completed', 'failed')),
    cleanup_attempts INTEGER NOT NULL DEFAULT 0 CHECK (cleanup_attempts >= 0),
    next_cleanup_at TIMESTAMPTZ,
    last_hook VARCHAR(32),
    diagnostic_ref VARCHAR(128),
    error_summary TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    cleaned_at TIMESTAMPTZ,
    UNIQUE (id, organization_id),
    UNIQUE (organization_id, workspace_key),
    UNIQUE (organization_id, task_id, attempt),
    UNIQUE (run_id),
    FOREIGN KEY (task_id, organization_id)
        REFERENCES devrail_tasks (id, organization_id),
    FOREIGN KEY (run_id, organization_id)
        REFERENCES devrail_runs (id, organization_id),
    FOREIGN KEY (repository_id, organization_id)
        REFERENCES devrail_repositories (id, organization_id),
    FOREIGN KEY (environment_id, organization_id)
        REFERENCES devrail_environments (id, organization_id),
    FOREIGN KEY (department_id, organization_id)
        REFERENCES departments (id, organization_id)
);

CREATE INDEX idx_devrail_task_workspaces_scope
    ON devrail_task_workspaces (organization_id, department_id, owner_user_id, updated_at DESC);
CREATE INDEX idx_devrail_task_workspaces_cleanup
    ON devrail_task_workspaces (lifecycle_status, cleanup_status, next_cleanup_at, updated_at);
CREATE INDEX idx_devrail_task_workspaces_task
    ON devrail_task_workspaces (organization_id, task_id, attempt DESC);
