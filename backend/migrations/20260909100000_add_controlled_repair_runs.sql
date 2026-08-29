-- Controlled repair requests, immutable diagnostics, gate reruns and human handoffs.
-- Additive migration: repair automation remains disabled by default.

ALTER TABLE devrail_tasks
    DROP CONSTRAINT IF EXISTS devrail_tasks_status_check,
    ADD CONSTRAINT devrail_tasks_status_check CHECK (status IN (
        'draft', 'queued', 'running', 'awaiting_approval', 'continuation_pending',
        'repair_pending', 'repair_running', 'repair_handoff',
        'succeeded', 'failed', 'cancelled', 'skipped', 'archived'
    ));

CREATE TABLE devrail_repair_diagnoses (
    id BIGSERIAL PRIMARY KEY,
    organization_id BIGINT NOT NULL REFERENCES organizations (id),
    department_id BIGINT,
    owner_user_id BIGINT NOT NULL REFERENCES users (id),
    project_id BIGINT NOT NULL,
    task_id BIGINT NOT NULL,
    source_run_id BIGINT NOT NULL,
    evidence_ref VARCHAR(256) NOT NULL,
    evidence_digest CHAR(64) NOT NULL CHECK (evidence_digest ~ '^[0-9a-f]{64}$'),
    evidence_observed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    evidence_expires_at TIMESTAMPTZ,
    affected_gates JSONB NOT NULL DEFAULT '[]'::jsonb,
    error_summary VARCHAR(512) NOT NULL,
    structured_error JSONB NOT NULL DEFAULT '{}'::jsonb,
    log_ref VARCHAR(256),
    changeset_digest CHAR(64) CHECK (changeset_digest IS NULL OR changeset_digest ~ '^[0-9a-f]{64}$'),
    environment_summary JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (id, organization_id),
    UNIQUE (organization_id, source_run_id, evidence_ref),
    FOREIGN KEY (project_id, organization_id) REFERENCES devrail_projects (id, organization_id),
    FOREIGN KEY (task_id, organization_id) REFERENCES devrail_tasks (id, organization_id),
    FOREIGN KEY (source_run_id, organization_id) REFERENCES devrail_runs (id, organization_id),
    FOREIGN KEY (department_id, organization_id) REFERENCES departments (id, organization_id),
    CHECK (octet_length(error_summary) <= 512),
    CHECK (octet_length(structured_error::text) <= 16384),
    CHECK (octet_length(environment_summary::text) <= 8192),
    CHECK (octet_length(affected_gates::text) <= 4096),
    CHECK (evidence_expires_at IS NULL OR evidence_expires_at > evidence_observed_at)
);

CREATE INDEX idx_devrail_repair_diagnoses_scope
    ON devrail_repair_diagnoses (organization_id, project_id, task_id, created_at DESC);

