-- Add organization ownership and role-driven row-level data scopes.

CREATE TABLE organizations (
    id         BIGSERIAL PRIMARY KEY,
    code       VARCHAR(64)  NOT NULL UNIQUE,
    name       VARCHAR(128) NOT NULL,
    status     VARCHAR(16)  NOT NULL DEFAULT 'active'
               CHECK (status IN ('active', 'inactive')),
    created_at TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ  NOT NULL DEFAULT now()
);

CREATE TABLE departments (
    id              BIGSERIAL PRIMARY KEY,
    organization_id BIGINT       NOT NULL REFERENCES organizations (id),
    parent_id       BIGINT,
    code            VARCHAR(64)  NOT NULL,
    name            VARCHAR(128) NOT NULL,
    status          VARCHAR(16)  NOT NULL DEFAULT 'active'
                    CHECK (status IN ('active', 'inactive')),
    created_at      TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ  NOT NULL DEFAULT now(),
    UNIQUE (organization_id, code),
    UNIQUE (id, organization_id),
    FOREIGN KEY (parent_id, organization_id)
        REFERENCES departments (id, organization_id)
);

INSERT INTO organizations (code, name)
VALUES ('default', '默认组织');

INSERT INTO departments (organization_id, code, name)
SELECT id, 'root', '根部门'
FROM organizations
WHERE code = 'default';

ALTER TABLE users
ADD COLUMN organization_id BIGINT,
ADD COLUMN department_id BIGINT;

UPDATE users
SET organization_id = (SELECT id FROM organizations WHERE code = 'default'),
    department_id = (
        SELECT d.id
        FROM departments d
        JOIN organizations o ON o.id = d.organization_id
        WHERE o.code = 'default' AND d.code = 'root'
    );

ALTER TABLE users
ALTER COLUMN organization_id SET NOT NULL,
ADD CONSTRAINT fk_users_organization
    FOREIGN KEY (organization_id) REFERENCES organizations (id),
ADD CONSTRAINT fk_users_department_organization
    FOREIGN KEY (department_id, organization_id)
    REFERENCES departments (id, organization_id);

CREATE INDEX idx_users_organization_active
    ON users (organization_id, id) WHERE deleted_at IS NULL;
CREATE INDEX idx_users_department_active
    ON users (organization_id, department_id, id) WHERE deleted_at IS NULL;
CREATE INDEX idx_departments_parent
    ON departments (organization_id, parent_id);

ALTER TABLE roles
ADD COLUMN data_scope VARCHAR(32) NOT NULL DEFAULT 'self'
    CHECK (data_scope IN (
        'all',
        'organization',
        'department_and_children',
        'department',
        'self'
    ));

UPDATE roles SET data_scope = CASE code
    WHEN 'super_admin' THEN 'all'
    WHEN 'compliance_auditor' THEN 'organization'
    WHEN 'support_tier2' THEN 'organization'
    WHEN 'billing_manager' THEN 'organization'
    WHEN 'editor' THEN 'department_and_children'
    ELSE 'self'
END;

ALTER TABLE audit_logs
ADD COLUMN organization_id BIGINT REFERENCES organizations (id),
ADD COLUMN department_id BIGINT REFERENCES departments (id);

UPDATE audit_logs a
SET organization_id = u.organization_id,
    department_id = u.department_id
FROM users u
WHERE u.id = a.actor_user_id;

CREATE INDEX idx_audit_logs_organization
    ON audit_logs (organization_id, created_at DESC, id DESC);
CREATE INDEX idx_audit_logs_department
    ON audit_logs (organization_id, department_id, created_at DESC, id DESC);
