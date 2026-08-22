-- DevRail Phase 0 business data model and permissions.
-- All rows carry organization, department and owner boundaries.  Child resources
-- retain a project foreign key so every query can enforce the same scope.

INSERT INTO permission_groups (code, name, icon, sort_order)
VALUES ('devrail', 'DevRail Harness', 'terminal', 80)
ON CONFLICT (code) DO UPDATE SET name = EXCLUDED.name, icon = EXCLUDED.icon;

INSERT INTO permissions (group_id, code, name, type, description, sort_order)
VALUES
    ((SELECT id FROM permission_groups WHERE code = 'devrail'), 'devrail:project:read', '查看 DevRail 项目', 'menu', '允许查看项目及其资源。', 10),
    ((SELECT id FROM permission_groups WHERE code = 'devrail'), 'devrail:project:write', '管理 DevRail 项目', 'api', '允许创建、修改和归档项目。', 20),
    ((SELECT id FROM permission_groups WHERE code = 'devrail'), 'devrail:repository:read', '查看代码仓库', 'menu', '允许查看项目代码仓库。', 30),
    ((SELECT id FROM permission_groups WHERE code = 'devrail'), 'devrail:repository:write', '管理代码仓库', 'api', '允许创建、修改和归档代码仓库。', 40),
    ((SELECT id FROM permission_groups WHERE code = 'devrail'), 'devrail:environment:read', '查看运行环境', 'menu', '允许查看项目运行环境。', 50),
    ((SELECT id FROM permission_groups WHERE code = 'devrail'), 'devrail:environment:write', '管理运行环境', 'api', '允许创建、修改和归档运行环境。', 60),
    ((SELECT id FROM permission_groups WHERE code = 'devrail'), 'devrail:task:read', '查看开发任务', 'menu', '允许查看开发任务。', 70),
    ((SELECT id FROM permission_groups WHERE code = 'devrail'), 'devrail:task:write', '管理开发任务', 'api', '允许创建、修改和归档开发任务。', 80)
ON CONFLICT (code) DO UPDATE
SET group_id = EXCLUDED.group_id, name = EXCLUDED.name, type = EXCLUDED.type,
    description = EXCLUDED.description, sort_order = EXCLUDED.sort_order;

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code LIKE 'devrail:%'
WHERE r.code = 'super_admin'
ON CONFLICT DO NOTHING;

CREATE TABLE devrail_projects (
    id BIGSERIAL PRIMARY KEY,
    organization_id BIGINT NOT NULL REFERENCES organizations (id),
    department_id BIGINT,
    owner_user_id BIGINT NOT NULL REFERENCES users (id),
    slug VARCHAR(64) NOT NULL,
    name VARCHAR(128) NOT NULL,
    description TEXT,
    status VARCHAR(16) NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'archived')),
    default_repository_id BIGINT,
    default_environment_id BIGINT,
    notification_policy JSONB NOT NULL DEFAULT '{}'::jsonb,
    quality_gate_template JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    archived_at TIMESTAMPTZ,
    UNIQUE (organization_id, slug),
    UNIQUE (id, organization_id),
    FOREIGN KEY (department_id, organization_id) REFERENCES departments (id, organization_id)
);

CREATE TABLE devrail_repositories (
    id BIGSERIAL PRIMARY KEY,
    organization_id BIGINT NOT NULL REFERENCES organizations (id),
    department_id BIGINT,
    owner_user_id BIGINT NOT NULL REFERENCES users (id),
    project_id BIGINT NOT NULL,
    name VARCHAR(128) NOT NULL,
    remote_url TEXT NOT NULL,
    protocol VARCHAR(16) NOT NULL CHECK (protocol IN ('https', 'ssh')),
    default_branch VARCHAR(128) NOT NULL DEFAULT 'main',
    credential_ref VARCHAR(128),
    last_sync_status VARCHAR(32) NOT NULL DEFAULT 'unknown',
    last_head_sha VARCHAR(128),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    archived_at TIMESTAMPTZ,
    UNIQUE (project_id, name),
    UNIQUE (id, organization_id),
    FOREIGN KEY (project_id, organization_id) REFERENCES devrail_projects (id, organization_id),
    FOREIGN KEY (department_id, organization_id) REFERENCES departments (id, organization_id)
);

CREATE TABLE devrail_environments (
    id BIGSERIAL PRIMARY KEY,
    organization_id BIGINT NOT NULL REFERENCES organizations (id),
    department_id BIGINT,
    owner_user_id BIGINT NOT NULL REFERENCES users (id),
    project_id BIGINT NOT NULL,
    name VARCHAR(128) NOT NULL,
    workspace_root TEXT NOT NULL,
    network_mode VARCHAR(16) NOT NULL DEFAULT 'off' CHECK (network_mode IN ('off', 'allowlist')),
    tool_policy JSONB NOT NULL DEFAULT '{}'::jsonb,
    secret_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    max_duration_secs BIGINT NOT NULL DEFAULT 3600 CHECK (max_duration_secs BETWEEN 60 AND 86400),
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    archived_at TIMESTAMPTZ,
    UNIQUE (project_id, name),
    UNIQUE (id, organization_id),
    FOREIGN KEY (project_id, organization_id) REFERENCES devrail_projects (id, organization_id),
    FOREIGN KEY (department_id, organization_id) REFERENCES departments (id, organization_id)
);

CREATE TABLE devrail_tasks (
    id BIGSERIAL PRIMARY KEY,
    organization_id BIGINT NOT NULL REFERENCES organizations (id),
    department_id BIGINT,
    owner_user_id BIGINT NOT NULL REFERENCES users (id),
    project_id BIGINT NOT NULL,
    assignee_user_id BIGINT REFERENCES users (id),
    title VARCHAR(200) NOT NULL,
    goal TEXT NOT NULL,
    background TEXT,
    acceptance_criteria TEXT,
    constraints TEXT,
    priority VARCHAR(16) NOT NULL DEFAULT 'normal' CHECK (priority IN ('low', 'normal', 'high', 'urgent')),
    status VARCHAR(24) NOT NULL DEFAULT 'draft' CHECK (status IN ('draft', 'queued', 'running', 'awaiting_approval', 'succeeded', 'failed', 'cancelled', 'archived')),
    labels JSONB NOT NULL DEFAULT '[]'::jsonb,
    due_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    archived_at TIMESTAMPTZ,
    UNIQUE (id, organization_id),
    FOREIGN KEY (project_id, organization_id) REFERENCES devrail_projects (id, organization_id),
    FOREIGN KEY (department_id, organization_id) REFERENCES departments (id, organization_id)
);

ALTER TABLE devrail_projects
    ADD CONSTRAINT fk_devrail_projects_default_repository
        FOREIGN KEY (default_repository_id, organization_id)
        REFERENCES devrail_repositories (id, organization_id),
    ADD CONSTRAINT fk_devrail_projects_default_environment
        FOREIGN KEY (default_environment_id, organization_id)
        REFERENCES devrail_environments (id, organization_id);

CREATE INDEX idx_devrail_projects_scope ON devrail_projects (organization_id, department_id, owner_user_id, updated_at DESC);
CREATE INDEX idx_devrail_repositories_scope ON devrail_repositories (organization_id, project_id, department_id, updated_at DESC);
CREATE INDEX idx_devrail_environments_scope ON devrail_environments (organization_id, project_id, department_id, updated_at DESC);
CREATE INDEX idx_devrail_tasks_scope ON devrail_tasks (organization_id, project_id, department_id, owner_user_id, updated_at DESC);
