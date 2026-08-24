INSERT INTO permissions (group_id, code, name, type, description, sort_order)
VALUES
    ((SELECT id FROM permission_groups WHERE code='devrail'), 'devrail:review:read', '查看变更审查', 'api', '允许查看数据范围内的变更审查。', 200),
    ((SELECT id FROM permission_groups WHERE code='devrail'), 'devrail:review:write', '管理变更审查', 'api', '允许创建和处理变更审查。', 210)
ON CONFLICT (code) DO NOTHING;
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id FROM roles r CROSS JOIN permissions p
WHERE r.code IN ('organization_admin','project_admin','developer','reviewer')
  AND p.code IN ('devrail:review:read','devrail:review:write') ON CONFLICT DO NOTHING;

CREATE TABLE devrail_reviews (
    id BIGSERIAL PRIMARY KEY,
    organization_id BIGINT NOT NULL REFERENCES organizations(id),
    department_id BIGINT,
    task_id BIGINT NOT NULL,
    run_id BIGINT NOT NULL,
    requested_by BIGINT NOT NULL REFERENCES users(id),
    reviewer_user_id BIGINT NOT NULL REFERENCES users(id),
    status VARCHAR(16) NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','approved','rejected','cancelled')),
    summary TEXT,
    decision_reason TEXT,
    decided_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (organization_id, run_id, reviewer_user_id),
    FOREIGN KEY (task_id, organization_id) REFERENCES devrail_tasks(id, organization_id),
    FOREIGN KEY (run_id, organization_id) REFERENCES devrail_runs(id, organization_id),
    FOREIGN KEY (department_id, organization_id) REFERENCES departments(id, organization_id)
);
CREATE INDEX idx_devrail_reviews_scope ON devrail_reviews (organization_id, reviewer_user_id, status, created_at DESC);
