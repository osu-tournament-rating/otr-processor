-- Complete database schema for otr-processor database tests
-- This matches the production database schema

-- First create the users table as it's referenced by foreign keys
CREATE TABLE IF NOT EXISTS users (
    id SERIAL PRIMARY KEY
);

-- Create beatmaps table as it's referenced by games
CREATE TABLE IF NOT EXISTS beatmaps (
    id SERIAL PRIMARY KEY
);

-- Main tables
CREATE TABLE IF NOT EXISTS players (
    id INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    osu_id BIGINT NOT NULL,
    username VARCHAR(32) DEFAULT ''::character varying NOT NULL,
    country VARCHAR(4) DEFAULT ''::character varying NOT NULL,
    default_ruleset INTEGER DEFAULT 0 NOT NULL,
    osu_last_fetch TIMESTAMP WITH TIME ZONE DEFAULT '2007-09-17 00:00:00'::timestamp without time zone NOT NULL,
    osu_track_last_fetch TIMESTAMP WITH TIME ZONE DEFAULT '2007-09-17 00:00:00'::timestamp without time zone NOT NULL,
    created TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated TIMESTAMP WITH TIME ZONE
);

CREATE UNIQUE INDEX IF NOT EXISTS ix_players_osu_id ON players (osu_id);
CREATE INDEX IF NOT EXISTS ix_players_country ON players (country);

CREATE TABLE IF NOT EXISTS player_osu_ruleset_data (
    id INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    ruleset INTEGER NOT NULL,
    pp DOUBLE PRECISION NOT NULL,
    global_rank INTEGER NOT NULL,
    earliest_global_rank INTEGER,
    earliest_global_rank_date TIMESTAMP WITH TIME ZONE,
    player_id INTEGER NOT NULL REFERENCES players(id) ON DELETE CASCADE,
    created TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated TIMESTAMP WITH TIME ZONE
);

CREATE UNIQUE INDEX IF NOT EXISTS ix_player_osu_ruleset_data_player_id_ruleset ON player_osu_ruleset_data (player_id, ruleset);
CREATE UNIQUE INDEX IF NOT EXISTS ix_player_osu_ruleset_data_player_id_ruleset_global_rank ON player_osu_ruleset_data (player_id, ruleset, global_rank);

CREATE TABLE IF NOT EXISTS player_highest_ranks (
    id INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    ruleset INTEGER NOT NULL,
    global_rank INTEGER NOT NULL,
    global_rank_date TIMESTAMP WITH TIME ZONE NOT NULL,
    country_rank INTEGER NOT NULL,
    country_rank_date TIMESTAMP WITH TIME ZONE NOT NULL,
    player_id INTEGER NOT NULL REFERENCES players(id) ON DELETE CASCADE,
    created TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated TIMESTAMP WITH TIME ZONE
);

CREATE INDEX IF NOT EXISTS ix_player_highest_ranks_country_rank ON player_highest_ranks (country_rank DESC);
CREATE INDEX IF NOT EXISTS ix_player_highest_ranks_global_rank ON player_highest_ranks (global_rank DESC);
CREATE UNIQUE INDEX IF NOT EXISTS ix_player_highest_ranks_player_id_ruleset ON player_highest_ranks (player_id, ruleset);

