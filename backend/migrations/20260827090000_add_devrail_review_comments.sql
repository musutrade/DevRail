CREATE TABLE devrail_review_comments (
    id BIGSERIAL PRIMARY KEY,
    organization_id BIGINT NOT NULL REFERENCES organizations(id),
    review_id BIGINT NOT NULL REFERENCES devrail_reviews(id) ON DELETE CASCADE,
    author_user_id BIGINT NOT NULL REFERENCES users(id),
    file_path TEXT NOT NULL,
    line_start INTEGER,
    line_end INTEGER,
    body TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (length(file_path) BETWEEN 1 AND 1024),
    CHECK (length(body) BETWEEN 1 AND 10000),
    CHECK (line_start IS NULL OR line_start > 0),
    CHECK (line_end IS NULL OR line_end >= COALESCE(line_start, 1)),
    UNIQUE (review_id, author_user_id, file_path, line_start, line_end, body)
);
CREATE INDEX idx_devrail_review_comments_review ON devrail_review_comments (organization_id, review_id, created_at, id);
