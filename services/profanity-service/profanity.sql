CREATE TABLE IF NOT EXISTS words (
    word text PRIMARY KEY
);

INSERT INTO words (word) VALUES
    ('arsehole'),
    ('asshole'),
    ('bollocks'),
    ('bullshit'),
    ('crap'),
    ('damn'),
    ('fuck'),
    ('shit')
ON CONFLICT (word) DO NOTHING;
