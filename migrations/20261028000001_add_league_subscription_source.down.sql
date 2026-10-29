DELETE FROM subscriptions WHERE source_kind = 'league';

ALTER TYPE subscription_source_kind RENAME TO subscription_source_kind_with_league;

CREATE TYPE subscription_source_kind AS ENUM ('player', 'team');

ALTER TABLE subscriptions
    ALTER COLUMN source_kind TYPE subscription_source_kind
    USING source_kind::text::subscription_source_kind;

DROP TYPE subscription_source_kind_with_league;
