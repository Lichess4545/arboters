CREATE TYPE subscription_event AS ENUM ('game_started', 'game_finished');

CREATE TYPE subscription_source_kind AS ENUM ('player', 'team');

CREATE TABLE subscriptions (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    requester_user_id BIGINT NOT NULL CHECK (requester_user_id >= 0),
    league TEXT NOT NULL CHECK (league <> '' AND league = lower(league)),
    source_kind subscription_source_kind NOT NULL,
    source_name TEXT NOT NULL CHECK (source_name <> '' AND source_name = lower(source_name)),
    event subscription_event NOT NULL,
    target_user_id BIGINT CHECK (target_user_id >= 0),
    target_channel TEXT CHECK (target_channel <> ''),
    target_topic TEXT CHECK (target_topic <> ''),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT subscriptions_target_is_direct_or_channel CHECK (
        (target_user_id IS NOT NULL AND target_channel IS NULL AND target_topic IS NULL)
        OR (target_user_id IS NULL AND target_channel IS NOT NULL AND target_topic IS NOT NULL)
    )
);

CREATE UNIQUE INDEX subscriptions_unique_intent ON subscriptions (
    requester_user_id,
    league,
    source_kind,
    source_name,
    event,
    target_user_id,
    target_channel,
    target_topic
) NULLS NOT DISTINCT;

CREATE INDEX subscriptions_by_source ON subscriptions (league, event, source_kind, source_name);
