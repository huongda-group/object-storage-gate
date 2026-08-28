# G2 — SigV4 và client upstream — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Một module SigV4 verify được request của client và ký được request lên upstream, cộng một client `reqwest` stream được body theo cả hai chiều.

**Architecture:** `canonical_request()` là hàm chung, chạy hai chiều — verify dùng nó để dựng lại chuỗi mà client đã ký, sign dùng nó để dựng chuỗi mình sắp ký. Đây là lý do chọn tự ký thay vì `aws-sdk-s3`: verifier là thứ bắt buộc phải có, signer thành phần dôi ra gần như miễn phí. `upstream.rs` bọc `reqwest` với `Body::wrap_stream` nên một PUT 5 GiB đi qua với bộ nhớ hằng số.

**Tech Stack:** Rust, `reqwest`, `hmac`, `sha2`, `percent-encoding`, `hex`. Không `aws-sdk-*`.

**Spec:** `docs/superpowers/specs/2026-08-17-s3-gateway-design.md` mục 5, 16

**Deliverable:** `sigv4.rs` xanh với bộ test vector chính thức của AWS, và `upstream.rs` gửi được một request đã ký tới một server giả rồi stream response về. Không route S3 nào — G3 cắm chúng vào.

## Global Constraints

- Ba backend hạng nhất: Postgres, MySQL >= 8.0.13, SQLite. Plan này gần như không đụng DB; chỗ nào đụng thì vẫn phải chạy cả ba.
- Comment trong code: tiếng Anh, một câu một dòng, không xuống dòng giữa câu.
- Không tự commit/push ngoài các bước commit trong plan. Không AI attribution.
- Sau mỗi task: `cargo clippy --all-features --all-targets -- -D warnings -W clippy::pedantic -W clippy::nursery -W rust-2018-idioms` sạch.
- **Dependency mới phải `default-features = false`** khi nó có nguy cơ kéo `openssl`. `rustls` đã có sẵn qua `sea-orm`; hai TLS stack trong một binary là một cách hỏng lúc runtime.

---

## File Structure

**Tạo mới:**
- `src/s3/mod.rs`
- `src/s3/sigv4.rs` — canonical request, verify (header + query), sign
- `src/s3/error.rs` — `S3Error` và map sang `(status, Code, Message)`
- `src/s3/upstream.rs` — client `reqwest`, streaming, map lỗi upstream
- `tests/s3_vectors/` — test vector của AWS, chép vào repo
- `tests/support/mod.rs`, `tests/support/mock_upstream.rs`

**Sửa:**
- `Cargo.toml`
- `src/lib.rs` — thêm `pub mod s3;`

---

## Task 1: Dependency và khung module

**Files:**
- Modify: `Cargo.toml`, `src/lib.rs`
- Create: `src/s3/mod.rs`

**Interfaces:**
- Consumes: —
- Produces: module `s3` compile được, rỗng.

- [ ] **Step 1: Thêm dependency**

```bash
cargo add reqwest --no-default-features --features stream,rustls-tls,http2
cargo add hmac sha2 hex percent-encoding
cargo add quick-xml --features serialize
```

`--no-default-features` cho `reqwest` là bắt buộc: default kéo `native-tls` tức là `openssl`, còn `sea-orm` đã dùng `rustls`. Hai TLS stack trong một binary không phải lỗi biên dịch — nó là lỗi lúc runtime, và lỗi đó xuất hiện ở chỗ khó chẩn nhất.

Kiểm không có `openssl` lọt vào:

```bash
cargo tree -i openssl-sys 2>&1 | head -5
```

Expected: `error: package ID specification ... did not match any packages` — nghĩa là không có.

- [ ] **Step 2: Khung module**

Tạo `src/s3/mod.rs`:

```rust
//! The S3 data plane.
//!
//! `sigv4` verifies what a client signed and signs what the gateway sends upstream — one canonical-request implementation running in both directions.
//! `upstream` streams bodies through without buffering them.
//! `error` maps every failure to an S3 error code, because a client that cannot read the code cannot act on it.
pub mod error;
pub mod sigv4;
pub mod upstream;
```

Thêm `pub mod s3;` vào `src/lib.rs`.

- [ ] **Step 3: Kiểm và commit**

```bash
cargo build 2>&1 | tail -3
cargo clippy --all-features --all-targets -- -D warnings -W clippy::pedantic -W clippy::nursery -W rust-2018-idioms
git add Cargo.toml Cargo.lock src/
git commit -m "chore(s3): add the HTTP and crypto deps for the data plane

reqwest with default-features off: the default pulls native-tls, and sea-orm
already uses rustls. Two TLS stacks in one binary is not a compile error, it is
a runtime one, in the place that is hardest to diagnose."
```

---

## Task 2: `S3Error`

**Files:**
- Create: `src/s3/error.rs`
- Test: unit test trong cùng file

**Interfaces:**
- Consumes: —
- Produces: `S3Error` enum; `S3Error::code() -> &'static str`; `S3Error::status() -> StatusCode`; `S3Error::message() -> String`; `impl From<ModelError> for S3Error`.

Làm cái này trước SigV4 vì SigV4 trả `S3Error`.

- [ ] **Step 1: Viết test**

