FROM rust:1.96.0-bookworm AS build

WORKDIR /workspace
COPY Cargo.toml Cargo.lock ./
COPY src/backend/Cargo.toml src/backend/Cargo.toml
COPY src/backend/src src/backend/src
COPY src/backend/migrations src/backend/migrations
RUN cargo build --release -p mirror-backend --bin api --bin worker --bin maintenance

FROM debian:bookworm-slim AS onnxruntime

ARG ONNXRUNTIME_VERSION=1.24.0
ARG ONNXRUNTIME_ARCH=x64

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl tar \
    && curl -fsSL \
        "https://github.com/microsoft/onnxruntime/releases/download/v${ONNXRUNTIME_VERSION}/onnxruntime-linux-${ONNXRUNTIME_ARCH}-${ONNXRUNTIME_VERSION}.tgz" \
        -o /tmp/onnxruntime.tgz \
    && mkdir -p /opt/onnxruntime \
    && tar -xzf /tmp/onnxruntime.tgz --strip-components=1 -C /opt/onnxruntime \
    && rm -rf /var/lib/apt/lists/* /tmp/onnxruntime.tgz

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        ffmpeg \
        libgomp1 \
        libheif-examples \
        libstdc++6 \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system --gid 10001 mirror \
    && useradd --system --uid 10001 --gid 10001 --home-dir /nonexistent --shell /usr/sbin/nologin mirror \
    && mkdir -p /var/lib/mirror/storage \
    && chown -R 10001:10001 /var/lib/mirror

COPY --from=onnxruntime /opt/onnxruntime/lib/libonnxruntime.so* /usr/local/lib/
COPY --from=onnxruntime /opt/onnxruntime/lib/libonnxruntime_providers_shared.so* /usr/local/lib/
COPY --from=build /workspace/target/release/api /usr/local/bin/mirror-api
COPY --from=build /workspace/target/release/worker /usr/local/bin/mirror-worker
COPY --from=build /workspace/target/release/maintenance /usr/local/bin/mirror-maintenance

USER 10001:10001
WORKDIR /var/lib/mirror

ENV MIRROR_BIND_ADDR=0.0.0.0:8080
ENV MIRROR_STORAGE_ROOT=/var/lib/mirror/storage
ENV LD_LIBRARY_PATH=/usr/local/lib
ENV ORT_DYLIB_PATH=/usr/local/lib/libonnxruntime.so

ENTRYPOINT ["/usr/local/bin/mirror-api"]
