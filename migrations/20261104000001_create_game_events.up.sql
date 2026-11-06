CREATE TYPE game_event_kind AS ENUM ('game_started', 'game_finished');

CREATE TABLE game_events (
    game_id TEXT NOT NULL CHECK (game_id <> ''),
    kind game_event_kind NOT NULL,
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (game_id, kind)
);

CREATE TABLE game_event_fixtures (
    game_id TEXT NOT NULL,
    kind game_event_kind NOT NULL,
    fixtures_id BIGINT NOT NULL,
    white TEXT NOT NULL,
    black TEXT NOT NULL,
    PRIMARY KEY (game_id, kind, fixtures_id, white, black),
    FOREIGN KEY (game_id, kind) REFERENCES game_events (game_id, kind),
    FOREIGN KEY (fixtures_id, white, black) REFERENCES fixture_games (fixtures_id, white, black)
);
