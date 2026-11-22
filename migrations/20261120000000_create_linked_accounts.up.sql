CREATE TABLE linked_accounts (
    chat_user_id BIGINT PRIMARY KEY CHECK (chat_user_id >= 0),
    lichess_user_id TEXT NOT NULL CHECK (lichess_user_id <> '' AND lichess_user_id = lower(lichess_user_id)),
    linked_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
