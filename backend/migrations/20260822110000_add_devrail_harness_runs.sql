-- DevRail Harness Supervisor run snapshots and append-only event journal.
INSERT INTO permissions (group_id, code, name, type, description, sort_order)
VALUES
    ((SELECT id FROM permission_groups WHERE code = 'devrail'), 'devrail:run:read', '查看 Harness 运行', 'menu', '允许查看运行状态和事件。', 90),
    ((SELECT id FROM permission_groups WHERE code = 'devrail'), 'devrail:run:execute', '执行 Harness 运行', 'api', '允许启动 Harness 运行。', 100),
    ((SELECT id FROM permission_groups WHERE code = 'devrail'), 'devrail:run:interrupt', '中断 Harness 运行', 'api', '允许中断活动运行。', 110),
    ((SELECT id FROM permission_groups WHERE code = 'devrail'), 'devrail:run:retry', '重试 Harness 运行', 'api', '允许从快照创建新运行。', 120)
ON CONFLICT (code) DO UPDATE
SET group_id = EXCLUDED.group_id, name = EXCLUDED.name, type = EXCLUDED.type,
    description = EXCLUDED.description, sort_order = EXCLUDED.sort_order;

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id FROM roles r JOIN permissions p ON p.code LIKE 'devrail:run:%'
WHERE r.code = 'super_admin' ON CONFLICT DO NOTHING;

CREATE TABLE devrail_task_snapshots (
    id BIGSERIAL PRIMARY KEY,
    organization_id BIGINT NOT NULL REFERENCES organizations (id),
    department_id BIGINT,
    owner_user_id BIGINT NOT NULL REFERENCES users (id),
    task_id BIGINT NOT NULL,
    snapshot JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (id, organization_id),
    FOREIGN KEY (task_id, organization_id) REFERENCES devrail_tasks (id, organization_id),
    FOREIGN KEY (department_id, organization_id) REFERENCES departments (id, organization_id)
);

CREATE TABLE devrail_runs (
    id BIGSERIAL PRIMARY KEY,
    organization_id BIGINT NOT NULL REFERENCES organizations (id),
    department_id BIGINT,
    owner_user_id BIGINT NOT NULL REFERENCES users (id),
    task_id BIGINT NOT NULL,
    snapshot_id BIGINT NOT NULL REFERENCES devrail_task_snapshots (id),
    idempotency_key VARCHAR(128) NOT NULL,
    status VARCHAR(32) NOT NULL DEFAULT 'created' CHECK (status IN ('created','starting','active','awaiting_approval','completed','failed','cancelled')),
    thread_id VARCHAR(256),
    turn_id VARCHAR(256),
    harness_version VARCHAR(128),
    model_id VARCHAR(128),
    cwd TEXT NOT NULL,
    policy JSONB NOT NULL DEFAULT '{}'::jsonb,
    startup_args_summary JSONB NOT NULL DEFAULT '[]'::jsonb,
    exit_reason VARCHAR(64),
    exit_code INTEGER,
    stderr_summary TEXT,
    trace_id VARCHAR(128),
    recovery_suggestion TEXT,
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (id, organization_id),
    UNIQUE (organization_id, task_id, idempotency_key),
    FOREIGN KEY (task_id, organization_id) REFERENCES devrail_tasks (id, organization_id),
    FOREIGN KEY (department_id, organization_id) REFERENCES departments (id, organization_id)
);

CREATE UNIQUE INDEX uq_devrail_active_run_per_task ON devrail_runs (task_id)
WHERE status IN ('starting', 'active', 'awaiting_approval');

CREATE TABLE devrail_run_events (
    id BIGSERIAL PRIMARY KEY,
    organization_id BIGINT NOT NULL REFERENCES organizations (id),
    department_id BIGINT,
    owner_user_id BIGINT NOT NULL REFERENCES users (id),
    run_id BIGINT NOT NULL,
    cursor BIGINT NOT NULL,
    event_type VARCHAR(64) NOT NULL,
    source_event_id VARCHAR(256),
    idempotency_key VARCHAR(256) NOT NULL,
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    summary TEXT,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (run_id, cursor),
    UNIQUE (run_id, idempotency_key),
    FOREIGN KEY (run_id, organization_id) REFERENCES devrail_runs (id, organization_id),
    FOREIGN KEY (department_id, organization_id) REFERENCES departments (id, organization_id)
);
CREATE INDEX idx_devrail_runs_scope ON devrail_runs (organization_id, task_id, department_id, owner_user_id, created_at DESC);
CREATE INDEX idx_devrail_run_events_cursor ON devrail_run_events (run_id, cursor);
