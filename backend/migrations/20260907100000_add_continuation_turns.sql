-- Continuation request ledger, immutable run handoff evidence, and run lineage.
-- All changes are additive; historical tasks and runs receive compatible values.

ALTER TABLE devrail_tasks
    DROP CONSTRAINT devrail_tasks_status_check,
    ADD CONSTRAINT devrail_tasks_status_check
        CHECK (status IN (
            'draft', 'queued', 'running', 'awaiting_approval', 'continuation_pending',
            'succeeded', 'failed', 'cancelled', 'skipped', 'archived'
        ));

CREATE OR REPLACE FUNCTION devrail_guard_task_dispatch_snapshot()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    input_changed BOOLEAN;
BEGIN
    input_changed := ROW(
        NEW.title, NEW.goal, NEW.background, NEW.acceptance_criteria,
        NEW.constraints, NEW.repository_id, NEW.environment_id
    ) IS DISTINCT FROM ROW(
        OLD.title, OLD.goal, OLD.background, OLD.acceptance_criteria,
        OLD.constraints, OLD.repository_id, OLD.environment_id
    );

    IF OLD.status IN ('queued', 'running', 'awaiting_approval', 'continuation_pending')
       AND input_changed THEN
        RAISE EXCEPTION 'dispatched task inputs are immutable'
            USING ERRCODE = '23514';
    END IF;

    IF input_changed THEN
        NEW.revision := OLD.revision + 1;
    END IF;

    IF OLD.status IN ('queued', 'running', 'awaiting_approval', 'continuation_pending') AND (
        NEW.dispatch_snapshot IS DISTINCT FROM OLD.dispatch_snapshot
        OR NEW.dispatch_snapshot_digest IS DISTINCT FROM OLD.dispatch_snapshot_digest
        OR NEW.workflow_source IS DISTINCT FROM OLD.workflow_source
        OR NEW.workflow_version IS DISTINCT FROM OLD.workflow_version
        OR NEW.workflow_digest IS DISTINCT FROM OLD.workflow_digest
    ) THEN
        RAISE EXCEPTION 'dispatched task workflow snapshot is immutable'
            USING ERRCODE = '23514';
    END IF;

    RETURN NEW;
END;
$$;

ALTER TABLE devrail_runs
    ADD COLUMN run_kind VARCHAR(24) NOT NULL DEFAULT 'primary'
        CHECK (run_kind IN ('primary', 'retry', 'continuation', 'follow_up')),
    ADD COLUMN root_run_id BIGINT,
    ADD COLUMN continuation_sequence SMALLINT
        CHECK (continuation_sequence > 0),
    ADD COLUMN harness_start_key VARCHAR(160),
    ADD COLUMN harness_start_claim_owner VARCHAR(128),
    ADD COLUMN harness_start_claim_token UUID,
    ADD COLUMN harness_start_claim_expires_at TIMESTAMPTZ,
    ADD CONSTRAINT devrail_runs_harness_start_claim_check CHECK (
        (harness_start_claim_owner IS NULL
            AND harness_start_claim_token IS NULL
            AND harness_start_claim_expires_at IS NULL)
        OR (harness_start_claim_owner IS NOT NULL
            AND harness_start_claim_token IS NOT NULL
            AND harness_start_claim_expires_at IS NOT NULL)
    );

UPDATE devrail_runs AS runs
SET run_kind = 'follow_up'
FROM devrail_tasks AS tasks
WHERE tasks.id = runs.task_id
  AND tasks.organization_id = runs.organization_id
  AND tasks.creation_source = 'agent_followup';

UPDATE devrail_runs
SET run_kind = 'retry'
WHERE parent_run_id IS NOT NULL AND run_kind = 'primary';

WITH RECURSIVE run_roots AS (
    SELECT id, organization_id, id AS root_run_id
    FROM devrail_runs
    WHERE parent_run_id IS NULL
    UNION ALL
    SELECT child.id, child.organization_id, parent.root_run_id
    FROM devrail_runs AS child
    JOIN run_roots AS parent
      ON parent.id = child.parent_run_id
     AND parent.organization_id = child.organization_id
)
UPDATE devrail_runs AS runs
SET root_run_id = roots.root_run_id
FROM run_roots AS roots
WHERE roots.id = runs.id AND roots.organization_id = runs.organization_id;

UPDATE devrail_runs SET root_run_id = id WHERE root_run_id IS NULL;

ALTER TABLE devrail_runs
    ADD CONSTRAINT fk_devrail_runs_root
        FOREIGN KEY (root_run_id, organization_id)
        REFERENCES devrail_runs (id, organization_id);

CREATE UNIQUE INDEX uq_devrail_runs_harness_start_key
    ON devrail_runs (organization_id, harness_start_key)
    WHERE harness_start_key IS NOT NULL;
