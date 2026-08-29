-- Controlled, scoped run artifacts. Storage keys are relative references only;
-- bytes remain under the configured controlled artifact root.
INSERT INTO permissions (group_id, code, name, type, description, sort_order)
VALUES
    ((SELECT id FROM permission_groups WHERE code = 'devrail'),
     'devrail:artifact:read', '查看运行产物', 'api',
     '允许查看数据范围内的运行产物元数据和下载内容。', 150)
ON CONFLICT (code) DO UPDATE
SET group_id = EXCLUDED.group_id,
    name = EXCLUDED.name,
    type = EXCLUDED.type,
    description = EXCLUDED.description,
    sort_order = EXCLUDED.sort_order;

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code = 'devrail:artifact:read'
WHERE r.code IN ('super_admin', 'organization_admin', 'project_admin', 'developer', 'reviewer', 'observer')
ON CONFLICT DO NOTHING;

CREATE TABLE devrail_artifacts (
    id BIGSERIAL PRIMARY KEY,
    organization_id BIGINT NOT NULL REFERENCES organizations (id),
    department_id BIGINT,
    owner_user_id BIGINT NOT NULL REFERENCES users (id),
    project_id BIGINT NOT NULL,
    task_id BIGINT NOT NULL,
    run_id BIGINT,
    quality_gate_id VARCHAR(128),
    artifact_type VARCHAR(32) NOT NULL
        CHECK (artifact_type IN ('test_report', 'patch', 'screenshot', 'video', 'trace', 'diagnosis', 'log', 'other')),
    storage_key VARCHAR(512) NOT NULL
        CHECK (storage_key <> '' AND storage_key NOT LIKE '/%' AND storage_key NOT LIKE '%..%'),
    file_name VARCHAR(256) NOT NULL,
    content_type VARCHAR(128) NOT NULL DEFAULT 'application/octet-stream',
    byte_size BIGINT NOT NULL CHECK (byte_size >= 0 AND byte_size <= 16777216),
    sha256 CHAR(64) NOT NULL CHECK (sha256 ~ '^[0-9a-f]{64}$'),
    summary VARCHAR(1000),
    cleanup_status VARCHAR(16) NOT NULL DEFAULT 'pending'
        CHECK (cleanup_status IN ('pending', 'running', 'deleted', 'failed')),
    cleanup_attempts INTEGER NOT NULL DEFAULT 0 CHECK (cleanup_attempts >= 0),
    next_cleanup_at TIMESTAMPTZ,
    last_cleanup_error VARCHAR(500),
    expires_at TIMESTAMPTZ NOT NULL,
    deleted_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (id, organization_id),
    UNIQUE (organization_id, storage_key),
    FOREIGN KEY (project_id, organization_id)
        REFERENCES devrail_projects (id, organization_id),
    FOREIGN KEY (task_id, organization_id)
        REFERENCES devrail_tasks (id, organization_id),
    FOREIGN KEY (run_id, organization_id)
        REFERENCES devrail_runs (id, organization_id),
    FOREIGN KEY (department_id, organization_id)
        REFERENCES departments (id, organization_id)
);

CREATE INDEX idx_devrail_artifacts_scope
    ON devrail_artifacts (organization_id, department_id, owner_user_id, created_at DESC);
CREATE INDEX idx_devrail_artifacts_run
    ON devrail_artifacts (organization_id, run_id, created_at DESC);
CREATE INDEX idx_devrail_artifacts_cleanup
    ON devrail_artifacts (cleanup_status, next_cleanup_at, expires_at, updated_at);
