# Multi-stage Dockerfile for Revue (Static Musl Build)
FROM rust:1.85-alpine AS builder

WORKDIR /app
RUN apk add --no-cache musl-dev pkgconfig openssl-dev openssl-libs-static

# Copy source
COPY Cargo.toml Cargo.lock ./
COPY src/ ./src/

# Alpine's /bin/sh is busybox ash, which supports pipefail.
SHELL ["/bin/ash", "-o", "pipefail", "-c"]

# Build static binary
RUN target="$(rustc -vV | sed -n 's|host: ||p')" \
    && cargo build --release --target "$target" \
    && cp "target/$target/release/revue" /usr/local/bin/revue

# Runtime container
FROM alpine:3.21

RUN apk add --no-cache ca-certificates git bash

COPY --from=builder /usr/local/bin/revue /usr/local/bin/revue
RUN ln -s /usr/local/bin/revue /usr/local/bin/nix-revue

EXPOSE 8383

ENTRYPOINT ["/usr/local/bin/revue"]
CMD ["daemon", "--listen", "0.0.0.0:8383"]
