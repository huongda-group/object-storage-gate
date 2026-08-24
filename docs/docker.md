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
| `JWT_SECRET` | console session signing key — **must be base64**, see below |
| `OSG_MASTER_KEY` | base64 of 32 random bytes — AES-256-GCM key for every stored secret |
| `SERVER_HOST` | public origin, e.g. `https://osg.example.com`; no default, a missing value fails the boot |
| `POSTGRES_USER` / `POSTGRES_PASSWORD` | Postgres overlay; no defaults |
| `MYSQL_ROOT_PASSWORD` / `MYSQL_USER` / `MYSQL_PASSWORD` | MySQL overlay; no defaults |
| `RATE_LIMIT_PER_MINUTE` | requests per minute per IP after the burst; default `60` |
| `RATE_LIMIT_BURST` | back-to-back requests allowed before the rate applies; default `30` |
| `RATE_LIMIT_TRUST_PROXY` | `true` to read the client IP from `Forwarded` / `X-Forwarded-For`; default `false` |

`JWT_SECRET` must be valid base64: loco signs with
`EncodingKey::from_base64_secret`, so a non-base64 value lets the app boot and
then fails every login with a generic "unauthorized!" that looks exactly like a
wrong password. `App::after_context` now refuses to boot production on that too.

Generate both with `openssl rand -base64 32`. `App::after_context`
refuses to boot production when `OSG_MASTER_KEY` is missing, is not valid
base64, does not decode to exactly 32 bytes, or is the development key checked
into this repository. The check runs on the CLI subcommands too, so
`db migrate` is refused on the same terms.

`RATE_LIMIT_TRUST_PROXY` needs care in both directions. Left off behind a
reverse proxy, every request shares the proxy's IP and the per-IP limit becomes
a gateway-wide one. Turned on without a proxy that overwrites the header,
anyone resets their own bucket by sending a new value. Set it only when a proxy
you control sets the header.

For local development the database port has to reach the host, which the production
overlays deliberately do not do. Add `docker-compose/dev-ports.yml` for that, and never in
production:

```sh
docker compose -f docker-compose.yml -f docker-compose/postgres.yml \
               -f docker-compose/dev-ports.yml up -d db
```

Compose keeps the app in `docker-compose.yml` and adds one
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
| `DOCKERHUB_USERNAME` | Docker Hub account — the login, and the image namespace |
| `DOCKERHUB_TOKEN` | access token from Docker Hub → Account Settings → Personal access tokens |

The published name is `<DOCKERHUB_USERNAME>/object-storage-gate`. Change
`IMAGE_NAME` in the workflow to rename the repository half; publishing under a
different account means changing the two secrets. Docker Hub has no way to drop
the namespace — an unprefixed name resolves to `library/`, which only Docker's
own official images use.

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

## Migration là bước riêng trước khi rollout

`auto_migrate` đã tắt trên production. Nhiều replica cùng boot sẽ đua nhau chạy
`Migrator::up`, và trên MySQL một migration áp dụng dở làm kẹt schema mà không
có đường phục hồi nào được ghi lại.

```sh
docker compose run --rm app object_storage_gate-cli db migrate
docker compose up -d
```

## Pool phải có credential trước khi gateway phục vụ được

Migration `m20260818_000002_bucket_pool` tạo một pool tên `default` với
`physical_bucket = 'CHANGE-ME'` và **không có credential** khi cài đặt đã có
bucket từ trước. Mọi request S3 vào pool đó sẽ hỏng cho tới khi admin điền vào.

Sau khi migrate:

1. Đăng nhập console bằng tài khoản admin.
2. Vào Admin → Pool. Pool `default` hiện `CHƯA CÓ CREDENTIAL` màu đỏ.
3. Điền `physical_bucket` thật, `access_id`, `access_secret`, và `api_endpoint`
   nếu không dùng AWS.

Cài mới thì không có bucket nào nên không có pool `default`; tạo pool đầu tiên
bằng tay ở cùng màn hình đó.

Migration này `ALTER TABLE buckets` trên bảng có dữ liệu. Trên MySQL đó là một
lần rebuild bảng, khoá ghi trong lúc chạy — `buckets` nhỏ nên nhanh, nhưng vẫn
nên lên lịch. Và MySQL không rollback được DDL: nếu bước giữa chừng hỏng thì
bảng nằm ở trạng thái dở dang, nên hãy dump trước.

