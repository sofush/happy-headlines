CREATE TABLE IF NOT EXISTS subscribers (
    email text PRIMARY KEY,
    subscribed_at timestamptz NOT NULL DEFAULT now()
);