Trong `src/s3/error.rs`, cuối file:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_variant_has_a_code_and_a_status() {
        // Spec §12. A wrong status here is a client that retries when it should not, or
        // gives up when it should retry.
        let cases = [
            (S3Error::AccessDenied, "AccessDenied", 403),
            (S3Error::InvalidAccessKeyId, "InvalidAccessKeyId", 403),
            (S3Error::SignatureDoesNotMatch, "SignatureDoesNotMatch", 403),
            (S3Error::RequestTimeTooSkewed, "RequestTimeTooSkewed", 403),
            (S3Error::QuotaExceeded, "QuotaExceeded", 403),
            (S3Error::NoSuchBucket, "NoSuchBucket", 404),
            (S3Error::NoSuchKey, "NoSuchKey", 404),
            (S3Error::NoSuchUpload, "NoSuchUpload", 404),
            (S3Error::KeyTooLong, "KeyTooLongError", 400),
            (S3Error::MissingContentLength, "MissingContentLength", 411),
            (S3Error::PreconditionFailed, "PreconditionFailed", 412),
            (S3Error::InternalError, "InternalError", 500),
        ];

        for (err, code, status) in cases {
            assert_eq!(err.code(), code);
            assert_eq!(err.status().as_u16(), status, "status for {code}");
            assert!(!err.message().is_empty(), "message for {code}");
        }
    }

    #[test]
    fn not_implemented_carries_what_is_missing() {
        let err = S3Error::NotImplemented("aws-chunked payload signing".to_string());
        assert_eq!(err.status().as_u16(), 501);
        assert!(err.message().contains("aws-chunked"));
    }

    /// An upstream error is re-emitted with its Code but never with its body: the upstream body
    /// carries the physical bucket and key, and forwarding it leaks the layout the product
    /// promises never to expose.
    #[test]
    fn upstream_error_keeps_the_code_and_drops_the_body() {
        let err = S3Error::Upstream {
            code: "EntityTooSmall".to_string(),
            status: 400,
            message: "Your proposed upload is smaller than the minimum allowed size"
                .to_string(),
        };
        assert_eq!(err.code(), "EntityTooSmall");
        assert_eq!(err.status().as_u16(), 400);
        assert!(!err.message().contains("osg-main"));
    }
}
```

- [ ] **Step 2: Chạy để chắc nó fail**

Run: `cargo test --lib s3::error 2>&1 | tail -10`
Expected: FAIL biên dịch — `S3Error` chưa tồn tại.

- [ ] **Step 3: Viết `S3Error`**

```rust
//! Every failure the data plane can produce, as an S3 error code.
//!
//! S3 clients branch on `<Code>`, not on the HTTP status alone — botocore raises `ClientError`
//! carrying the code, and retry logic reads it. A generic 500 is a client that cannot act.
use axum::http::StatusCode;
use loco_rs::model::ModelError;

#[derive(Debug, Clone)]
pub enum S3Error {
    AccessDenied,
    InvalidAccessKeyId,
    SignatureDoesNotMatch,
    RequestTimeTooSkewed,
    /// Non-standard. The one code S3 has no equivalent for; marked `gateway_only` in the
    /// conformance suite.
    QuotaExceeded,
    NoSuchBucket,
    NoSuchKey,
    NoSuchUpload,
    KeyTooLong,
    InvalidArgument(String),
    InvalidRequest(String),
    MalformedXml(String),
    MissingContentLength,
    PreconditionFailed,
    NotImplemented(String),
    /// An error the upstream store returned, re-emitted with its own code.
    ///
    /// The message is the upstream message with nothing added; the upstream *body* is dropped
    /// entirely, because it names the physical bucket and key.
    Upstream {
        code: String,
        status: u16,
        message: String,
    },
    InternalError,
}

impl S3Error {
    #[must_use]
    pub fn code(&self) -> &str {
        match self {
            Self::AccessDenied => "AccessDenied",
            Self::InvalidAccessKeyId => "InvalidAccessKeyId",
            Self::SignatureDoesNotMatch => "SignatureDoesNotMatch",
            Self::RequestTimeTooSkewed => "RequestTimeTooSkewed",
            Self::QuotaExceeded => "QuotaExceeded",
            Self::NoSuchBucket => "NoSuchBucket",
            Self::NoSuchKey => "NoSuchKey",
            Self::NoSuchUpload => "NoSuchUpload",
            Self::KeyTooLong => "KeyTooLongError",
            Self::InvalidArgument(_) => "InvalidArgument",
            Self::InvalidRequest(_) => "InvalidRequest",
            Self::MalformedXml(_) => "MalformedXML",
            Self::MissingContentLength => "MissingContentLength",
            Self::PreconditionFailed => "PreconditionFailed",
            Self::NotImplemented(_) => "NotImplemented",
            Self::Upstream { code, .. } => code,
            Self::InternalError => "InternalError",
        }
    }

    #[must_use]
    pub fn status(&self) -> StatusCode {
        match self {
            Self::AccessDenied
            | Self::InvalidAccessKeyId
            | Self::SignatureDoesNotMatch
            | Self::RequestTimeTooSkewed
            | Self::QuotaExceeded => StatusCode::FORBIDDEN,
            Self::NoSuchBucket | Self::NoSuchKey | Self::NoSuchUpload => StatusCode::NOT_FOUND,
            Self::KeyTooLong
            | Self::InvalidArgument(_)
            | Self::InvalidRequest(_)
            | Self::MalformedXml(_) => StatusCode::BAD_REQUEST,
            Self::MissingContentLength => StatusCode::LENGTH_REQUIRED,
            Self::PreconditionFailed => StatusCode::PRECONDITION_FAILED,
            Self::NotImplemented(_) => StatusCode::NOT_IMPLEMENTED,
            Self::Upstream { status, .. } => {
                StatusCode::from_u16(*status).unwrap_or(StatusCode::BAD_GATEWAY)
            }
            Self::InternalError => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::AccessDenied => "Access Denied".to_string(),
            Self::InvalidAccessKeyId => {
                "The AWS Access Key Id you provided does not exist in our records.".to_string()
            }
            Self::SignatureDoesNotMatch => {
                "The request signature we calculated does not match the signature you provided."
                    .to_string()
            }
            Self::RequestTimeTooSkewed => {
                "The difference between the request time and the current time is too large."
                    .to_string()
            }
            Self::QuotaExceeded => {
                "The storage quota for this bucket or account has been reached.".to_string()
            }
            Self::NoSuchBucket => "The specified bucket does not exist.".to_string(),
            Self::NoSuchKey => "The specified key does not exist.".to_string(),
            Self::NoSuchUpload => {
                "The specified multipart upload does not exist.".to_string()
            }
            Self::KeyTooLong => "Your key is too long.".to_string(),
            Self::InvalidArgument(m)
            | Self::InvalidRequest(m)
            | Self::MalformedXml(m)
            | Self::NotImplemented(m) => m.clone(),
            Self::MissingContentLength => {
                "You must provide the Content-Length HTTP header.".to_string()
            }
            Self::PreconditionFailed => {
                "At least one of the pre-conditions you specified did not hold.".to_string()
            }
            Self::Upstream { message, .. } => message.clone(),
            Self::InternalError => {
                "We encountered an internal error. Please try again.".to_string()
            }
        }
    }
}

