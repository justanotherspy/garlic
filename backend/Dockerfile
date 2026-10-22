# syntax=docker/dockerfile:1@sha256:4edf897a3ffa55b89f906fc8cc78afdb3f1834cc9c7083565e611a8a7d5fe99e

# --- Build stage -----------------------------------------------------------
FROM rust:slim-bookworm@sha256:54e39a2f4480fd827ebfa47736212703a1e46be0fddf7e1590aa0e94b8d0cae1 AS builder
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
FROM debian:bookworm-slim@sha256:7ff6ba31527b9e336162fa061746c366c4b2032f4d2cff2078bea43f78efe3f8 AS runtime
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