CREATE TABLE IF NOT EXISTS player_ratings (
    id INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    ruleset INTEGER NOT NULL,
    rating DOUBLE PRECISION NOT NULL,
    volatility DOUBLE PRECISION NOT NULL,
    percentile DOUBLE PRECISION NOT NULL,
    global_rank INTEGER NOT NULL,
    country_rank INTEGER NOT NULL,
    player_id INTEGER NOT NULL REFERENCES players(id) ON DELETE CASCADE,
    created TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX IF NOT EXISTS ix_player_ratings_player_id ON player_ratings (player_id);
CREATE UNIQUE INDEX IF NOT EXISTS ix_player_ratings_player_id_ruleset ON player_ratings (player_id, ruleset);
CREATE INDEX IF NOT EXISTS ix_player_ratings_rating ON player_ratings (rating DESC);
CREATE INDEX IF NOT EXISTS ix_player_ratings_ruleset ON player_ratings (ruleset);
CREATE INDEX IF NOT EXISTS ix_player_ratings_ruleset_rating ON player_ratings (ruleset ASC, rating DESC);

CREATE TABLE IF NOT EXISTS tournaments (
    id INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    name VARCHAR(512) NOT NULL,
    abbreviation VARCHAR(32) NOT NULL,
    forum_url VARCHAR(255) NOT NULL,
    rank_range_lower_bound INTEGER NOT NULL,
    ruleset INTEGER NOT NULL,
    lobby_size INTEGER NOT NULL,
    verification_status INTEGER DEFAULT 0 NOT NULL,
    last_processing_date TIMESTAMP WITH TIME ZONE DEFAULT '2007-09-17 00:00:00'::timestamp without time zone NOT NULL,
    rejection_reason INTEGER DEFAULT 0 NOT NULL,
    processing_status INTEGER DEFAULT 0 NOT NULL,
    submitted_by_user_id INTEGER REFERENCES users(id) ON DELETE SET NULL,
    verified_by_user_id INTEGER REFERENCES users(id) ON DELETE SET NULL,
    start_time TIMESTAMP WITH TIME ZONE,
    end_time TIMESTAMP WITH TIME ZONE,
    created TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated TIMESTAMP WITH TIME ZONE
);

CREATE UNIQUE INDEX IF NOT EXISTS ix_tournaments_name_abbreviation ON tournaments (name, abbreviation);
CREATE INDEX IF NOT EXISTS ix_tournaments_ruleset ON tournaments (ruleset);
CREATE INDEX IF NOT EXISTS ix_tournaments_submitted_by_user_id ON tournaments (submitted_by_user_id);
CREATE INDEX IF NOT EXISTS ix_tournaments_verified_by_user_id ON tournaments (verified_by_user_id);

CREATE TABLE IF NOT EXISTS matches (
    id INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    osu_id BIGINT NOT NULL,
    name VARCHAR(512) DEFAULT ''::character varying NOT NULL,
    start_time TIMESTAMP WITH TIME ZONE,
    end_time TIMESTAMP WITH TIME ZONE,
    verification_status INTEGER DEFAULT 0 NOT NULL,
    last_processing_date TIMESTAMP WITH TIME ZONE DEFAULT '2007-09-17 00:00:00'::timestamp without time zone NOT NULL,
    rejection_reason INTEGER DEFAULT 0 NOT NULL,
    warning_flags INTEGER DEFAULT 0 NOT NULL,
    processing_status INTEGER DEFAULT 0 NOT NULL,
    tournament_id INTEGER NOT NULL REFERENCES tournaments(id) ON DELETE CASCADE,
    submitted_by_user_id INTEGER REFERENCES users(id) ON DELETE SET NULL,
    verified_by_user_id INTEGER REFERENCES users(id) ON DELETE SET NULL,
    created TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated TIMESTAMP WITH TIME ZONE
);

CREATE UNIQUE INDEX IF NOT EXISTS ix_matches_osu_id ON matches (osu_id);
CREATE INDEX IF NOT EXISTS ix_matches_submitted_by_user_id ON matches (submitted_by_user_id);
CREATE INDEX IF NOT EXISTS ix_matches_tournament_id ON matches (tournament_id);
CREATE INDEX IF NOT EXISTS ix_matches_verified_by_user_id ON matches (verified_by_user_id);

CREATE TABLE IF NOT EXISTS games (
    id INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    osu_id BIGINT NOT NULL,
    ruleset INTEGER NOT NULL,
    scoring_type INTEGER NOT NULL,
    team_type INTEGER NOT NULL,
    mods INTEGER NOT NULL,
    start_time TIMESTAMP WITH TIME ZONE DEFAULT '2007-09-17 00:00:00'::timestamp without time zone NOT NULL,
    end_time TIMESTAMP WITH TIME ZONE DEFAULT '2007-09-17 00:00:00'::timestamp without time zone NOT NULL,
    verification_status INTEGER DEFAULT 0 NOT NULL,
    rejection_reason INTEGER DEFAULT 0 NOT NULL,
    warning_flags INTEGER DEFAULT 0 NOT NULL,
    processing_status INTEGER DEFAULT 0 NOT NULL,
    last_processing_date TIMESTAMP WITH TIME ZONE DEFAULT '2007-09-17 00:00:00'::timestamp without time zone NOT NULL,
    match_id INTEGER NOT NULL REFERENCES matches(id) ON DELETE CASCADE,
    beatmap_id INTEGER REFERENCES beatmaps(id) ON DELETE CASCADE,
    created TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated TIMESTAMP WITH TIME ZONE
);

CREATE INDEX IF NOT EXISTS ix_games_beatmap_id ON games (beatmap_id);
CREATE INDEX IF NOT EXISTS ix_games_match_id ON games (match_id);
CREATE UNIQUE INDEX IF NOT EXISTS ix_games_osu_id ON games (osu_id);
CREATE INDEX IF NOT EXISTS ix_games_start_time ON games (start_time);

CREATE TABLE IF NOT EXISTS game_scores (
    id INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    score INTEGER NOT NULL,
    placement INTEGER NOT NULL,
    max_combo INTEGER NOT NULL,
    count50 INTEGER NOT NULL,
    count100 INTEGER NOT NULL,
    count300 INTEGER NOT NULL,
    count_miss INTEGER NOT NULL,
    count_katu INTEGER NOT NULL,
    count_geki INTEGER NOT NULL,
    pass BOOLEAN NOT NULL,
    perfect BOOLEAN NOT NULL,
    grade INTEGER NOT NULL,
    mods INTEGER NOT NULL,
    team INTEGER NOT NULL,
    ruleset INTEGER NOT NULL,
    verification_status INTEGER NOT NULL,
    last_processing_date TIMESTAMP WITH TIME ZONE DEFAULT '2007-09-17 00:00:00'::timestamp without time zone NOT NULL,
    rejection_reason INTEGER NOT NULL,
    processing_status INTEGER NOT NULL,
    game_id INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    player_id INTEGER NOT NULL REFERENCES players(id) ON DELETE CASCADE,
    created TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated TIMESTAMP WITH TIME ZONE
);

CREATE INDEX IF NOT EXISTS ix_game_scores_game_id ON game_scores (game_id);
CREATE INDEX IF NOT EXISTS ix_game_scores_player_id ON game_scores (player_id);
CREATE UNIQUE INDEX IF NOT EXISTS ix_game_scores_player_id_game_id ON game_scores (player_id, game_id);

CREATE TABLE IF NOT EXISTS player_match_stats (
    id INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    match_cost DOUBLE PRECISION NOT NULL,
    average_score DOUBLE PRECISION NOT NULL,
    average_placement DOUBLE PRECISION NOT NULL,
    average_misses DOUBLE PRECISION NOT NULL,
    average_accuracy DOUBLE PRECISION NOT NULL,
    games_played INTEGER NOT NULL,
    games_won INTEGER NOT NULL,
    games_lost INTEGER NOT NULL,
    won BOOLEAN NOT NULL,
    teammate_ids INTEGER[] NOT NULL,
    opponent_ids INTEGER[] NOT NULL,
    player_id INTEGER NOT NULL REFERENCES players(id) ON DELETE CASCADE,
    match_id INTEGER NOT NULL REFERENCES matches(id) ON DELETE CASCADE,
    created TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX IF NOT EXISTS ix_player_match_stats_match_id ON player_match_stats (match_id);
CREATE INDEX IF NOT EXISTS ix_player_match_stats_player_id ON player_match_stats (player_id);
CREATE UNIQUE INDEX IF NOT EXISTS ix_player_match_stats_player_id_match_id ON player_match_stats (player_id, match_id);
CREATE INDEX IF NOT EXISTS ix_player_match_stats_player_id_won ON player_match_stats (player_id, won);

CREATE TABLE IF NOT EXISTS player_tournament_stats (
    id INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    average_rating_delta DOUBLE PRECISION NOT NULL,
    average_match_cost DOUBLE PRECISION NOT NULL,
    average_score INTEGER NOT NULL,
    average_placement DOUBLE PRECISION NOT NULL,
    average_accuracy DOUBLE PRECISION NOT NULL,
    matches_played INTEGER NOT NULL,
    matches_won INTEGER NOT NULL,
    matches_lost INTEGER NOT NULL,
    games_played INTEGER NOT NULL,
    games_won INTEGER NOT NULL,
    games_lost INTEGER NOT NULL,
    teammate_ids INTEGER[] NOT NULL,
    player_id INTEGER NOT NULL REFERENCES players(id) ON DELETE CASCADE,
    tournament_id INTEGER NOT NULL REFERENCES tournaments(id) ON DELETE CASCADE,
    created TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP NOT NULL,
    match_win_rate DOUBLE PRECISION DEFAULT 0.0 NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS ix_player_tournament_stats_player_id_tournament_id ON player_tournament_stats (player_id, tournament_id);
CREATE INDEX IF NOT EXISTS ix_player_tournament_stats_tournament_id ON player_tournament_stats (tournament_id);

CREATE TABLE IF NOT EXISTS rating_adjustments (
    id INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    adjustment_type INTEGER NOT NULL,
    ruleset INTEGER NOT NULL,
    timestamp TIMESTAMP WITH TIME ZONE NOT NULL,
    rating_before DOUBLE PRECISION NOT NULL,
    rating_after DOUBLE PRECISION NOT NULL,
    volatility_before DOUBLE PRECISION NOT NULL,
    volatility_after DOUBLE PRECISION NOT NULL,
    player_rating_id INTEGER NOT NULL REFERENCES player_ratings(id) ON DELETE CASCADE,
    player_id INTEGER NOT NULL REFERENCES players(id) ON DELETE CASCADE,
    match_id INTEGER REFERENCES matches(id) ON DELETE CASCADE,
    created TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX IF NOT EXISTS ix_rating_adjustments_match_id ON rating_adjustments (match_id);
CREATE UNIQUE INDEX IF NOT EXISTS ix_rating_adjustments_player_id_match_id ON rating_adjustments (player_id, match_id);
CREATE INDEX IF NOT EXISTS ix_rating_adjustments_player_id_timestamp ON rating_adjustments (player_id, timestamp);
CREATE INDEX IF NOT EXISTS ix_rating_adjustments_player_rating_id ON rating_adjustments (player_rating_id);