# Repository Guide

## Purpose

`otr-processor` is the Rust rating engine for osu! Tournament Rating. It reads
verified tournament data from PostgreSQL, rebuilds player ratings and rating
history, updates derived records, and asks the downstream data worker to refresh
tournament statistics through RabbitMQ.

This repository owns the rating calculation and its direct SQL. The physical
database schema and migrations are owned by the sibling `otr-web` repository.
Treat that boundary as a cross-repository contract.

## Setup and commands

- Use stable Rust for builds, Clippy, and tests. CI uses nightly Rust only for
  rustfmt because `rustfmt.toml` enables nightly options.
- Start from `.env.example` and keep local credentials in the ignored `.env`.
- `CONNECTION_STRING` is required. `RUST_LOG`, `IGNORE_CONSTRAINTS`,
  `RABBITMQ_URL`, and `RABBITMQ_ROUTING_KEY` have CLI or runtime defaults shown
  by `cargo run -- --help`.
- Never commit credentials or include a RabbitMQ URL with credentials in logs,
  fixtures, or review output.

Useful commands:

```sh
cargo run -- --help
cargo run -- --log-level debug
cargo +nightly fmt -- --check
cargo clippy
cargo test
```

Running the binary is destructive. Do not use `cargo run` against production,
staging, or another shared database merely to verify a change. Use a disposable
database unless the user explicitly requests a real recomputation and confirms
the target. The `--ignore-constraints` option is not a dry-run or safety mode.

## Runtime flow

`src/main.rs` orchestrates one full recomputation inside a PostgreSQL
transaction:

1. Recalculate every game score placement.
2. Load tournaments, matches, games, and scores whose verification status is
   the persisted accepted value `4`, plus players and their ruleset data.
3. Build initial ratings, process matches chronologically with `OtrModel`, and
   apply the final decay pass.
4. Replace `player_ratings` and `rating_adjustments`, update
   `player_highest_ranks`, and remove derived stats for rejected data.
5. Publish refresh requests for tournaments whose source data is newer than
   their derived stats, then commit the transaction.

The main module boundaries are:

- `src/args.rs`: command-line and environment parsing.
- `src/model/`: initial ratings, Plackett-Luce processing, decay, ranking, and
  persisted ruleset/adjustment types.
- `src/database/`: physical SQL, row mappings, bulk writes, and transaction
  handling.
- `src/messaging/`: RabbitMQ configuration, topology, envelope, retry, and
  publishing behavior.
- `src/utils/`: progress reporting and test-data helpers.

Keep orchestration in `main`, domain calculations in `model`, persistence in
`database`, and broker behavior in `messaging`. Use structured `tracing` fields
for diagnostics and never log secrets.

## Rating invariants

- Matches are processed in chronological order. Timestamp ordering, match-end
  fallback behavior, decay boundaries, and ranking tie behavior are part of the
  rating contract.
- Persisted `Ruleset` values are `0..=5`; persisted `RatingAdjustmentType`
  values are `0..=3`. Do not reorder or renumber these enums.
- The two match methods, their weights, initial-rating bounds, decay constants,
  volatility behavior, and the absolute rating floor affect all historical
  results. Changes require focused unit tests and an explicit explanation of
  the expected rating impact.
- A verified game with fewer than two verified scores is skipped. Preserve the
  data-integrity warnings and cover any eligibility change with tests.
- Ranking, country ranking, percentile, rating history, and highest-rank updates
  must remain mutually consistent across every ruleset.
- Prefer deterministic fixtures with explicit timestamps and placements.
  Approximate floating-point assertions should state a meaningful tolerance.

## Database and messaging contracts

`src/database/db.rs` embeds SQL against tables and columns defined in
`../otr-web/packages/otr-core/src/db/schema.ts`; migrations live under
`../otr-web/apps/web/drizzle/`. When either side changes a physical name, type,
nullability rule, enum representation, verification rule, or relationship:

1. Inspect and update both repositories as one compatibility change.
2. Update this repository's row structs, SQL, COPY column lists, and
   `tests/database/schema.sql` together.
3. Use additive migrations and a rollout order that keeps the deployed web app,
   processor, and workers compatible. Never edit an applied migration.
4. Test against a fresh disposable database, not a developer or shared data
   store.

The processor truncates `rating_adjustments` and `player_ratings` with
`RESTART IDENTITY CASCADE`, writes with PostgreSQL `COPY`, updates
`game_scores`, and deletes stale match/tournament stats. Keep all recomputation
writes on the same connection and inside the transaction guard. Do not weaken
rollback behavior or assume the constraints option makes destructive testing
safe.

The RabbitMQ exchange, queue, routing key, AMQP properties, and camel-case JSON
shape of `ProcessTournamentStatsMessage` are contracts with the data worker.
Coordinate changes with the consumer and retain compatibility during rollout.
Broker startup failure currently allows rating processing to continue without
messages, and individual publish failures do not abort the run. Publishing also
happens before the database commit, so database writes and messages are not
atomic; do not design changes that assume exactly-once delivery or committed
data at publish time.

## Verification

Run the narrowest relevant test while iterating, then mirror CI before handing
off a change:

```sh
cargo +nightly fmt -- --check
cargo clippy
cargo test
git diff --check
```

- Model or constant changes need focused tests in the affected model module and
  full rating tests.
- SQL, transaction, or row-mapping changes need the database integration tests.
  They use Testcontainers and require a working Docker daemon.
- Message/configuration changes need serialization and publisher unit tests.
  The real-broker tests are ignored by default; run them only against an
  explicitly approved disposable RabbitMQ instance.
- If infrastructure prevents a check, report the exact skipped command and the
  reason rather than treating it as passed.

## Git conventions

```text
Branch: <short-kebab-case-description>

Commit:
<Imperative verb> <specific outcome>

<Optional explanation of why, compatibility impact, or validation details>

Refs #<issue>  # optional
```

- Branch names use two to five meaningful lowercase kebab-case terms, such as
  `agent-skills-refactor`, `rating-decay-window`, or `player-layout-fix`.
- Do not require `feature/`, `fix/`, `hotfix/`, `chore/`, usernames, vendors, or
  issue numbers.
- Tool-generated, Dependabot, upstream-sync, and scratch-worktree branches are
  exceptions.
- Commit subjects use sentence case and imperative mood, preferably at most 72
  characters, without a trailing period or Conventional Commit prefix.
- Avoid opaque subjects such as `fmt`, `prettier`, `cleanup`, or `(wip)`.
- Let GitHub add pull request numbers and merge metadata.