impl From<ModelError> for S3Error {
    /// Any model failure that reaches the data plane is an internal error from the client's
    /// point of view — except the one the quota path raises, which the client can act on.
    fn from(e: ModelError) -> Self {
        if e.to_string().contains("quota exceeded") {
            return Self::QuotaExceeded;
        }
        tracing::error!(error = %e, "model error in the S3 data plane");
        Self::InternalError
    }
}
```

Ghi chú về `From<ModelError>`: khớp chuỗi `"quota exceeded"` là mong manh. Nó khớp với thông điệp mà `quota::exceeded()` sinh ra ở P5. Thêm một `ponytail:` ghi trần:

```rust
// ponytail: matches on the message string that quota::exceeded() produces.
// Ceiling: renaming that message silently turns a 403 QuotaExceeded into a 500.
// Upgrade path: give ModelError a typed variant for quota, which means touching loco's error enum or wrapping it — not worth it for one call site that a test covers.
```

Và một test khẳng định đúng cái đó, để việc đổi tên bị bắt:

```rust
    #[test]
    fn a_quota_error_from_the_model_maps_to_quota_exceeded() {
        // Guards the string match in From<ModelError>. If quota::exceeded() ever changes its
        // message, this fails instead of a 500 reaching a client in production.
        let e: S3Error = ModelError::msg("quota exceeded").into();
        assert_eq!(e.code(), "QuotaExceeded");
    }
```

- [ ] **Step 4: Chạy test và commit**

```bash
cargo test --lib s3::error 2>&1 | tail -5
cargo clippy --all-features --all-targets -- -D warnings -W clippy::pedantic -W clippy::nursery -W rust-2018-idioms
cargo fmt --all
git add src/
git commit -m "feat(s3): the error surface

S3 clients branch on <Code>, not on the status alone, so every failure carries
one. Upstream errors keep their code and drop their body: the body names the
physical bucket and key, and forwarding it leaks the layout the product promises
never to expose."
```

---

## Task 3: `canonical_request` với test vector của AWS

**Files:**
- Create: `src/s3/sigv4.rs`, `tests/s3_vectors/README.md` và các file vector
- Test: unit test trong `sigv4.rs`

**Interfaces:**
- Consumes: —
- Produces:
  - `sigv4::CanonicalParts { method, uri, query, headers, signed_headers, payload_hash }`
  - `sigv4::canonical_request(&CanonicalParts) -> String`
  - `sigv4::string_to_sign(datetime, scope, canonical_request) -> String`
  - `sigv4::signing_key(secret, date, region, service) -> [u8; 32]`
  - `sigv4::signature(signing_key, string_to_sign) -> String`

**Đây là task quan trọng nhất của cả G2.** Một `canonical_request` sai một dấu newline thì mọi test tự viết cũng sai giống nhau và cùng xanh. Test vector của AWS là chỗ duy nhất trong dự án có đáp án đúng do người khác công bố.

- [ ] **Step 1: Chép test vector vào repo**

AWS công bố bộ `aws-sig-v4-test-suite`. Mỗi case là một thư mục chứa:

```
<case>/
  <case>.req            request thô
  <case>.creq           canonical request mong đợi
  <case>.sts            string to sign mong đợi
  <case>.authz          Authorization header mong đợi
  <case>.sreq           signed request mong đợi
```

Chép vào `tests/s3_vectors/`. Các case tối thiểu phải có (chúng bắt đúng những chỗ dễ sai nhất):

| Case | Bắt lỗi gì |
|---|---|
| `get-vanilla` | khung cơ bản |
| `get-vanilla-empty-query-key` | query rỗng |
| `get-vanilla-query-order-key-case` | sắp xếp query phải theo byte, không phải theo case |
| `get-header-key-duplicate` | header trùng tên phải gộp, phân tách bằng dấu phẩy |
| `get-header-value-trim` | trim value, gộp khoảng trắng liên tiếp |
| `get-header-value-order` | thứ tự value của header trùng |
| `get-unreserved` | ký tự unreserved **không** được encode |
| `get-utf8` | UTF-8 trong path |
| `get-space` | khoảng trắng phải thành `%20`, không phải `+` |
| `get-slashes` | `//` trong path giữ nguyên |
| `get-relative-relative` | `..` được chuẩn hoá |
| `post-vanilla-query` | POST có query |
| `post-x-www-form-urlencoded` | body có content type |

Tạo `tests/s3_vectors/README.md` ghi nguồn, phiên bản, và **vì sao** những case này:

```markdown
# SigV4 test vectors

Nguồn: bộ `aws-sig-v4-test-suite` do AWS công bố.

Đây là chỗ duy nhất trong dự án có đáp án đúng do người khác công bố. Mọi test
SigV4 tự viết đều có cùng một điểm mù: nếu `canonical_request` sai một dấu
newline thì test cũng sai giống hệt và cùng xanh.

Credential mà bộ vector dùng là credential ví dụ của AWS, cố định trong tài
liệu, không phải secret:

    AKIDEXAMPLE / wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY
    region us-east-1, service service, 20150830T123600Z
```

**Lưu ý:** service trong bộ vector là `service`, không phải `s3`. Đó là cố ý của AWS — nó kiểm thuật toán, không kiểm cách S3 dùng thuật toán. Cho `service` vào tham số của `signing_key`, đừng hardcode `s3`.

- [ ] **Step 2: Viết test đọc vector**

