CREATE TABLE leagues (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    source TEXT NOT NULL CHECK (source <> ''),
    key TEXT NOT NULL CHECK (key <> ''),
    UNIQUE (source, key)
);

CREATE TABLE fixtures (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    league_id BIGINT NOT NULL REFERENCES leagues (id),
    starts_at TIMESTAMPTZ,
    ends_at TIMESTAMPTZ,
    stored_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    replaced_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ
);

CREATE UNIQUE INDEX fixtures_one_unreplaced_per_league ON fixtures (league_id) WHERE replaced_at IS NULL;

CREATE TYPE variant AS ENUM (
    'standard',
    'chess960',
    'crazyhouse',
    'antichess',
    'atomic',
    'horde',
    'king_of_the_hill',
    'racing_kings',
    'three_check',
    'from_position'
);

CREATE TABLE fixture_games (
    fixtures_id BIGINT NOT NULL REFERENCES fixtures (id),
    white TEXT NOT NULL CHECK (white <> '' AND white = lower(white)),
    black TEXT NOT NULL CHECK (black <> '' AND black = lower(black)),
    initial_seconds BIGINT CHECK (initial_seconds >= 0),
    increment_seconds BIGINT CHECK (increment_seconds >= 0),
    variant variant NOT NULL,
    rated BOOLEAN NOT NULL,
    CHECK ((initial_seconds IS NULL) = (increment_seconds IS NULL)),
    PRIMARY KEY (fixtures_id, white, black)
);
