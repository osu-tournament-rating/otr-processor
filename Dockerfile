# Build stage
FROM rust:latest as builder

WORKDIR /usr/src/otr-processor

# Copy source and build the application
COPY . .
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

# Copy the binary from builder
COPY --from=builder /usr/src/otr-processor/target/release/otr-processor /usr/local/bin/otr-processor

ENTRYPOINT ["/usr/local/bin/otr-processor"]
