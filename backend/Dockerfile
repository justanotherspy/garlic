# syntax=docker/dockerfile:1@sha256:87999aa3d42bdc6bea60565083ee17e86d1f3339802f543c0d03998580f9cb89

# --- Build stage -----------------------------------------------------------
FROM rust:slim-bookworm@sha256:edc389bf15702f98eab1afaec84c491529d07ac62da463cff24e7feee2577460 AS builder
WORKDIR /app

# Pre-build dependencies as a cacheable layer: compile against stub sources so
# this layer only invalidates when Cargo.toml / Cargo.lock change.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src \
    && echo "fn main() {}" > src/main.rs \
    && echo "" > src/lib.rs \
    && cargo build --release \
    && rm -rf src

# Build the real binary.
COPY src ./src
RUN touch src/main.rs src/lib.rs \
    && cargo build --release --bin garlic-backend \
    && strip target/release/garlic-backend

# --- Runtime stage ---------------------------------------------------------
FROM debian:bookworm-slim@sha256:7b140f374b289a7c2befc338f42ebe6441b7ea838a042bbd5acbfca6ec875818 AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --no-create-home garlic

COPY --from=builder /app/target/release/garlic-backend /usr/local/bin/garlic-backend

USER garlic
ENV GARLIC_BIND=0.0.0.0:8080 \
    RUST_LOG=info
EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -fsS "http://127.0.0.1:8080/health" || exit 1

ENTRYPOINT ["/usr/local/bin/garlic-backend"]