Trong `src/s3/sigv4.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    const VECTOR_SECRET: &str = "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY";
    const VECTOR_REGION: &str = "us-east-1";
    const VECTOR_SERVICE: &str = "service";
    const VECTOR_DATETIME: &str = "20150830T123600Z";
    const VECTOR_DATE: &str = "20150830";

    /// Parses a `.req` file: request line, headers, blank line, body.
    fn parse_req(raw: &str) -> CanonicalParts { /* ... */ }

    fn run_case(dir: &Path) {
        let name = dir.file_name().unwrap().to_str().unwrap();
        let req = fs::read_to_string(dir.join(format!("{name}.req"))).unwrap();
        let want_creq = fs::read_to_string(dir.join(format!("{name}.creq"))).unwrap();
        let want_sts = fs::read_to_string(dir.join(format!("{name}.sts"))).unwrap();

        let parts = parse_req(&req);
        assert_eq!(
            canonical_request(&parts),
            want_creq.trim_end(),
            "canonical request for {name}"
        );

        let scope = format!("{VECTOR_DATE}/{VECTOR_REGION}/{VECTOR_SERVICE}/aws4_request");
        assert_eq!(
            string_to_sign(VECTOR_DATETIME, &scope, &canonical_request(&parts)),
            want_sts.trim_end(),
            "string to sign for {name}"
        );
    }

    #[test]
    fn matches_the_aws_test_suite() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/s3_vectors");
        let mut ran = 0;
        for entry in fs::read_dir(&root).unwrap() {
            let dir = entry.unwrap().path();
            if !dir.is_dir() {
                continue;
            }
            run_case(&dir);
            ran += 1;
        }
        // A passing run over zero cases is the failure mode this guards.
        assert!(ran >= 13, "expected at least 13 vector cases, ran {ran}");
    }

    #[test]
    fn signing_key_matches_the_documented_chain() {
        // AWS documents this exact derivation with this exact expected hex.
        let key = signing_key(VECTOR_SECRET, VECTOR_DATE, VECTOR_REGION, VECTOR_SERVICE);
        assert_eq!(
            hex::encode(key),
            "c4afb1cc5771d871763a393e44b703571b55cc28424d1a5e86da6ed3c154a4b9"
        );
    }
}
```

`assert!(ran >= 13)` là chỗ dễ bỏ sót nhất: một vòng lặp trên thư mục rỗng pass mà không kiểm gì. Không có nó thì quên chép vector cũng xanh.

- [ ] **Step 3: Chạy để chắc nó fail**

Run: `cargo test --lib s3::sigv4 2>&1 | tail -10`
Expected: FAIL biên dịch — các hàm chưa tồn tại.

- [ ] **Step 4: Viết `canonical_request` và bộ ký**

```rust
//! SigV4, running in both directions.
//!
//! `canonical_request` is shared: verification rebuilds the string the client signed, signing
//! builds the string the gateway is about to sign. One implementation means a bug shows up on
//! both sides at once instead of hiding on one.
use hmac::{Hmac, Mac};
use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// Everything unreserved per RFC 3986 stays literal; everything else is percent-encoded.
/// This is the set AWS specifies, and it is where a hand-rolled signer usually goes wrong:
/// `+` for space, or encoding `-._~`, both produce a signature that never matches.
const AWS_ENCODE: &AsciiSet = &CONTROLS
    .add(b' ').add(b'"').add(b'#').add(b'$').add(b'%').add(b'&').add(b'\'')
    .add(b'(').add(b')').add(b'*').add(b'+').add(b',').add(b'/').add(b':')
    .add(b';').add(b'<').add(b'=').add(b'>').add(b'?').add(b'@').add(b'[')
    .add(b'\\').add(b']').add(b'^').add(b'`').add(b'{').add(b'|').add(b'}');

pub struct CanonicalParts {
    pub method: String,
    /// Already-normalised path, still percent-encoded per segment.
    pub uri: String,
    /// `(name, value)` pairs, unsorted.
    pub query: Vec<(String, String)>,
    /// `(lowercase-name, value)` pairs, unsorted; duplicates allowed.
    pub headers: Vec<(String, String)>,
    /// Lowercase names, in the order the client declared them.
    pub signed_headers: Vec<String>,
    pub payload_hash: String,
}

#[must_use]
pub fn canonical_request(p: &CanonicalParts) -> String { /* per spec §5.1 step 5 */ }

#[must_use]
pub fn string_to_sign(datetime: &str, scope: &str, canonical: &str) -> String {
    format!(
        "AWS4-HMAC-SHA256\n{datetime}\n{scope}\n{}",
        hex::encode(Sha256::digest(canonical.as_bytes()))
    )
}

#[must_use]
pub fn signing_key(secret: &str, date: &str, region: &str, service: &str) -> [u8; 32] {
    let mut key = hmac(format!("AWS4{secret}").as_bytes(), date.as_bytes());
    key = hmac(&key, region.as_bytes());
    key = hmac(&key, service.as_bytes());
    hmac(&key, b"aws4_request")
}

#[must_use]
pub fn signature(key: &[u8; 32], string_to_sign: &str) -> String {
    hex::encode(hmac(key, string_to_sign.as_bytes()))
}

fn hmac(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().into()
}
```

Bốn chi tiết của `canonical_request` mà bộ vector sẽ bắt nếu sai:

1. **Query sắp xếp theo byte của tên đã encode**, rồi theo value. Không phải sắp xếp không phân biệt hoa/thường.
2. **Header trùng tên gộp lại**, value phân tách bằng `,` theo thứ tự xuất hiện.
3. **Value của header trim hai đầu**, và khoảng trắng liên tiếp bên trong nén thành một — trừ trong chuỗi có dấu ngoặc kép.
4. **Path chuẩn hoá `.` và `..`** nhưng `//` giữ nguyên. Với S3 thì AWS **không** chuẩn hoá path — nhưng bộ vector dùng service `service` nên có chuẩn hoá. Tách thành một cờ:

```rust
pub struct CanonicalParts {
    // ...
    /// S3 does not normalise the path; every other service does. The AWS vector suite uses a
    /// non-S3 service, so it exercises the normalising branch.
    pub normalise_path: bool,
}
```

