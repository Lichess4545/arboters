CREATE TYPE response_matching AS ENUM ('any_case', 'exact_case');

CREATE TABLE responses (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    phrase TEXT NOT NULL CHECK (phrase <> ''),
    matching response_matching NOT NULL,
    markdown TEXT NOT NULL CHECK (markdown <> ''),
    source_message_id BIGINT NOT NULL CHECK (source_message_id >= 0),
    added_by_user_id BIGINT NOT NULL CHECK (added_by_user_id >= 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX responses_unique_source ON responses (lower(phrase), source_message_id);
