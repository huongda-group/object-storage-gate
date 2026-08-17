FROM node:22-slim AS frontend
WORKDIR /app/frontend
RUN corepack enable
COPY frontend/package.json frontend/pnpm-lock.yaml frontend/pnpm-workspace.yaml ./
RUN pnpm install --frozen-lockfile
COPY frontend/ ./
RUN pnpm build

FROM rust:slim-bookworm AS builder
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

FROM debian:bookworm-slim
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
CMD ["object_storage_gate-cli", "start"]