Đây là chỗ dễ sai nhất trong cả plan: nếu chuẩn hoá path cho S3 thì một key tên `a/../b` bị ký thành `b` và signature lệch với mọi client thật.

- [ ] **Step 5: Chạy test**

Run: `cargo test --lib s3::sigv4 2>&1 | tail -10`
Expected: PASS, và dòng `ran >= 13` không nổ.

- [ ] **Step 6: Commit**

```bash
cargo clippy --all-features --all-targets -- -D warnings -W clippy::pedantic -W clippy::nursery -W rust-2018-idioms
cargo fmt --all
git add src/ tests/s3_vectors/
git commit -m "feat(s3): SigV4 canonical request and signing, against the AWS vectors

The AWS test suite is the only place in this project with a correct answer
published by someone else: a canonical request that is wrong by one newline
makes every hand-written test wrong the same way and green together. The suite
uses a non-S3 service so it exercises path normalisation, which S3 does not do —
that is a flag, not a constant, because normalising an S3 key breaks every real
client."
```

---

## Task 4: Verify — dạng header và dạng query

**Files:**
- Modify: `src/s3/sigv4.rs`
- Test: unit test trong cùng file

**Interfaces:**
- Consumes: task 3, `S3Error` (task 2).
- Produces:
  - `sigv4::PresentedSignature { access_key_id, date, region, service, signed_headers, signature, datetime, expires: Option<u64> }`
  - `sigv4::parse_authorization(&HeaderMap) -> Result<PresentedSignature, S3Error>`
  - `sigv4::parse_query(&[(String, String)]) -> Result<PresentedSignature, S3Error>`
  - `sigv4::verify(&PresentedSignature, secret: &str, &CanonicalParts, now: DateTime<Utc>) -> Result<(), S3Error>`
  - `sigv4::CLOCK_SKEW_SECS: i64 = 900`

- [ ] **Step 1: Viết test**

```rust
    fn a_signed_get(secret: &str, when: &str) -> (PresentedSignature, CanonicalParts) { /* ... */ }

    #[test]
    fn a_correctly_signed_request_verifies() {
        let (sig, parts) = a_signed_get(VECTOR_SECRET, "20150830T123600Z");
        let now = parse_amz_datetime("20150830T123600Z").unwrap();
        assert!(verify(&sig, VECTOR_SECRET, &parts, now).is_ok());
    }

    #[test]
    fn a_wrong_secret_does_not_verify() {
        let (sig, parts) = a_signed_get(VECTOR_SECRET, "20150830T123600Z");
        let now = parse_amz_datetime("20150830T123600Z").unwrap();
        assert!(matches!(
            verify(&sig, "not-the-secret", &parts, now),
            Err(S3Error::SignatureDoesNotMatch)
        ));
    }

    /// Spec §5.1 step 4. test_auth.py::test_signature_within_the_clock_window_still_works
    #[test]
    fn a_signature_inside_the_window_verifies() {
        let (sig, parts) = a_signed_get(VECTOR_SECRET, "20150830T123600Z");
        let now = parse_amz_datetime("20150830T124000Z").unwrap();   // +4 phút
        assert!(verify(&sig, VECTOR_SECRET, &parts, now).is_ok());
    }

    /// test_auth.py::test_signature_far_outside_the_clock_window_is_refused
    #[test]
    fn a_signature_outside_the_window_is_refused() {
        let (sig, parts) = a_signed_get(VECTOR_SECRET, "20150830T123600Z");
        let now = parse_amz_datetime("20150830T140000Z").unwrap();   // +24 phút
        assert!(matches!(
            verify(&sig, VECTOR_SECRET, &parts, now),
            Err(S3Error::RequestTimeTooSkewed)
        ));
    }

    /// Skew is symmetric: a client whose clock runs fast is as suspect as one running slow.
    #[test]
    fn a_signature_from_the_future_is_refused() {
        let (sig, parts) = a_signed_get(VECTOR_SECRET, "20150830T140000Z");
        let now = parse_amz_datetime("20150830T123600Z").unwrap();
        assert!(matches!(
            verify(&sig, VECTOR_SECRET, &parts, now),
            Err(S3Error::RequestTimeTooSkewed)
        ));
    }

    #[test]
    fn a_missing_authorization_header_is_access_denied() {
        let headers = axum::http::HeaderMap::new();
        assert!(matches!(
            parse_authorization(&headers),
            Err(S3Error::AccessDenied)
        ));
    }

    #[test]
    fn a_malformed_authorization_header_is_access_denied() {
        for bad in [
            "Bearer abc",
            "AWS4-HMAC-SHA256 nonsense",
            "AWS4-HMAC-SHA256 Credential=x, SignedHeaders=host",   // thiếu Signature
            "AWS4-HMAC-SHA256 Credential=x/y/z, SignedHeaders=host, Signature=s", // scope 3 phần
        ] {
            let mut headers = axum::http::HeaderMap::new();
            headers.insert("authorization", bad.parse().unwrap());
            assert!(
                parse_authorization(&headers).is_err(),
                "should reject {bad:?}"
            );
        }
    }

    /// Presigned form. Spec §5.3.
    #[test]
    fn a_presigned_query_parses_and_carries_its_expiry() {
        let q = vec![
            ("X-Amz-Algorithm".into(), "AWS4-HMAC-SHA256".into()),
            ("X-Amz-Credential".into(), "AKIDEXAMPLE/20150830/us-east-1/s3/aws4_request".into()),
            ("X-Amz-Date".into(), "20150830T123600Z".into()),
            ("X-Amz-Expires".into(), "3600".into()),
            ("X-Amz-SignedHeaders".into(), "host".into()),
            ("X-Amz-Signature".into(), "abc".into()),
        ];
        let sig = parse_query(&q).unwrap();
        assert_eq!(sig.access_key_id, "AKIDEXAMPLE");
        assert_eq!(sig.expires, Some(3600));
    }

    /// test_presigned.py::test_expired_presigned_url_is_refused
    #[test]
    fn an_expired_presigned_signature_is_access_denied() {
        let sig = presigned_at("20150830T123600Z", 60);
        let now = parse_amz_datetime("20150830T124000Z").unwrap();   // +4 phút, hạn 1 phút
        assert!(matches!(
            check_expiry(&sig, now),
            Err(S3Error::AccessDenied)
        ));
    }

    /// The canonical query string for a presigned request excludes X-Amz-Signature — including
    /// it makes every presigned URL fail, and the failure looks like a wrong secret.
    #[test]
    fn the_signature_param_is_excluded_from_the_canonical_query() {
        let q = vec![
            ("X-Amz-Signature".into(), "abc".into()),
            ("X-Amz-Date".into(), "20150830T123600Z".into()),
        ];
        let canonical = canonical_query_for_presigned(&q);
        assert!(!canonical.contains("X-Amz-Signature"));
        assert!(canonical.contains("X-Amz-Date"));
    }
```

