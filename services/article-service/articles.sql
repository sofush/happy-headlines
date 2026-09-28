CREATE TABLE IF NOT EXISTS articles (
    id bigserial PRIMARY KEY,
    continent text NOT NULL,
    title text NOT NULL,
    content text NOT NULL
);
