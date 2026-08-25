CREATE TABLE devrail_external_review_comments (
  id BIGSERIAL PRIMARY KEY,
  organization_id BIGINT NOT NULL REFERENCES organizations(id),
  review_id BIGINT NOT NULL REFERENCES devrail_reviews(id) ON DELETE CASCADE,
  provider TEXT NOT NULL,
  external_id TEXT NOT NULL,
  file_path TEXT NOT NULL,
  line_start INTEGER,
  line_end INTEGER,
  body TEXT NOT NULL,
  author_name TEXT NOT NULL,
  external_created_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE(provider, external_id)
);
CREATE INDEX devrail_external_review_comments_review_idx ON devrail_external_review_comments(review_id, created_at);