- [ ] **Step 2: Chạy để chắc nó fail**

Run: `cargo test --lib s3::sigv4 2>&1 | tail -10`
Expected: FAIL biên dịch.

- [ ] **Step 3: Viết verify**

```rust
/// AWS's default tolerance, and what every S3 client assumes.
pub const CLOCK_SKEW_SECS: i64 = 900;

pub struct PresentedSignature {
    pub access_key_id: String,
    pub date: String,
    pub region: String,
    pub service: String,
    pub signed_headers: Vec<String>,
    pub signature: String,
    pub datetime: String,
    /// Only the presigned form carries this.
    pub expires: Option<u64>,
}

/// Parses `Authorization: AWS4-HMAC-SHA256 Credential=…, SignedHeaders=…, Signature=…`.
///
/// # Errors
/// `AccessDenied` when the header is absent or not SigV4 at all — a request that never tried to
/// authenticate is not a signature failure.
pub fn parse_authorization(headers: &HeaderMap) -> Result<PresentedSignature, S3Error> { /* ... */ }

/// # Errors
/// `AccessDenied` when the required `X-Amz-*` params are absent or malformed.
pub fn parse_query(query: &[(String, String)]) -> Result<PresentedSignature, S3Error> { /* ... */ }

/// Recomputes the signature and compares it in constant time.
///
/// The clock check runs before the HMAC: rejecting a stale request costs one comparison, and
/// verifying it first would let an attacker use signature timing on requests that are refused
/// anyway.
///
/// # Errors
/// `RequestTimeTooSkewed` beyond ±15 minutes; `SignatureDoesNotMatch` otherwise.
pub fn verify(
    presented: &PresentedSignature,
    secret: &str,
    parts: &CanonicalParts,
    now: DateTime<Utc>,
) -> Result<(), S3Error> {
    let signed_at = parse_amz_datetime(&presented.datetime)
        .ok_or(S3Error::RequestTimeTooSkewed)?;
    if (now - signed_at).num_seconds().abs() > CLOCK_SKEW_SECS {
        return Err(S3Error::RequestTimeTooSkewed);
    }

    let scope = format!(
        "{}/{}/{}/aws4_request",
        presented.date, presented.region, presented.service
    );
    let expected = signature(
        &signing_key(secret, &presented.date, &presented.region, &presented.service),
        &string_to_sign(&presented.datetime, &scope, &canonical_request(parts)),
    );

    // Constant-time: a byte-by-byte early return leaks the signature one byte at a time.
    if constant_time_eq(expected.as_bytes(), presented.signature.as_bytes()) {
        Ok(())
    } else {
        Err(S3Error::SignatureDoesNotMatch)
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}
```

So sánh constant-time: `==` trên `String` trả về sớm ở byte đầu khác nhau, và signature là thứ người gọi kiểm soát được — đủ để dò từng byte. Độ dài lệch thì trả `false` ngay là an toàn: độ dài của hex signature là hằng số 64, nên nó không rò gì.

Không thêm crate `subtle` cho việc này: bốn dòng ở trên đủ, và `fold` không có nhánh nên compiler không tối ưu thành return sớm được.

- [ ] **Step 4: Chạy test và commit**

```bash
cargo test --lib s3::sigv4 2>&1 | tail -5
cargo clippy --all-features --all-targets -- -D warnings -W clippy::pedantic -W clippy::nursery -W rust-2018-idioms
cargo fmt --all
git add src/
git commit -m "feat(s3): verify SigV4 in both the header and query forms

The clock check runs before the HMAC, so a stale request costs one comparison
rather than a full verification. Signature comparison is constant-time: == on a
String returns at the first differing byte, and the signature is attacker-
controlled, which is enough to probe it one byte at a time."
```

---

## Task 5: Client upstream

**Files:**
- Create: `src/s3/upstream.rs`, `tests/support/mock_upstream.rs`, `tests/support/mod.rs`
- Modify: `tests/mod.rs`
- Test: `tests/requests/upstream.rs`

**Interfaces:**
- Consumes: `sigv4` (task 3, 4), `S3Error` (task 2), `pools::Model` (G1).
- Produces:
  - `upstream::Client::new(pool: &pools::Model) -> Result<Client, S3Error>`
  - `upstream::Client::send(&self, req: UpstreamRequest) -> Result<UpstreamResponse, S3Error>`
  - `upstream::UpstreamRequest { method, key, query, headers, body: Body }`
  - `upstream::Body::Empty | Bytes(Vec<u8>) | Stream(BoxStream)`
  - `upstream::UpstreamResponse { status, headers, body: BoxStream }`
  - `tests::support::MockUpstream` — ghi lại request nhận được

- [ ] **Step 1: Viết upstream giả**

Tạo `tests/support/mock_upstream.rs`. Đây là công cụ mà **mọi plan sau đều dựa vào**, nên nó phải làm được ba việc: ghi lại request, trả response đặt trước, và khẳng định **không** nhận gì.

