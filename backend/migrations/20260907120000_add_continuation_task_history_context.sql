-- Continuation task projections carry immutable lineage context in the
-- TaskTracker status history. Non-continuation transitions keep these fields
-- null and remain backward compatible.

ALTER TABLE devrail_task_status_history
    ADD COLUMN continuation_request_id BIGINT,
    ADD COLUMN source_run_id BIGINT,
    ADD COLUMN child_run_id BIGINT,
    ADD COLUMN continuation_trigger_type VARCHAR(24),
    ADD COLUMN continuation_policy_version VARCHAR(128),
    ADD CONSTRAINT fk_devrail_task_history_continuation_request
        FOREIGN KEY (continuation_request_id, organization_id)
        REFERENCES devrail_continuation_requests (id, organization_id),
    ADD CONSTRAINT fk_devrail_task_history_source_run
        FOREIGN KEY (source_run_id, organization_id)
        REFERENCES devrail_runs (id, organization_id),
    ADD CONSTRAINT fk_devrail_task_history_child_run
        FOREIGN KEY (child_run_id, organization_id)
        REFERENCES devrail_runs (id, organization_id),
    ADD CONSTRAINT devrail_task_history_continuation_context_check CHECK (
        (continuation_request_id IS NULL
            AND source_run_id IS NULL
            AND child_run_id IS NULL
            AND continuation_trigger_type IS NULL
            AND continuation_policy_version IS NULL)
        OR (continuation_request_id IS NOT NULL
            AND source_run_id IS NOT NULL
            AND continuation_trigger_type IN (
                'user_context', 'quality_gate', 'review_changes'
            )
            AND continuation_policy_version IS NOT NULL)
    );

CREATE INDEX idx_devrail_task_history_continuation
    ON devrail_task_status_history (
        organization_id, continuation_request_id, created_at, id
    )
    WHERE continuation_request_id IS NOT NULL;

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
    configured_source_run_id BIGINT;
    configured_child_run_id BIGINT;
    configured_trigger_type TEXT;
    configured_policy_version TEXT;
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
    configured_source_run_id :=
        NULLIF(current_setting('devrail.source_run_id', true), '')::BIGINT;
    configured_child_run_id :=
        NULLIF(current_setting('devrail.child_run_id', true), '')::BIGINT;
    configured_trigger_type :=
        NULLIF(current_setting('devrail.continuation_trigger_type', true), '');
    configured_policy_version :=
        NULLIF(current_setting('devrail.continuation_policy_version', true), '');
    effective_actor_type := COALESCE(
        configured_actor_type,
        CASE WHEN NEW.scheduler_claim_token IS NOT NULL THEN 'system' ELSE 'user' END
    );

    INSERT INTO devrail_task_status_history (
        organization_id, department_id, owner_user_id, task_id, task_revision,
        from_status, to_status, actor_type, actor_user_id, reason, trace_id,
        continuation_request_id, source_run_id, child_run_id,
        continuation_trigger_type, continuation_policy_version
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
        configured_continuation_request_id,
        configured_source_run_id,
        configured_child_run_id,
        configured_trigger_type,
        configured_policy_version
    );
    RETURN NEW;
END;
$$;
