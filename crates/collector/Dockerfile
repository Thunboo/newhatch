# syntax=docker/dockerfile:1.7
FROM rust:1.85-bookworm AS builder
WORKDIR /build
COPY Cargo.toml Cargo.toml
COPY Cargo.lock Cargo.lock
COPY crates/protocol crates/protocol
COPY crates/collector crates/collector
COPY crates/analyzer crates/analyzer
RUN --mount=type=cache,id=newhatch-cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=newhatch-cargo-target,target=/build/target,sharing=locked \
    cargo build --locked --release --package newhatch-collector \
    && cp /build/target/release/newhatch-collector /build/newhatch-collector

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/newhatch-collector /usr/local/bin/newhatch-collector
ENV CAPTURE_INTERFACE=eth0 \
    RECEIVER_PORT=39090 \
    COLLECTOR_ID=vulnbox-1 \
    COLLECTOR_QUEUE_CAPACITY=8192 \
    RUST_LOG=newhatch=info
ENTRYPOINT ["newhatch-collector"]
