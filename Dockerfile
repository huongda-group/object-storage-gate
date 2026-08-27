FROM node:22.22-slim AS frontend
WORKDIR /app/frontend
# `corepack enable` alone breaks: the corepack bundled in this image verifies the pnpm download
# against signing keys that npm has since rotated, so it dies with "Cannot find matching keyid"
# — an image that built last month stops building today with nothing in the repo having changed.
# Installing pnpm from the registry at the version package.json pins avoids the key check entirely.
COPY frontend/package.json frontend/pnpm-lock.yaml frontend/pnpm-workspace.yaml ./
RUN npm install -g "pnpm@$(node -p "require('./package.json').packageManager.split('@')[1]")"
RUN pnpm install --frozen-lockfile
COPY frontend/ ./
RUN pnpm build

FROM rust:1.94-slim-bookworm AS builder
WORKDIR /app
RUN apt-get update \
 && apt-get install -y --no-install-recommends pkg-config libssl-dev \
 && rm -rf /var/lib/apt/lists/*
COPY . .
# Baked into `App::app_version()` via option_env!("BUILD_SHA") — shows up in the
# boot banner. Absent locally, so the version falls back to "dev".
ARG BUILD_SHA
ENV BUILD_SHA=${BUILD_SHA}
RUN cargo build --release --locked

FROM debian:bookworm-20241202-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates libssl3 \
 && rm -rf /var/lib/apt/lists/* \
 && useradd -m -u 10001 app
WORKDIR /app
COPY --from=builder /app/target/release/object_storage_gate-cli /usr/local/bin/
COPY config/ config/
COPY --from=frontend /app/frontend/dist frontend/dist
# Chỗ chứa file SQLite khi deploy bằng docker-compose/sqlite.yml. Phải tồn tại và
# thuộc user app từ trong image: named volume mount lên đây kế thừa ownership đó,
# không thì container ghi không được.
RUN mkdir -p /app/data && chown 10001:10001 /app/data
USER app
ENV LOCO_ENV=production
EXPOSE 5150
# ponytail: this only proves the binary runs, because the runtime layer has no HTTP client.
# Ceiling: add curl and probe /_readiness if the orchestrator cannot do HTTP checks itself.
# /_health and /_ping are constants that never touch the database; /_readiness is the one
# that does, so point a real readiness probe there.
HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
  CMD ["object_storage_gate-cli", "--help"]
CMD ["object_storage_gate-cli", "start"]
