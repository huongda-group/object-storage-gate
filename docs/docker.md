# Docker

How the image is built, run against each database backend, and published to
Docker Hub.

## The image

`Dockerfile` is a three-stage build:

1. `node:22-slim` — `pnpm build` produces `frontend/dist`.
2. `rust:slim-bookworm` — `cargo build --release --locked`. All three database
   drivers (`sqlx-postgres`, `sqlx-mysql`, `sqlx-sqlite`) are compiled in, so one
   image serves every backend; the URI scheme decides at boot.
3. `debian:bookworm-slim` — the binary, `config/`, `frontend/dist`, running as
   uid 10001. `LOCO_ENV=production`, port 5150.

`/app/data` exists in the image owned by uid 10001 so a named volume mounted
there stays writable — that is where the SQLite file lives.

`BUILD_SHA` is a build arg baked into `App::app_version()` and printed in the boot
banner. Without it the version reads `dev`.

## Build locally

```sh
docker build -t object-storage-gate:dev --build-arg BUILD_SHA=$(git rev-parse HEAD) .
```

## Run

Production config requires three environment variables and refuses to boot
without them:

| Variable | Purpose |
|---|---|
| `DATABASE_URL` | full URI; the scheme picks the backend |
| `JWT_SECRET` | console session signing key |
| `OSG_MASTER_KEY` | base64 of 32 random bytes — AES-256-GCM key for every stored secret |

Generate a master key with `openssl rand -base64 32`. Reusing the checked-in
development key in production is refused by `App::after_context`.

Compose keeps the stack (app + valkey) in `docker-compose.yml` and adds one
overlay per database:

```sh
docker compose -f docker-compose.yml -f docker-compose/postgres.yml up -d
docker compose -f docker-compose.yml -f docker-compose/mysql.yml    up -d
docker compose -f docker-compose.yml -f docker-compose/sqlite.yml   up -d
```

Bare `docker run`, one backend each:

```sh
# Postgres
docker run -p 5150:5150 \
  -e DATABASE_URL=postgres://loco:loco@host.docker.internal:5432/osg \
  -e JWT_SECRET=… -e OSG_MASTER_KEY=… object-storage-gate:dev

# MySQL (8.0.13+)
docker run -p 5150:5150 \
  -e DATABASE_URL=mysql://loco:loco@host.docker.internal:3306/osg \
  -e JWT_SECRET=… -e OSG_MASTER_KEY=… object-storage-gate:dev

# SQLite — one writer, so a single container only
docker run -p 5150:5150 -v osgdata:/app/data \
  -e DATABASE_URL='sqlite:///app/data/osg.sqlite?mode=rwc' \
  -e JWT_SECRET=… -e OSG_MASTER_KEY=… object-storage-gate:dev
```

The server auto-migrates on boot (`database.auto_migrate: true`), so a fresh
volume or empty database comes up ready.

## Publish

`.github/workflows/docker.yaml` builds `linux/amd64` + `linux/arm64` with Buildx
and pushes to Docker Hub. It runs on any bare version tag — `0.1.0`, `1.2.3`, no
leading `v` — and on manual dispatch with an explicit tag input.

Repository secrets it needs:

| Secret | Value |
|---|---|
| `DOCKERHUB_USERNAME` | Docker Hub account the workflow logs in as |
| `DOCKERHUB_TOKEN` | access token from Docker Hub → Account Settings → Personal access tokens |

The published name is `<namespace>/object-storage-gate`, and the namespace is
independent of the login account:

| Where | Setting | Result |
|---|---|---|
| nothing set | — | `<DOCKERHUB_USERNAME>/object-storage-gate` |
| repo **variable** `DOCKERHUB_NAMESPACE` | e.g. `hdg` | `hdg/object-storage-gate` |
| workflow `env.IMAGE_NAME` | e.g. `osg-gateway` | `<namespace>/osg-gateway` |

Set the variable under Settings → Secrets and variables → Actions → *Variables*
(not Secrets). The login token must be allowed to push there: for an org
namespace that means the account is an org member with write access to the repo,
or the token is an org access token.

Tags produced from `1.2.3`: `1.2.3`, `1.2`, and `latest`.

```sh
git tag 0.1.0 && git push origin 0.1.0
```

arm64 is emulated through QEMU, so that leg of the build is several times slower
than amd64. Drop `linux/arm64` from `platforms:` if the wait costs more than the
architecture is worth.

Pushing by hand instead:

```sh
docker login
docker buildx build --platform linux/amd64,linux/arm64 \
  -t youruser/object-storage-gate:0.1.0 \
  --build-arg BUILD_SHA=$(git rev-parse HEAD) --push .
```
