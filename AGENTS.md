# otr-processor agent guidance

Run commands from the repository root with stable Rust (`rust-version` in
`Cargo.toml`). `cargo +nightly fmt` is the one nightly command;
`rustfmt.toml` uses unstable options.

- This repository owns the rating calculation and its SQL. The schema and
  migrations are owned by `otr-web`; treat that boundary as a contract.
- Start from `.env.example` and keep credentials in the ignored `.env`. Never
  commit credentials or put a RabbitMQ URL with credentials in logs, fixtures,
  or review output.
- Running the binary is destructive. Never `cargo run` against production,
  staging, or a shared database to verify a change; use a disposable database.
  `--ignore-constraints` is not a dry run.

## Commands

- `cargo run -- --help` lists options. `CONNECTION_STRING` is required.
- `cargo test` runs unit tests. Database integration tests use Testcontainers
  and need Docker; real-broker tests are ignored by default and run only
  against an approved disposable RabbitMQ.
- Before handing off: `cargo +nightly fmt -- --check`, `cargo clippy`,
  `cargo test`, `git diff --check`. Report a check infrastructure prevented as
  skipped, not passed.

## Layout

`src/main.rs` runs one full recomputation in a single transaction: recalculate
score placements, load data with verification status `4`, build initial
ratings, process matches chronologically with `OtrModel`, apply decay, replace
`player_ratings` and `rating_adjustments`, update `player_highest_ranks`, drop
derived stats for rejected data, publish stats refresh messages, commit.

- `src/args.rs` CLI and env parsing. `src/model/` initial ratings,
  Plackett-Luce, decay, ranking, persisted types. `src/database/` SQL, row
  mappings, bulk writes, transactions. `src/messaging/` RabbitMQ topology,
  envelope, retry, publishing. `src/utils/` shared helpers.
- Keep orchestration in `main`, math in `model`, persistence in `database`,
  broker behavior in `messaging`. Diagnostics use structured `tracing` fields.

## Rating invariants

- Chronological match order, match-end fallback, decay boundaries, and ranking
  tie behavior are part of the rating contract.
- Persisted `Ruleset` is `0..=5` and `RatingAdjustmentType` is `0..=3`. Never
  reorder or renumber.
- Match-method weights, initial-rating bounds, decay constants, volatility, and
  the rating floor move every historical result. Changes need focused unit
  tests, full rating tests, and a stated expected impact.
- A verified game with fewer than two verified scores is skipped. Keep the
  data-integrity warnings and test any eligibility change.
- Ranking, country ranking, percentile, rating history, and highest rank stay
  mutually consistent across every ruleset.
- Use deterministic fixtures with explicit timestamps and placements. Float
  assertions state a meaningful tolerance.

## Database and messaging contracts

- `src/database/db.rs` embeds SQL against the sibling `otr-web` checkout's
  `packages/otr-core/src/db/schema.ts`; migrations live in its
  `apps/web/drizzle/`. A physical name, type, nullability, enum,
  verification-rule, or relationship change is one compatibility change across
  both repositories: update row structs, SQL, COPY column lists, and
  `tests/database/schema.sql` together, use additive migrations, and keep the
  deployed web app, processor, and workers compatible. Never edit an applied
  migration. Test on a fresh disposable database.
- Recomputation truncates with `RESTART IDENTITY CASCADE`, writes with `COPY`,
  updates `game_scores`, and deletes stale stats. Keep every write on the same
  connection inside the transaction guard; do not weaken rollback.
- The exchange, queue, routing key, AMQP properties, and camel-case JSON of
  `ProcessTournamentStatsMessage` are contracts with the data worker.
  Publishing happens before commit, broker failure does not stop processing,
  and publish failures do not abort the run; never assume exactly-once delivery
  or committed data at publish time.
