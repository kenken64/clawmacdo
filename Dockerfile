FROM rust:1-bookworm AS builder

WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY assets ./assets

RUN cargo build --release -p clawmacdo-cli

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        bash \
        curl \
        libssl3 \
        openssh-client \
        sudo \
        unzip \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/clawmacdo /usr/local/bin/clawmacdo

ENV CLAWMACDO_BIND=0.0.0.0
ENV CLAWMACDO_STATE_DIR=/app/.clawmacdo

EXPOSE 3456

CMD ["sh", "-c", "mkdir -p \"$CLAWMACDO_STATE_DIR\" && exec clawmacdo serve --port \"${PORT:-3456}\""]
