-- Scope external provider identities by organization before enabling the
-- repository-level upsert path.
ALTER TABLE devrail_external_review_comments
  DROP CONSTRAINT IF EXISTS devrail_external_review_comments_provider_external_id_key;

CREATE UNIQUE INDEX IF NOT EXISTS devrail_external_review_comments_org_provider_external_key
  ON devrail_external_review_comments (organization_id, provider, external_id);

-- Every new run already supplies a stable start key. Backfill historical rows
-- so a legacy NULL key cannot bypass the database claim predicate.
UPDATE devrail_runs
SET harness_start_key = 'legacy-run-start:' || id::text,
    updated_at = now()
WHERE harness_start_key IS NULL;

-- Serialize quality-gate command execution for a run. The lease is deliberately
-- separate from the harness start lease because gate commands can run after a
-- run has reached a terminal state.
ALTER TABLE devrail_runs
  ADD COLUMN IF NOT EXISTS quality_gate_claim_owner VARCHAR(128),
  ADD COLUMN IF NOT EXISTS quality_gate_claim_token UUID,
  ADD COLUMN IF NOT EXISTS quality_gate_claim_expires_at TIMESTAMPTZ;

CREATE INDEX IF NOT EXISTS idx_devrail_runs_quality_gate_claim
  ON devrail_runs (quality_gate_claim_expires_at, id)
  WHERE quality_gate_claim_token IS NOT NULL;