CREATE TABLE devrail_repair_requests (
    id BIGSERIAL PRIMARY KEY,
    organization_id BIGINT NOT NULL REFERENCES organizations (id),
    department_id BIGINT,
    owner_user_id BIGINT NOT NULL REFERENCES users (id),
    project_id BIGINT NOT NULL,
    task_id BIGINT NOT NULL,
    source_run_id BIGINT NOT NULL,
    root_run_id BIGINT NOT NULL,
    diagnosis_id BIGINT NOT NULL,
    failure_evidence_ref VARCHAR(256) NOT NULL,
    failure_evidence_digest CHAR(64) NOT NULL CHECK (failure_evidence_digest ~ '^[0-9a-f]{64}$'),
    changeset_digest CHAR(64) CHECK (changeset_digest IS NULL OR changeset_digest ~ '^[0-9a-f]{64}$'),
    idempotency_key VARCHAR(160) NOT NULL,
    repair_sequence SMALLINT NOT NULL CHECK (repair_sequence > 0),
    risk_category VARCHAR(32) NOT NULL CHECK (risk_category IN (
        'low_risk', 'logical_change', 'dependency_change', 'remote_write', 'security_change', 'forbidden'
    )),
    strategy_version VARCHAR(128) NOT NULL,
    policy_snapshot JSONB NOT NULL DEFAULT '{}'::jsonb,
    source_task_status VARCHAR(24) NOT NULL,
    status VARCHAR(24) NOT NULL DEFAULT 'pending' CHECK (status IN (
        'pending', 'claimed', 'dispatched', 'running', 'succeeded', 'failed',
        'cancelled', 'handed_off', 'rejected'
    )),
    status_version BIGINT NOT NULL DEFAULT 1 CHECK (status_version > 0),
    claim_owner VARCHAR(128),
    claim_token UUID,
    claim_expires_at TIMESTAMPTZ,
    dispatch_attempts INTEGER NOT NULL DEFAULT 0 CHECK (dispatch_attempts >= 0),
    next_attempt_at TIMESTAMPTZ,
    child_run_id BIGINT,
    cost_units INTEGER NOT NULL DEFAULT 0 CHECK (cost_units >= 0),
    result_code VARCHAR(64) CHECK (result_code IS NULL OR result_code ~ '^[a-z][a-z0-9_]{0,63}$'),
    handoff_reason VARCHAR(128),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    claimed_at TIMESTAMPTZ,
    dispatched_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    cancelled_at TIMESTAMPTZ,
    UNIQUE (id, organization_id),
    UNIQUE (organization_id, task_id, idempotency_key),
    UNIQUE (organization_id, task_id, source_run_id, repair_sequence),
    UNIQUE (child_run_id),
    FOREIGN KEY (project_id, organization_id) REFERENCES devrail_projects (id, organization_id),
    FOREIGN KEY (task_id, organization_id) REFERENCES devrail_tasks (id, organization_id),
    FOREIGN KEY (source_run_id, organization_id) REFERENCES devrail_runs (id, organization_id),
    FOREIGN KEY (root_run_id, organization_id) REFERENCES devrail_runs (id, organization_id),
    FOREIGN KEY (diagnosis_id, organization_id) REFERENCES devrail_repair_diagnoses (id, organization_id),
    FOREIGN KEY (department_id, organization_id) REFERENCES departments (id, organization_id),
    CHECK ((claim_owner IS NULL AND claim_token IS NULL AND claim_expires_at IS NULL)
        OR (claim_owner IS NOT NULL AND claim_token IS NOT NULL AND claim_expires_at IS NOT NULL)),
    CHECK (octet_length(policy_snapshot::text) <= 16384)
);

CREATE INDEX idx_devrail_repair_requests_pending
    ON devrail_repair_requests (organization_id, status, next_attempt_at, id)
    WHERE status IN ('pending', 'claimed');
CREATE INDEX idx_devrail_repair_requests_scope
    ON devrail_repair_requests (organization_id, project_id, task_id, created_at DESC);

CREATE UNIQUE INDEX uq_devrail_run_events_source_event
    ON devrail_run_events (organization_id, source_event_id, event_type)
    WHERE source_event_id IS NOT NULL;

CREATE TABLE devrail_repair_approvals (
    id BIGSERIAL PRIMARY KEY,
    organization_id BIGINT NOT NULL REFERENCES organizations (id),
    department_id BIGINT,
    owner_user_id BIGINT NOT NULL REFERENCES users (id),
    project_id BIGINT NOT NULL,
    task_id BIGINT NOT NULL,
    repair_request_id BIGINT NOT NULL,
    idempotency_key VARCHAR(160) NOT NULL,
    risk_category VARCHAR(32) NOT NULL CHECK (risk_category IN (
        'logical_change', 'dependency_change', 'remote_write', 'security_change'
    )),
    policy_version VARCHAR(128) NOT NULL,
    status VARCHAR(16) NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'approved', 'rejected', 'expired', 'withdrawn')),
    requested_by BIGINT NOT NULL REFERENCES users (id),
    decided_by BIGINT REFERENCES users (id),
    decision_reason VARCHAR(512),
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (id, organization_id),
    UNIQUE (organization_id, repair_request_id, idempotency_key),
    FOREIGN KEY (project_id, organization_id) REFERENCES devrail_projects (id, organization_id),
    FOREIGN KEY (task_id, organization_id) REFERENCES devrail_tasks (id, organization_id),
    FOREIGN KEY (repair_request_id, organization_id) REFERENCES devrail_repair_requests (id, organization_id),
    FOREIGN KEY (department_id, organization_id) REFERENCES departments (id, organization_id)
);

