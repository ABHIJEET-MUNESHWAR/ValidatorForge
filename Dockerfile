# syntax=docker/dockerfile:1

# ---- builder ----
FROM rust:1.89-slim-bookworm AS builder
WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config \
    && rm -rf /var/lib/apt/lists/*

COPY . .
RUN cargo build --release -p validatorforge-node

# ---- runtime ----
FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -r -u 10001 validatorforge

WORKDIR /app
COPY --from=builder /app/target/release/validatorforge-node /usr/local/bin/validatorforge-node

USER validatorforge
ENV VALIDATORFORGE_HOST=0.0.0.0 \
    VALIDATORFORGE_PORT=8080 \
    VALIDATORFORGE_LOG_JSON=true \
    RUST_LOG=info

EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/validatorforge-node"]
# Default to the GraphQL ops server; override with e.g. `docker run … plan --kind upgrade`.
CMD ["serve"]
