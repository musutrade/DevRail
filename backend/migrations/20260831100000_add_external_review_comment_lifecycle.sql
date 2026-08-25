ALTER TABLE devrail_external_review_comments
  ADD COLUMN resolved BOOLEAN NOT NULL DEFAULT FALSE,
  ADD COLUMN deleted_at TIMESTAMPTZ;