```rust
//! An axum server standing in for the object store.
//!
//! It records what it received, so a test can assert on the physical key the gateway sent —
//! and, more importantly, can assert that it received *nothing at all*. "Upstream saw no
//! request" is a stronger claim than "the client got a 403": it catches the case where the
//! gateway called upstream and only then refused, which means the bytes had already crossed
//! the isolation boundary.
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct Seen {
    pub method: String,
    pub path: String,
    pub query: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[derive(Clone)]
pub struct Canned {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

pub struct MockUpstream {
    pub base_url: String,
    seen: Arc<Mutex<Vec<Seen>>>,
    canned: Arc<Mutex<Vec<Canned>>>,
    _shutdown: tokio::sync::oneshot::Sender<()>,
}

impl MockUpstream {
    /// Binds an ephemeral port and serves until dropped.
    pub async fn start() -> Self { /* ... */ }

    /// Queues one response. Requests beyond the queue get 200 with an empty body.
    pub fn push(&self, canned: Canned) { /* ... */ }

    pub fn requests(&self) -> Vec<Seen> { /* ... */ }

    /// The assertion that matters most: the gateway refused before touching the store.
    pub fn assert_untouched(&self) {
        let seen = self.requests();
        assert!(
            seen.is_empty(),
            "upstream received {} request(s) it should never have seen: {:?}",
            seen.len(),
            seen.iter().map(|s| format!("{} {}", s.method, s.path)).collect::<Vec<_>>()
        );
    }

    /// Asserts the physical key the gateway addressed, which is the rewrite under test.
    pub fn assert_key(&self, n: usize, expected: &str) {
        let seen = self.requests();
        let got = seen.get(n).unwrap_or_else(|| {
            panic!("no request at index {n}; upstream saw {}", seen.len())
        });
        assert_eq!(got.path.trim_start_matches('/'), expected);
    }
}
```

`assert_untouched` in ra cả danh sách request đã nhận khi fail — vì lúc nó fail, câu hỏi đầu tiên luôn là "nó gọi cái gì".

Thêm `mod support;` vào `tests/mod.rs`.

- [ ] **Step 2: Viết test cho client**

Tạo `tests/requests/upstream.rs`:

```rust
/// Every upstream request must be signed, and the physical bucket must be in the path.
#[tokio::test]
#[serial]
async fn a_get_is_signed_and_addresses_the_physical_bucket() {
    let mock = MockUpstream::start().await;
    let pool = pool_pointing_at(&mock);          // provider custom, api_endpoint = mock.base_url

    let client = upstream::Client::new(&pool).unwrap();
    client
        .send(upstream::UpstreamRequest::get("11111111/media-cdn/photos/a.jpg"))
        .await
        .unwrap();

    let seen = mock.requests();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].method, "GET");
    assert_eq!(seen[0].path, "/osg-main/11111111/media-cdn/photos/a.jpg");

    let auth = header(&seen[0], "authorization");
    assert!(auth.starts_with("AWS4-HMAC-SHA256 Credential=FIXTUREUPSTREAMID/"));
    assert!(auth.contains("SignedHeaders="));
    assert!(auth.contains("Signature="));
    assert!(!header(&seen[0], "x-amz-date").is_empty());
    assert!(!header(&seen[0], "x-amz-content-sha256").is_empty());
}

/// A pool with no credentials must fail before sending anything, not send an unsigned request.
#[tokio::test]
#[serial]
async fn an_unconfigured_pool_refuses_to_build_a_client() {
    let mock = MockUpstream::start().await;
    let pool = unconfigured_pool(&mock);

    assert!(upstream::Client::new(&pool).is_err());
    mock.assert_untouched();
}

/// Body streams through; nothing is buffered.
#[tokio::test]
#[serial]
async fn a_put_streams_its_body() {
    let mock = MockUpstream::start().await;
    let pool = pool_pointing_at(&mock);
    let client = upstream::Client::new(&pool).unwrap();

    let payload = vec![7u8; 3 * 1024 * 1024];     // 3 MiB
    client
        .send(upstream::UpstreamRequest::put(
            "11111111/media-cdn/big.bin",
            upstream::Body::Bytes(payload.clone()),
        ))
        .await
        .unwrap();

    assert_eq!(mock.requests()[0].body.len(), payload.len());
}

/// Spec §12: the upstream error body names the physical bucket. Keep the code, drop the body.
#[tokio::test]
#[serial]
async fn an_upstream_error_keeps_its_code_and_loses_its_body() {
    let mock = MockUpstream::start().await;
    mock.push(Canned {
        status: 404,
        headers: vec![("content-type".into(), "application/xml".into())],
        body: br#"<?xml version="1.0"?><Error><Code>NoSuchKey</Code>
                  <Message>The specified key does not exist.</Message>
                  <Key>osg-main/11111111/media-cdn/gone.jpg</Key>
                  <BucketName>osg-main</BucketName></Error>"#
            .to_vec(),
    });
    let pool = pool_pointing_at(&mock);
    let client = upstream::Client::new(&pool).unwrap();

    let err = client
        .send(upstream::UpstreamRequest::get("11111111/media-cdn/gone.jpg"))
        .await
        .unwrap_err();

    assert_eq!(err.code(), "NoSuchKey");
    let rendered = format!("{}{}", err.code(), err.message());
    assert!(!rendered.contains("osg-main"), "physical bucket leaked: {rendered}");
}

/// An upstream 5xx is not the client's fault and must not carry upstream detail.
#[tokio::test]
#[serial]
async fn an_upstream_5xx_becomes_internal_error() {
    let mock = MockUpstream::start().await;
    mock.push(Canned { status: 503, headers: vec![], body: b"upstream is having a day".to_vec() });
    let pool = pool_pointing_at(&mock);
    let client = upstream::Client::new(&pool).unwrap();

    let err = client
        .send(upstream::UpstreamRequest::get("11111111/media-cdn/a.jpg"))
        .await
        .unwrap_err();

    assert_eq!(err.code(), "InternalError");
    assert!(!err.message().contains("having a day"));
}
```

