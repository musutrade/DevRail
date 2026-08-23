-- Project membership with organization, department, and owner boundaries.
INSERT INTO permissions (group_id, code, name, type, description, sort_order)
VALUES
    ((SELECT id FROM permission_groups WHERE code = 'devrail'), 'devrail:member:read', '查看项目成员', 'api', '允许查看项目成员。', 25),
    ((SELECT id FROM permission_groups WHERE code = 'devrail'), 'devrail:member:write', '管理项目成员', 'api', '允许添加和移除项目成员。', 26)
ON CONFLICT (code) DO UPDATE SET name = EXCLUDED.name, description = EXCLUDED.description;

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id FROM roles r JOIN permissions p ON p.code IN ('devrail:member:read', 'devrail:member:write')
WHERE r.code = 'super_admin' ON CONFLICT DO NOTHING;

CREATE TABLE devrail_project_members (
    id BIGSERIAL PRIMARY KEY,
    organization_id BIGINT NOT NULL REFERENCES organizations (id),
    department_id BIGINT,
    owner_user_id BIGINT NOT NULL REFERENCES users (id),
    project_id BIGINT NOT NULL,
    user_id BIGINT NOT NULL REFERENCES users (id),
    role VARCHAR(16) NOT NULL DEFAULT 'developer' CHECK (role IN ('owner', 'admin', 'developer', 'observer')),
    joined_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at TIMESTAMPTZ,
    UNIQUE (project_id, user_id),
    FOREIGN KEY (project_id, organization_id) REFERENCES devrail_projects (id, organization_id),
    FOREIGN KEY (department_id, organization_id) REFERENCES departments (id, organization_id)
);

CREATE INDEX idx_devrail_project_members_scope ON devrail_project_members (organization_id, project_id, user_id, revoked_at);
