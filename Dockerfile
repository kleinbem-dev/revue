# Multi-stage Dockerfile for Revue (Rust + SvelteKit Dashboard Assets)
FROM node:24-alpine AS web-builder

WORKDIR /web
COPY web/package.json web/package-lock.json web/.npmrc ./
RUN npm ci
COPY web/ ./
RUN npm test && npm run check && npm run build

FROM rust:1.98-alpine AS builder

WORKDIR /app
RUN apk add --no-cache musl-dev pkgconf sqlite-dev sqlite-static

ENV LIBSQLITE3_SYS_USE_PKG_CONFIG=1

COPY Cargo.toml Cargo.lock ./
COPY src/ ./src/

SHELL ["/bin/ash", "-o", "pipefail", "-c"]

RUN target="$(rustc -vV | sed -n 's|host: ||p')" \
    && cargo build --release --target "$target" \
    && cp "target/$target/release/revue" /usr/local/bin/revue

FROM alpine:3.21

RUN apk add --no-cache ca-certificates git bash sqlite-libs \
    && addgroup -S -g 10001 revue \
    && adduser -S -u 10001 -G revue -h /var/lib/revue -s /bin/sh revue \
    && mkdir -p /var/lib/revue /usr/local/share/revue/web \
    && chown -R revue:revue /var/lib/revue

COPY --from=builder /usr/local/bin/revue /usr/local/bin/revue
COPY --from=web-builder /web/build /usr/local/share/revue/web

USER revue
WORKDIR /var/lib/revue

EXPOSE 8383

ENTRYPOINT ["/usr/local/bin/revue"]
CMD ["daemon", "--listen", "0.0.0.0:8383"]