CREATE INDEX idx_devrail_runs_harness_start_claim
    ON devrail_runs (status, harness_start_claim_expires_at, id)
    WHERE harness_start_key IS NOT NULL AND status = 'starting';
CREATE INDEX idx_devrail_runs_lineage
    ON devrail_runs (organization_id, root_run_id, run_kind, attempt, id);

CREATE TABLE devrail_continuation_requests (
    id BIGSERIAL PRIMARY KEY,
    organization_id BIGINT NOT NULL REFERENCES organizations (id),
    department_id BIGINT,
    owner_user_id BIGINT NOT NULL REFERENCES users (id),
    project_id BIGINT NOT NULL,
    task_id BIGINT NOT NULL,
    source_run_id BIGINT NOT NULL,
    root_run_id BIGINT NOT NULL,
    source_turn_id VARCHAR(256) NOT NULL,
    requested_by_user_id BIGINT REFERENCES users (id),
    trigger_type VARCHAR(24) NOT NULL
        CHECK (trigger_type IN ('user_context', 'quality_gate', 'review_changes')),
    evidence_ref VARCHAR(256) NOT NULL,
    evidence_digest CHAR(64) NOT NULL
        CHECK (evidence_digest ~ '^[0-9a-f]{64}$'),
    evidence_observed_at TIMESTAMPTZ NOT NULL,
    evidence_expires_at TIMESTAMPTZ,
    changeset_digest CHAR(64)
        CHECK (changeset_digest IS NULL OR changeset_digest ~ '^[0-9a-f]{64}$'),
    redacted_context TEXT NOT NULL,
    context_summary VARCHAR(256) NOT NULL,
    input_digest CHAR(64) NOT NULL
        CHECK (input_digest ~ '^[0-9a-f]{64}$'),
    idempotency_key VARCHAR(128) NOT NULL,
    continuation_sequence SMALLINT NOT NULL
        CHECK (continuation_sequence > 0),
    chain_depth SMALLINT NOT NULL
        CHECK (chain_depth BETWEEN 1 AND 32),
    prior_task_status VARCHAR(24) NOT NULL
        CHECK (prior_task_status IN ('succeeded', 'failed')),
    policy_version VARCHAR(128) NOT NULL,
    policy_snapshot JSONB NOT NULL,
    status VARCHAR(24) NOT NULL DEFAULT 'pending'
        CHECK (status IN (
            'pending', 'claimed', 'dispatched', 'completed', 'cancelled', 'rejected'
        )),
    status_version BIGINT NOT NULL DEFAULT 1 CHECK (status_version > 0),
    claim_owner VARCHAR(128),
    claim_token UUID,
    claim_expires_at TIMESTAMPTZ,
    dispatch_attempts INTEGER NOT NULL DEFAULT 0 CHECK (dispatch_attempts >= 0),
    next_attempt_at TIMESTAMPTZ,
    child_run_id BIGINT,
    result_code VARCHAR(64)
        CHECK (result_code IS NULL OR result_code ~ '^[a-z][a-z0-9_]{0,63}$'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    claimed_at TIMESTAMPTZ,
    dispatched_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    cancelled_at TIMESTAMPTZ,
    rejected_at TIMESTAMPTZ,
    UNIQUE (id, organization_id),
    UNIQUE (organization_id, task_id, idempotency_key),
    UNIQUE (organization_id, task_id, continuation_sequence),
    UNIQUE (organization_id, task_id, source_run_id, trigger_type, evidence_ref),
    UNIQUE (child_run_id),
    FOREIGN KEY (project_id, organization_id)
        REFERENCES devrail_projects (id, organization_id),
    FOREIGN KEY (task_id, organization_id)
        REFERENCES devrail_tasks (id, organization_id),
    FOREIGN KEY (source_run_id, organization_id)
        REFERENCES devrail_runs (id, organization_id),
    FOREIGN KEY (root_run_id, organization_id)
        REFERENCES devrail_runs (id, organization_id),
    FOREIGN KEY (child_run_id, organization_id)
        REFERENCES devrail_runs (id, organization_id),
    FOREIGN KEY (department_id, organization_id)
        REFERENCES departments (id, organization_id),
    CHECK (octet_length(redacted_context) <= 16384),
    CHECK (evidence_expires_at IS NULL OR evidence_expires_at > evidence_observed_at),
    CHECK (
        status <> 'claimed'
        OR claim_owner IS NOT NULL AND claim_token IS NOT NULL AND claim_expires_at IS NOT NULL
    ),
    CHECK (child_run_id IS NULL OR status IN ('dispatched', 'completed')),
    CHECK (status <> 'completed' OR child_run_id IS NOT NULL)
);

ALTER TABLE devrail_runs
    ADD COLUMN continuation_request_id BIGINT,
    ADD CONSTRAINT fk_devrail_runs_continuation_request
        FOREIGN KEY (continuation_request_id, organization_id)
        REFERENCES devrail_continuation_requests (id, organization_id),
    ADD CONSTRAINT devrail_runs_continuation_lineage_check CHECK (
        (run_kind = 'continuation'
            AND continuation_request_id IS NOT NULL
            AND continuation_sequence IS NOT NULL
            AND parent_run_id IS NOT NULL
            AND parent_turn_id IS NOT NULL)
        OR (run_kind <> 'continuation'
            AND continuation_request_id IS NULL
            AND continuation_sequence IS NULL)
    );

CREATE UNIQUE INDEX uq_devrail_runs_continuation_request
    ON devrail_runs (continuation_request_id)
    WHERE continuation_request_id IS NOT NULL;
CREATE INDEX idx_devrail_continuation_requests_scope
    ON devrail_continuation_requests (
        organization_id, project_id, task_id, department_id, owner_user_id, created_at DESC, id DESC
    );
CREATE INDEX idx_devrail_continuation_requests_claim
    ON devrail_continuation_requests (status, next_attempt_at, claim_expires_at, created_at, id)
    WHERE status IN ('pending', 'claimed');
CREATE INDEX idx_devrail_continuation_requests_source
    ON devrail_continuation_requests (
        organization_id, source_run_id, continuation_sequence, id
    );

CREATE TABLE devrail_run_handoffs (
    id BIGSERIAL PRIMARY KEY,
    organization_id BIGINT NOT NULL REFERENCES organizations (id),
    department_id BIGINT,
    owner_user_id BIGINT NOT NULL REFERENCES users (id),
    project_id BIGINT NOT NULL,
    task_id BIGINT NOT NULL,
    source_run_id BIGINT NOT NULL,
    task_snapshot_id BIGINT NOT NULL,
    repository_id BIGINT NOT NULL,
    environment_id BIGINT,
    task_snapshot_digest CHAR(64) NOT NULL
        CHECK (task_snapshot_digest ~ '^[0-9a-f]{64}$'),
    workflow_snapshot_digest CHAR(64) NOT NULL
        CHECK (workflow_snapshot_digest ~ '^[0-9a-f]{64}$'),
    environment_snapshot_digest CHAR(64)
        CHECK (
            environment_snapshot_digest IS NULL
            OR environment_snapshot_digest ~ '^[0-9a-f]{64}$'
        ),
    repository_identity VARCHAR(256) NOT NULL,
    repository_identity_digest CHAR(64) NOT NULL
        CHECK (repository_identity_digest ~ '^[0-9a-f]{64}$'),
    base_commit VARCHAR(128) NOT NULL,
    head_commit VARCHAR(128),
    branch_ref VARCHAR(256),
    changeset_ref VARCHAR(256),
    changeset_digest CHAR(64) NOT NULL
        CHECK (changeset_digest ~ '^[0-9a-f]{64}$'),
    tool_versions JSONB NOT NULL DEFAULT '{}'::jsonb,
    evidence_status VARCHAR(16) NOT NULL
        CHECK (evidence_status IN ('available', 'missing', 'invalid')),
    error_code VARCHAR(64)
        CHECK (error_code IS NULL OR error_code ~ '^[a-z][a-z0-9_]{0,63}$'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    validated_at TIMESTAMPTZ,
    UNIQUE (id, organization_id),
    UNIQUE (source_run_id),
    FOREIGN KEY (project_id, organization_id)
        REFERENCES devrail_projects (id, organization_id),
    FOREIGN KEY (task_id, organization_id)
        REFERENCES devrail_tasks (id, organization_id),
    FOREIGN KEY (source_run_id, organization_id)
        REFERENCES devrail_runs (id, organization_id),
    FOREIGN KEY (task_snapshot_id, organization_id)
        REFERENCES devrail_task_snapshots (id, organization_id),
    FOREIGN KEY (repository_id, organization_id)
        REFERENCES devrail_repositories (id, organization_id),
    FOREIGN KEY (environment_id, organization_id)
        REFERENCES devrail_environments (id, organization_id),
    FOREIGN KEY (department_id, organization_id)
        REFERENCES departments (id, organization_id),
    CHECK (head_commit IS NOT NULL OR changeset_ref IS NOT NULL),
    CHECK (
        (evidence_status = 'available' AND validated_at IS NOT NULL AND error_code IS NULL)
        OR (evidence_status <> 'available' AND error_code IS NOT NULL)
    )
);

CREATE INDEX idx_devrail_run_handoffs_scope
    ON devrail_run_handoffs (
        organization_id, project_id, task_id, department_id, owner_user_id, created_at DESC
    );
CREATE INDEX idx_devrail_run_handoffs_repository
    ON devrail_run_handoffs (organization_id, repository_id, source_run_id);