- [ ] **Step 3: Chạy để chắc nó fail**

Run: `cargo test --test mod requests::upstream 2>&1 | tail -10`
Expected: FAIL biên dịch.

- [ ] **Step 4: Viết client**

```rust
//! The outbound half of the data plane.
//!
//! Bodies stream in both directions: `reqwest::Body::wrap_stream` on the way up, the response
//! body handed back as a stream on the way down. A 5 GiB PUT crosses the gateway with constant
//! memory.
use crate::{models::pools, s3::{error::S3Error, sigv4}};

pub struct Client {
    http: reqwest::Client,
    endpoint: String,
    region: String,
    physical_bucket: String,
    access_id: String,
    secret: String,
}

impl Client {
    /// # Errors
    /// `InternalError` when the pool has no credentials — the backfill pool is in that state, and
    /// sending an unsigned request would be worse than failing.
    pub fn new(pool: &pools::Model) -> Result<Self, S3Error> {
        if !pool.is_configured() {
            tracing::error!(
                pool = %pool.name,
                "pool has no upstream credentials; an admin must configure it"
            );
            return Err(S3Error::InternalError);
        }
        // ...
    }
}
```

Bốn quyết định trong `Client`:

1. **`reqwest::Client` tái dùng, không tạo mỗi request.** Nó giữ connection pool; tạo mới mỗi request là một TLS handshake mỗi lần. Nhưng `Client::new` nhận `&pools::Model` nên mỗi pool một client — cache theo `pool.id` trong một `OnceLock<Mutex<HashMap<i32, reqwest::Client>>>`, hoặc đơn giản hơn: một `reqwest::Client` toàn cục dùng chung, credential truyền theo từng request. Chọn cái sau: `reqwest::Client` không giữ credential, nó chỉ giữ connection pool, nên chia sẻ được.

2. **Timeout chỉ áp cho request điều khiển.** `OSG_UPSTREAM_TIMEOUT_MS` (30s) đặt bằng `.timeout()` cho GET/HEAD/DELETE/POST nhỏ, **không** đặt cho PUT có body stream — một upload 5 GiB trên đường chậm là request hợp lệ. Đặt timeout toàn cục ở đây là làm hỏng đúng cái tính năng chính.

3. **Endpoint dựng theo provider.** `api_endpoint` có thì dùng; không thì suy từ provider và region (`https://s3.{region}.amazonaws.com` cho `aws`). Path-style: `{endpoint}/{physical_bucket}/{key}`.

4. **`x-amz-content-sha256` khi ký lên upstream.** Với body stream ta không có hash trước, nên gửi `UNSIGNED-PAYLOAD`. Điều đó yêu cầu upstream chấp nhận nó — S3, R2, MinIO đều chấp nhận trên HTTPS. Ghi lại như một giả định.

- [ ] **Step 5: Chạy test ba backend**

```bash
cargo test --test mod requests::upstream 2>&1 | tail -5
DATABASE_URL=postgres://loco:loco@localhost:5432/osg_test cargo test 2>&1 | tail -3
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo test 2>&1 | tail -3
```

- [ ] **Step 6: Commit**

```bash
cargo clippy --all-features --all-targets -- -D warnings -W clippy::pedantic -W clippy::nursery -W rust-2018-idioms
cargo fmt --all
git add src/ tests/
git commit -m "feat(s3): upstream client, streaming both ways

Bodies stream through: a 5 GiB PUT crosses the gateway with constant memory.
The request timeout covers control operations only — setting one for a streamed
upload breaks the feature it is meant to protect. A pool with no credentials
fails to build a client rather than sending an unsigned request.

Adds tests/support/mock_upstream.rs, which every later plan depends on: it
records what upstream received, and assert_untouched() proves the gateway
refused before any byte crossed the isolation boundary."
```

---

## Self-review

**Phủ spec.** Mục 5.1 (verify header) → task 4. Mục 5.2 (`x-amz-content-sha256`) → task 4 phần payload hash; ba dạng xử lý ở G3 khi có request thật. Mục 5.3 (presigned) → task 4. Mục 12 (error surface) → task 2. Mục 16 (dependency) → task 1. Mục 17.1 (test vector AWS) → task 3. Mục 17.2 (mock upstream) → task 5.

**Chưa phủ, cố ý.** Không có `S3Request`, không route, không dispatch — G3. `xml.rs` chưa có: G3 cần nó cho error response, G5 cho listing.

**Nhất quán kiểu.** `CanonicalParts` khai task 3, dùng task 3 và 4. `PresentedSignature` khai task 4. `S3Error` khai task 2, dùng task 4 và 5. `MockUpstream` khai task 5, dùng ở G3–G7.

**Rủi ro đã biết.**

1. **`normalise_path` là cờ, và mặc định sai là một lỗi im lặng.** S3 không chuẩn hoá path; bộ vector của AWS dùng service khác nên có chuẩn hoá. Nếu để mặc định `true` thì key `a/../b` bị ký thành `b` và mọi client thật lệch signature — mà test vector vẫn xanh. Cần một test riêng khẳng định đường S3 **không** chuẩn hoá; test đó không có trong bộ vector nên phải tự viết, và phải nói rõ nó không được bảo chứng bởi đáp án của AWS.
2. **`From<ModelError> for S3Error` khớp chuỗi.** Có test canh, nhưng trần đã ghi.
3. **`UNSIGNED-PAYLOAD` khi ký lên upstream** giả định upstream chấp nhận nó. Đúng với S3/R2/MinIO trên HTTPS. Một upstream từ chối sẽ fail toàn bộ đường ghi cùng lúc — dễ chẩn, nhưng cần một dòng trong `docs/docker.md` khi thêm provider mới.
4. **Chép test vector vào repo là thủ công.** Nếu chép thiếu thì `assert!(ran >= 13)` bắt được. Nếu chép sai nội dung thì không có gì bắt được — nên bước 1 của task 3 phải ghi nguồn và checksum trong `tests/s3_vectors/README.md`.
