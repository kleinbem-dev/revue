# Multi-stage Dockerfile for Revue (Static Musl Build + Dashboard Assets)
FROM node:25-alpine AS web-builder

WORKDIR /web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

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

RUN apk add --no-cache ca-certificates git bash \
    && addgroup -S -g 10001 revue \
    && adduser -S -u 10001 -G revue -h /var/lib/revue -s /bin/sh revue \
    && mkdir -p /var/lib/revue /usr/local/share/revue/web \
    && chown -R revue:revue /var/lib/revue

COPY --from=builder /usr/local/bin/revue /usr/local/bin/revue
COPY --from=web-builder /web/build /usr/local/share/revue/web
RUN ln -s /usr/local/bin/revue /usr/local/bin/nix-revue

USER revue
WORKDIR /var/lib/revue

EXPOSE 8383

ENTRYPOINT ["/usr/local/bin/revue"]
CMD ["daemon", "--listen", "0.0.0.0:8383"]
