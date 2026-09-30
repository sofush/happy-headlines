DROP TABLE IF EXISTS comments;

CREATE TABLE comments (
    id bigserial PRIMARY KEY,
    continent text NOT NULL,
    article_id bigint NOT NULL,
    author text NOT NULL,
    content text NOT NULL
);