CREATE INDEX idx_devrail_repair_approvals_scope
    ON devrail_repair_approvals (organization_id, status, expires_at, project_id, task_id);

ALTER TABLE devrail_runs
    ADD COLUMN repair_request_id BIGINT,
    ADD COLUMN repair_sequence SMALLINT,
    DROP CONSTRAINT IF EXISTS devrail_runs_run_kind_check,
    ADD CONSTRAINT devrail_runs_run_kind_check CHECK (run_kind IN ('primary', 'retry', 'continuation', 'follow_up', 'repair'));

ALTER TABLE devrail_runs
    ADD CONSTRAINT fk_devrail_runs_repair_request
        FOREIGN KEY (repair_request_id, organization_id)
        REFERENCES devrail_repair_requests (id, organization_id),
    ADD CONSTRAINT devrail_runs_repair_fields_check CHECK (
        (run_kind = 'repair' AND repair_request_id IS NOT NULL AND repair_sequence IS NOT NULL AND repair_sequence > 0)
        OR (run_kind <> 'repair' AND repair_request_id IS NULL AND repair_sequence IS NULL)
    );

CREATE UNIQUE INDEX uq_devrail_runs_repair_request
    ON devrail_runs (organization_id, repair_request_id)
    WHERE repair_request_id IS NOT NULL;

ALTER TABLE devrail_repair_requests
    ADD CONSTRAINT fk_devrail_repair_child_run
        FOREIGN KEY (child_run_id, organization_id)
        REFERENCES devrail_runs (id, organization_id);

ALTER TABLE devrail_tasks
    ADD COLUMN current_repair_request_id BIGINT;

ALTER TABLE devrail_tasks
    ADD CONSTRAINT fk_devrail_tasks_repair_request
        FOREIGN KEY (current_repair_request_id, organization_id)
        REFERENCES devrail_repair_requests (id, organization_id);

CREATE TABLE devrail_repair_gate_reruns (
    id BIGSERIAL PRIMARY KEY,
    organization_id BIGINT NOT NULL REFERENCES organizations (id),
    department_id BIGINT,
    owner_user_id BIGINT NOT NULL REFERENCES users (id),
    project_id BIGINT NOT NULL,
    task_id BIGINT NOT NULL,
    repair_request_id BIGINT NOT NULL,
    child_run_id BIGINT,
    gate_id VARCHAR(64) NOT NULL,
    changeset_digest CHAR(64) NOT NULL CHECK (changeset_digest ~ '^[0-9a-f]{64}$'),
    idempotency_key VARCHAR(256) NOT NULL,
    status VARCHAR(24) NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'running', 'passed', 'failed', 'cancelled')),
    claim_owner VARCHAR(128),
    claim_token UUID,
    claim_expires_at TIMESTAMPTZ,
    result_code VARCHAR(64),
    log_ref VARCHAR(256),
    summary VARCHAR(512),
    duration_ms BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    UNIQUE (id, organization_id),
    UNIQUE (organization_id, repair_request_id, gate_id, changeset_digest),
    UNIQUE (organization_id, idempotency_key),
    FOREIGN KEY (project_id, organization_id) REFERENCES devrail_projects (id, organization_id),
    FOREIGN KEY (task_id, organization_id) REFERENCES devrail_tasks (id, organization_id),
    FOREIGN KEY (repair_request_id, organization_id) REFERENCES devrail_repair_requests (id, organization_id),
    FOREIGN KEY (child_run_id, organization_id) REFERENCES devrail_runs (id, organization_id),
    FOREIGN KEY (department_id, organization_id) REFERENCES departments (id, organization_id),
    CHECK (octet_length(COALESCE(summary, '')) <= 512),
    CHECK ((claim_owner IS NULL AND claim_token IS NULL AND claim_expires_at IS NULL)
        OR (claim_owner IS NOT NULL AND claim_token IS NOT NULL AND claim_expires_at IS NOT NULL))
);

