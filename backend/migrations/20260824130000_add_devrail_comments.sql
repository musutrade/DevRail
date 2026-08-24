CREATE TABLE devrail_task_comments (
    id BIGSERIAL PRIMARY KEY,
    organization_id BIGINT NOT NULL REFERENCES organizations (id),
    department_id BIGINT,
    task_id BIGINT NOT NULL,
    author_user_id BIGINT NOT NULL REFERENCES users (id),
    body TEXT NOT NULL CHECK (char_length(body) BETWEEN 1 AND 10000),
    mentions JSONB NOT NULL DEFAULT '[]'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (id, organization_id),
    FOREIGN KEY (task_id, organization_id) REFERENCES devrail_tasks (id, organization_id),
    FOREIGN KEY (department_id, organization_id) REFERENCES departments (id, organization_id)
);
CREATE INDEX idx_devrail_task_comments_scope ON devrail_task_comments (organization_id, task_id, created_at DESC, id DESC);

INSERT INTO permissions (group_id, code, name, type, description, sort_order)
VALUES
    ((SELECT id FROM permission_groups WHERE code = 'devrail'), 'devrail:comment:read', '查看任务评论', 'api', '允许查看数据范围内的任务评论。', 180),
    ((SELECT id FROM permission_groups WHERE code = 'devrail'), 'devrail:comment:write', '发布任务评论', 'api', '允许发布任务评论并提及用户。', 190)
ON CONFLICT (code) DO NOTHING;
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id FROM roles r CROSS JOIN permissions p
WHERE r.code IN ('organization_admin', 'project_admin', 'developer', 'reviewer')
  AND p.code IN ('devrail:comment:read', 'devrail:comment:write')
ON CONFLICT DO NOTHING;
