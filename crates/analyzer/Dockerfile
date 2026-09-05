# syntax=docker/dockerfile:1.7
FROM rust:1.85-bookworm AS builder
WORKDIR /build
COPY Cargo.toml Cargo.toml
COPY Cargo.lock Cargo.lock
COPY crates/analyzer crates/analyzer
RUN --mount=type=cache,id=newhatch-cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=newhatch-cargo-target,target=/build/target,sharing=locked \
    cargo build --locked --release --package newhatch-analyzer \
    && cp /build/target/release/newhatch-analyzer /build/newhatch-analyzer

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/newhatch-analyzer /usr/local/bin/newhatch-analyzer
ENV DATA_DIR=/data \
    LISTEN_ADDR=0.0.0.0:3000 \
    RUST_LOG=newhatch=info
VOLUME ["/data"]
EXPOSE 3000
ENTRYPOINT ["newhatch-analyzer"]
