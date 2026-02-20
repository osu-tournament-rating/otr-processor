# otr-processor

Rust rating calculation engine using PlackettLuce model with OpenSkill.

## Commands

```bash
cargo build                         # Debug build
cargo build --release               # Release build
cargo run --release                 # Run processor
cargo test --all-features           # All tests
cargo +nightly fmt                  # Format code
cargo clippy --all-features --all-targets  # Lint (nightly)
```

## Environment

```bash
# Required
CONNECTION_STRING=postgresql://user:pass@localhost:5432/database

# Optional
RUST_LOG=info                       # trace|debug|info|warn|error
IGNORE_CONSTRAINTS=false            # Skip table access checks
RABBITMQ_URL=amqp://admin:admin@localhost:5672
RABBITMQ_ROUTING_KEY=processing.stats.tournaments
```

## Architecture

```
src/
├── main.rs           # Entry point, orchestration
├── model/            # Rating calculation core
│   ├── otr_model.rs  # PlackettLuce implementation
│   ├── decay.rs      # Rating/volatility decay
│   ├── constants.rs  # Model parameters
│   └── structures/   # Ruleset enum, adjustment types
├── database/         # PostgreSQL integration
│   ├── db.rs         # Queries, transaction management
│   └── db_structs.rs # Data models
└── messaging/        # RabbitMQ publishing
```

## Key Concepts

- **Rulesets**: Standard, Taiko, Catch, Mania (Other/4K/7K)
- **Two-method calculation**: Method A (skip missed games) + Method B (assume last place)
- **Decay**: Applies after 184 days inactivity, floor at 1000 or peak-based minimum
- **Weekly updates**: Ratings recalculated Tuesday 12:00 UTC

## Model Constants (src/model/constants.rs)

- `DEFAULT_VOLATILITY=400.0` - Initial uncertainty
- `DECAY_DAYS=184` - Inactivity threshold
- `DECAY_RATE=3.0` - Points lost per cycle
- `WEIGHT_A=0.9` / `WEIGHT_B=0.1` - Method weights

## Database Patterns

- Uses `tokio-postgres` with async transactions
- Batch processing: 500 matches, 5000 players per batch
- Transaction guard with auto-rollback on error
