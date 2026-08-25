CREATE TABLE devrail_pull_requests (
  id BIGSERIAL PRIMARY KEY,
  organization_id BIGINT NOT NULL REFERENCES organizations(id),
  repository_id BIGINT NOT NULL REFERENCES devrail_repositories(id) ON DELETE CASCADE,
  provider TEXT NOT NULL,
  number BIGINT NOT NULL,
  url TEXT NOT NULL,
  status TEXT NOT NULL,
  last_synced_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE(repository_id, provider, number)
);
CREATE INDEX devrail_pull_requests_scope_idx ON devrail_pull_requests(organization_id, repository_id, updated_at DESC);