CREATE TABLE devrail_repair_handoffs (
    id BIGSERIAL PRIMARY KEY,
    organization_id BIGINT NOT NULL REFERENCES organizations (id),
    department_id BIGINT,
    owner_user_id BIGINT NOT NULL REFERENCES users (id),
    project_id BIGINT NOT NULL,
    task_id BIGINT NOT NULL,
    repair_request_id BIGINT NOT NULL,
    reason_code VARCHAR(64) NOT NULL CHECK (reason_code ~ '^[a-z][a-z0-9_]{0,63}$'),
    recommendation VARCHAR(512) NOT NULL,
    status VARCHAR(16) NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'resolved')),
    resolved_by BIGINT REFERENCES users (id),
    resolved_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (id, organization_id),
    UNIQUE (organization_id, repair_request_id, reason_code),
    FOREIGN KEY (project_id, organization_id) REFERENCES devrail_projects (id, organization_id),
    FOREIGN KEY (task_id, organization_id) REFERENCES devrail_tasks (id, organization_id),
    FOREIGN KEY (repair_request_id, organization_id) REFERENCES devrail_repair_requests (id, organization_id),
    FOREIGN KEY (department_id, organization_id) REFERENCES departments (id, organization_id),
    CHECK (octet_length(recommendation) <= 512)
);

CREATE INDEX idx_devrail_repair_handoffs_scope
    ON devrail_repair_handoffs (organization_id, project_id, task_id, status, created_at DESC);

ALTER TABLE devrail_task_status_history
    DROP CONSTRAINT IF EXISTS devrail_task_history_continuation_context_check,
    ADD COLUMN repair_request_id BIGINT,
    ADD COLUMN repair_diagnosis_id BIGINT,
    ADD COLUMN repair_policy_version VARCHAR(128),
    ADD COLUMN repair_result_code VARCHAR(64),
    ADD CONSTRAINT fk_devrail_task_history_repair_request
        FOREIGN KEY (repair_request_id, organization_id)
        REFERENCES devrail_repair_requests (id, organization_id),
    ADD CONSTRAINT fk_devrail_task_history_repair_diagnosis
        FOREIGN KEY (repair_diagnosis_id, organization_id)
        REFERENCES devrail_repair_diagnoses (id, organization_id),
    ADD CONSTRAINT devrail_task_history_execution_context_check CHECK (
        (continuation_request_id IS NULL
            AND repair_request_id IS NULL
            AND source_run_id IS NULL
            AND child_run_id IS NULL
            AND continuation_trigger_type IS NULL
            AND continuation_policy_version IS NULL
            AND repair_diagnosis_id IS NULL
            AND repair_policy_version IS NULL
            AND repair_result_code IS NULL)
        OR (continuation_request_id IS NOT NULL
            AND repair_request_id IS NULL
            AND source_run_id IS NOT NULL
            AND continuation_trigger_type IN ('user_context', 'quality_gate', 'review_changes')
            AND continuation_policy_version IS NOT NULL
            AND repair_diagnosis_id IS NULL
            AND repair_policy_version IS NULL
            AND repair_result_code IS NULL)
        OR (repair_request_id IS NOT NULL
            AND continuation_request_id IS NULL
            AND source_run_id IS NOT NULL
            AND repair_diagnosis_id IS NOT NULL
            AND repair_policy_version IS NOT NULL
            AND continuation_trigger_type IS NULL
            AND continuation_policy_version IS NULL)
    );

