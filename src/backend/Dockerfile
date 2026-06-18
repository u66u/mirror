FROM rust:1.93.0-bookworm AS build

WORKDIR /workspace
COPY Cargo.toml Cargo.lock ./
COPY src/backend/Cargo.toml src/backend/Cargo.toml
COPY src/backend/src src/backend/src
COPY src/backend/migrations src/backend/migrations
RUN cargo build --release -p mirror-backend --bin api --bin worker --bin maintenance

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates ffmpeg \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system --gid 10001 mirror \
    && useradd --system --uid 10001 --gid 10001 --home-dir /nonexistent --shell /usr/sbin/nologin mirror \
    && mkdir -p /var/lib/mirror/storage \
    && chown -R 10001:10001 /var/lib/mirror

COPY --from=build /workspace/target/release/api /usr/local/bin/mirror-api
COPY --from=build /workspace/target/release/worker /usr/local/bin/mirror-worker
COPY --from=build /workspace/target/release/maintenance /usr/local/bin/mirror-maintenance

USER 10001:10001
WORKDIR /var/lib/mirror

ENV MIRROR_BIND_ADDR=0.0.0.0:8080
ENV MIRROR_STORAGE_ROOT=/var/lib/mirror/storage

ENTRYPOINT ["/usr/local/bin/mirror-api"]
