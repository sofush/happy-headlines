CREATE TABLE IF NOT EXISTS articles (
    id bigserial PRIMARY KEY,
    continent text NOT NULL,
    title text NOT NULL,
    content text NOT NULL,
    published_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS articles_published_at_idx ON articles (published_at DESC);