CREATE INDEX idx_devrail_task_history_repair
    ON devrail_task_status_history (organization_id, repair_request_id, created_at, id)
    WHERE repair_request_id IS NOT NULL;

CREATE OR REPLACE FUNCTION devrail_record_task_status_history()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    configured_actor_type TEXT;
    configured_actor_user_id BIGINT;
    configured_reason TEXT;
    configured_trace TEXT;
    configured_continuation_request_id BIGINT;
    configured_repair_request_id BIGINT;
    configured_source_run_id BIGINT;
    configured_child_run_id BIGINT;
    configured_trigger_type TEXT;
    configured_continuation_policy_version TEXT;
    configured_repair_diagnosis_id BIGINT;
    configured_repair_policy_version TEXT;
    configured_repair_result_code TEXT;
    effective_actor_type TEXT;
BEGIN
    IF OLD.status IS NOT DISTINCT FROM NEW.status THEN
        RETURN NEW;
    END IF;

    configured_actor_type := NULLIF(current_setting('devrail.actor_type', true), '');
    configured_actor_user_id := NULLIF(current_setting('devrail.actor_user_id', true), '')::BIGINT;
    configured_reason := NULLIF(current_setting('devrail.transition_reason', true), '');
    configured_trace := NULLIF(current_setting('devrail.trace_id', true), '');
    configured_continuation_request_id :=
        NULLIF(current_setting('devrail.continuation_request_id', true), '')::BIGINT;
    configured_repair_request_id :=
        NULLIF(current_setting('devrail.repair_request_id', true), '')::BIGINT;
    configured_source_run_id :=
        NULLIF(current_setting('devrail.source_run_id', true), '')::BIGINT;
    configured_child_run_id :=
        NULLIF(current_setting('devrail.child_run_id', true), '')::BIGINT;
    configured_trigger_type :=
        NULLIF(current_setting('devrail.continuation_trigger_type', true), '');
    configured_continuation_policy_version :=
        NULLIF(current_setting('devrail.continuation_policy_version', true), '');
    configured_repair_diagnosis_id :=
        NULLIF(current_setting('devrail.repair_diagnosis_id', true), '')::BIGINT;
    configured_repair_policy_version :=
        NULLIF(current_setting('devrail.repair_policy_version', true), '');
    configured_repair_result_code :=
        NULLIF(current_setting('devrail.repair_result_code', true), '');
    effective_actor_type := COALESCE(
        configured_actor_type,
        CASE WHEN NEW.scheduler_claim_token IS NOT NULL THEN 'system' ELSE 'user' END
    );

    INSERT INTO devrail_task_status_history (
        organization_id, department_id, owner_user_id, task_id, task_revision,
        from_status, to_status, actor_type, actor_user_id, reason, trace_id,
        continuation_request_id, repair_request_id, source_run_id, child_run_id,
        continuation_trigger_type, continuation_policy_version,
        repair_diagnosis_id, repair_policy_version, repair_result_code
    ) VALUES (
        NEW.organization_id, NEW.department_id, NEW.owner_user_id, NEW.id, NEW.revision,
        OLD.status, NEW.status,
        effective_actor_type,
        CASE
            WHEN effective_actor_type = 'system' THEN configured_actor_user_id
            ELSE COALESCE(configured_actor_user_id, NEW.owner_user_id)
        END,
        COALESCE(configured_reason, 'task_status_updated'),
        configured_trace,
        configured_continuation_request_id, configured_repair_request_id,
        configured_source_run_id, configured_child_run_id,
        configured_trigger_type, configured_continuation_policy_version,
        configured_repair_diagnosis_id, configured_repair_policy_version,
        configured_repair_result_code
    );
    RETURN NEW;
END;
$$;
