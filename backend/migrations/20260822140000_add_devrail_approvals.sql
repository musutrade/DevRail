-- DevRail approval requests and immutable decision history.
INSERT INTO permissions (group_id, code, name, type, description, sort_order)
VALUES
    ((SELECT id FROM permission_groups WHERE code = 'devrail'), 'devrail:approval:read', '查看 Harness 审批', 'menu', '允许查看数据范围内的审批请求。', 130),
    ((SELECT id FROM permission_groups WHERE code = 'devrail'), 'devrail:approval:approve', '批准 Harness 审批', 'api', '允许批准高风险工具或命令。', 140),
    ((SELECT id FROM permission_groups WHERE code = 'devrail'), 'devrail:approval:reject', '拒绝 Harness 审批', 'api', '允许拒绝高风险工具或命令。', 150)
ON CONFLICT (code) DO UPDATE
SET group_id = EXCLUDED.group_id, name = EXCLUDED.name, type = EXCLUDED.type,
    description = EXCLUDED.description, sort_order = EXCLUDED.sort_order;

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id FROM roles r JOIN permissions p ON p.code LIKE 'devrail:approval:%'
WHERE r.code = 'super_admin' ON CONFLICT DO NOTHING;

CREATE TABLE devrail_approvals (
    id BIGSERIAL PRIMARY KEY,
    organization_id BIGINT NOT NULL REFERENCES organizations (id),
    department_id BIGINT,
    owner_user_id BIGINT NOT NULL REFERENCES users (id),
    run_id BIGINT NOT NULL,
    event_id BIGINT,
    idempotency_key VARCHAR(256) NOT NULL,
    tool_name VARCHAR(256) NOT NULL,
    args_summary JSONB NOT NULL DEFAULT '{}'::jsonb,
    cwd TEXT NOT NULL,
    impact_scope TEXT,
    risk_level VARCHAR(16) NOT NULL CHECK (risk_level IN ('low','medium','high','critical')),
    requested_by BIGINT NOT NULL REFERENCES users (id),
    decided_by BIGINT REFERENCES users (id),
    status VARCHAR(16) NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','approved','rejected','expired','cancelled')),
    decision_reason TEXT,
    expires_at TIMESTAMPTZ NOT NULL,
    policy_version VARCHAR(128),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (id, organization_id),
    UNIQUE (run_id, idempotency_key),
    FOREIGN KEY (run_id, organization_id) REFERENCES devrail_runs (id, organization_id),
    FOREIGN KEY (department_id, organization_id) REFERENCES departments (id, organization_id),
    FOREIGN KEY (event_id) REFERENCES devrail_run_events (id)
);

CREATE TABLE devrail_approval_decisions (
    id BIGSERIAL PRIMARY KEY,
    organization_id BIGINT NOT NULL REFERENCES organizations (id),
    department_id BIGINT,
    owner_user_id BIGINT NOT NULL REFERENCES users (id),
    approval_id BIGINT NOT NULL,
    decided_by BIGINT REFERENCES users (id),
    decision VARCHAR(16) NOT NULL CHECK (decision IN ('approved','rejected','expired','cancelled')),
    reason TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (approval_id, organization_id) REFERENCES devrail_approvals (id, organization_id),
    FOREIGN KEY (department_id, organization_id) REFERENCES departments (id, organization_id)
);

CREATE INDEX idx_devrail_approvals_scope ON devrail_approvals (organization_id, status, expires_at, department_id, owner_user_id);
CREATE INDEX idx_devrail_approval_decisions_approval ON devrail_approval_decisions (approval_id, created_at DESC);
