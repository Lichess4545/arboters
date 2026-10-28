CREATE TYPE subscription_event AS ENUM ('game_started', 'game_finished');

CREATE TABLE subscriptions (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    requester_user_id BIGINT NOT NULL,
    source_lichess_username TEXT NOT NULL CHECK (source_lichess_username = lower(source_lichess_username)),
    event subscription_event NOT NULL,
    target_user_id BIGINT,
    target_channel TEXT,
    target_topic TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT subscriptions_target_is_direct_or_channel CHECK (
        (target_user_id IS NOT NULL AND target_channel IS NULL AND target_topic IS NULL)
        OR (target_user_id IS NULL AND target_channel IS NOT NULL AND target_topic IS NOT NULL)
    )
);

CREATE UNIQUE INDEX subscriptions_unique_intent ON subscriptions (
    requester_user_id,
    source_lichess_username,
    event,
    target_user_id,
    target_channel,
    target_topic
) NULLS NOT DISTINCT;

CREATE INDEX subscriptions_by_event_and_source ON subscriptions (event, source_lichess_username);