## Kiểm gateway cục bộ bằng MinIO

Bộ test dùng upstream giả. Một upstream thật bắt được những thứ upstream giả
không bao giờ bắt — ví dụ S3 và MinIO đều trả `411 MissingContentLength` cho một
`PutObject` đóng khung `Transfer-Encoding: chunked`.

```sh
docker run -d --name osg-upstream -p 9100:9000 \
  -e MINIO_ROOT_USER=upstream -e MINIO_ROOT_PASSWORD=upstream-secret \
  quay.io/minio/minio server /data

docker run --rm --network host \
  -e AWS_ACCESS_KEY_ID=upstream -e AWS_SECRET_ACCESS_KEY=upstream-secret \
  amazon/aws-cli s3 mb s3://osg-main \
  --endpoint-url http://localhost:9100 --region us-east-1
```

Chạy gateway. `SERVER_BINDING` phải là `0.0.0.0`: mặc định `localhost` chỉ nghe
127.0.0.1, và container trên Docker Desktop macOS không tới được.

```sh
SERVER_BINDING=0.0.0.0 DB_TYPE=sqlite LOCO_ENV=development cargo loco start
```

Qua console: tạo pool `provider=minio`, `api_endpoint=http://localhost:9100`,
`physical_bucket=osg-main`, credential của MinIO; tạo bucket `media-cdn`; tạo
access key có `read`, `write`, `delete`, `list`.

Rồi đi một vòng ghi–đọc–xoá bằng aws-cli thật:

```sh
export AWS_ACCESS_KEY_ID=OSG… AWS_SECRET_ACCESS_KEY=…
H=http://host.docker.internal:5150
head -c 1048576 /dev/urandom > /tmp/1mb.bin

docker run --rm --add-host=host.docker.internal:host-gateway -v /tmp:/w \
  -e AWS_ACCESS_KEY_ID -e AWS_SECRET_ACCESS_KEY amazon/aws-cli \
  s3 cp /w/1mb.bin s3://media-cdn/img/1mb.bin --endpoint-url $H --region us-east-1
```

Kiểm layout vật lý — đây là chỗ bắt được lỗi nặng nhất:

```sh
docker run --rm --network host \
  -e AWS_ACCESS_KEY_ID=upstream -e AWS_SECRET_ACCESS_KEY=upstream-secret \
  amazon/aws-cli s3 ls --recursive s3://osg-main \
  --endpoint-url http://localhost:9100 --region us-east-1
```

Phải ra `{user_pid}/media-cdn/img/1mb.bin`. Nếu thiếu `{user_pid}` thì hai user
đặt cùng tên bucket sẽ ghi đè lên nhau.

Kiểm quota: hạ quota bucket xuống dưới kích thước file rồi upload lại — phải
nhận `QuotaExceeded`, và `s3 ls` trên MinIO **không** được thấy file đó.

Dọn: `docker rm -f osg-upstream`.

## Sao lưu

Chụp một bản dump trước mỗi lần rollout có migration. `cargo loco db dump` chỉ
chạy trên Postgres và SQLite — trên MySQL dùng `mysqldump`.

Migration `m20260817_000001_auth_teardown` **xoá bảy cột** khỏi `users`. `down()`
dựng lại cấu trúc nhưng không dựng lại dữ liệu.

## Healthcheck

`/_health` và `/_ping` trả `{"ok":true}` vô điều kiện và không chạm database.
Chỉ `/_readiness` mới ping DB. Trỏ liveness probe vào `/_ping`, readiness probe
vào `/_readiness`. `HEALTHCHECK` trong Dockerfile chỉ chứng minh binary chạy
được, vì lớp runtime không có HTTP client.

## TLS

Image phục vụ HTTP thuần trên `0.0.0.0:5150`. Phải có một reverse proxy kết thúc
TLS ở phía trước — SigV4 credential và JWT đi qua dây, và không có gì trong stack
này tự mã hoá chúng. Nếu proxy đó set `X-Forwarded-For`, bật
`RATE_LIMIT_TRUST_PROXY=true` để rate limit tính theo IP client thật.
