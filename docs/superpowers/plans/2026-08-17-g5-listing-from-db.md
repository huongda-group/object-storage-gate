# G5 — Listing đọc từ DB — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `ListObjectsV2`, `ListBuckets`, `HeadBucket` trả XML đúng hình dạng S3, đọc hoàn toàn từ DB, không gọi upstream lần nào.

**Architecture:** SQL làm prefix và phân trang bằng so sánh khoảng (`prefix_upper_bound` từ P3); delimiter roll-up làm trong Rust vì SQL không diễn đạt được nó gọn. Prefix scoping của access key là một điều kiện `WHERE` thêm vào, không phải một bước lọc sau — nên không có đường nào key scoped thấy được key ngoài phạm vi rồi mới bị cắt.

**Tech Stack:** Rust, SeaORM 1.1, `quick-xml`, `base64`.

**Spec:** `docs/superpowers/specs/2026-08-17-s3-gateway-design.md` mục 9

**Phụ thuộc:** G3 xong.

**Deliverable:** `aws s3 ls s3://bucket/ --recursive` và `rclone lsd` chạy được; `test_bucket.py` (7 test) có bản Rust tương đương.

## Global Constraints

- Ba backend hạng nhất: Postgres, MySQL >= 8.0.13, SQLite. Query listing là chỗ dễ lệch nhất giữa ba backend — **mọi test của plan này phải chạy cả ba trước khi commit.**
- Cấm `ILIKE`, `RETURNING`, `ON CONFLICT`, `jsonb`, cột array, `pg_advisory_lock`, `FOR UPDATE SKIP LOCKED`.
- **Không dùng `LIKE` cho prefix.** P3 đã đổi sang so sánh khoảng vì `starts_with` của sea-orm không escape `%`/`_`, và `LIKE` của SQLite không phân biệt hoa/thường với ASCII. Quay lại `LIKE` là mở lại đúng lỗ đó.
- Comment trong code: tiếng Anh, một câu một dòng.
- Không tự commit/push ngoài các bước commit trong plan. Không AI attribution.

---

## File Structure

**Tạo mới:**
- `src/controllers/s3/listing.rs`
- `tests/requests/s3/listing.rs`

**Sửa:**
- `src/models/objects.rs` — `list_page` thay cho `list_by_prefix` ở đường S3
- `src/s3/xml.rs` — `list_objects_v2`, `list_buckets`
- `src/controllers/s3/mod.rs`

---

## Task 1: `objects::Model::list_page`

**Files:**
- Modify: `src/models/objects.rs`
- Test: `tests/models/objects.rs`

**Interfaces:**
- Consumes: `prefix_upper_bound` (P3).
- Produces:
  - `objects::ListQuery { bucket_id, prefix, after: Option<String>, limit: u64 }`
  - `objects::Model::list_page(db, &ListQuery) -> ModelResult<Vec<Model>>` — trả tối đa `limit + 1` dòng

`list_by_prefix` giữ nguyên cho caller cũ; `list_page` là bản có `after` và trả thêm một dòng để biết `IsTruncated`.

- [ ] **Step 1: Viết test**

Thêm vào `tests/models/objects.rs`:

```rust
async fn seed_keys(db: &DatabaseConnection, bucket_id: i32, keys: &[&str]) {
    for k in keys {
        objects::Model::put_object(db, bucket_id, k, 1, "e", "text/plain")
            .await
            .unwrap();
    }
}

/// The extra row is how IsTruncated is decided without a second COUNT query.
#[tokio::test]
#[serial]
async fn list_page_returns_one_more_than_the_limit_when_there_is_more() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();
    let bucket_id = a_bucket(db, "paging").await;
    seed_keys(db, bucket_id, &["a", "b", "c", "d", "e"]).await;

    let page = objects::Model::list_page(
        db,
        &objects::ListQuery { bucket_id, prefix: String::new(), after: None, limit: 3 },
    )
    .await
    .unwrap();

    assert_eq!(page.len(), 4, "3 rows plus the lookahead");
    assert_eq!(page[0].object_key, "a");
}

#[tokio::test]
#[serial]
async fn list_page_stops_at_the_end_without_a_lookahead_row() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();
    let bucket_id = a_bucket(db, "short").await;
    seed_keys(db, bucket_id, &["a", "b"]).await;

    let page = objects::Model::list_page(
        db,
        &objects::ListQuery { bucket_id, prefix: String::new(), after: None, limit: 10 },
    )
    .await
    .unwrap();

    assert_eq!(page.len(), 2);
}

/// `after` is exclusive: the marker itself must not come back, or every page repeats one key.
#[tokio::test]
#[serial]
async fn after_is_exclusive() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();
    let bucket_id = a_bucket(db, "after").await;
    seed_keys(db, bucket_id, &["a", "b", "c"]).await;

    let page = objects::Model::list_page(
        db,
        &objects::ListQuery {
            bucket_id,
            prefix: String::new(),
            after: Some("b".to_string()),
            limit: 10,
        },
    )
    .await
    .unwrap();

    assert_eq!(
        page.iter().map(|o| o.object_key.as_str()).collect::<Vec<_>>(),
        vec!["c"]
    );
}

/// Ordering must be byte order, not collation order — S3 lists keys byte-ascending, and on MySQL
/// a case-insensitive collation would interleave `B` between `a` and `b`. The binary collation
/// from m20260817_000004 is what makes this hold.
#[tokio::test]
#[serial]
async fn ordering_is_byte_ascending_across_case() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();
    let bucket_id = a_bucket(db, "ordering").await;
    seed_keys(db, bucket_id, &["b", "A", "a", "B"]).await;

    let page = objects::Model::list_page(
        db,
        &objects::ListQuery { bucket_id, prefix: String::new(), after: None, limit: 10 },
    )
    .await
    .unwrap();

    // 'A'(0x41) 'B'(0x42) 'a'(0x61) 'b'(0x62)
    assert_eq!(
        page.iter().map(|o| o.object_key.as_str()).collect::<Vec<_>>(),
        vec!["A", "B", "a", "b"]
    );
}

/// A wildcard in the prefix is literal. Same property P3 fixed, asserted again on the paging path
/// because it is a separate query.
#[tokio::test]
#[serial]
async fn list_page_treats_wildcards_literally() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();
    let bucket_id = a_bucket(db, "wild").await;
    seed_keys(db, bucket_id, &["a_/one", "ab/two", "a%/three"]).await;

    let page = objects::Model::list_page(
        db,
        &objects::ListQuery {
            bucket_id,
            prefix: "a_/".to_string(),
            after: None,
            limit: 10,
        },
    )
    .await
    .unwrap();

    assert_eq!(page.len(), 1);
    assert_eq!(page[0].object_key, "a_/one");
}

/// `after` interacts with `prefix`: paging inside a prefix must not walk out of it.
#[tokio::test]
#[serial]
async fn after_stays_inside_the_prefix() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    seed::<App>(&boot.app_context).await.unwrap();
    let bucket_id = a_bucket(db, "bounded").await;
    seed_keys(db, bucket_id, &["img/a", "img/b", "zz/c"]).await;

    let page = objects::Model::list_page(
        db,
        &objects::ListQuery {
            bucket_id,
            prefix: "img/".to_string(),
            after: Some("img/a".to_string()),
            limit: 10,
        },
    )
    .await
    .unwrap();

    assert_eq!(
        page.iter().map(|o| o.object_key.as_str()).collect::<Vec<_>>(),
        vec!["img/b"],
        "paging must not walk past the prefix into zz/"
    );
}
```

Test `ordering_is_byte_ascending_across_case` là chỗ MySQL sẽ fail nếu collation binary của `m20260817_000004` không áp — và nó fail theo cách khó thấy: danh sách vẫn có đủ key, chỉ sai thứ tự, nên client phân trang sẽ nhảy hoặc lặp.

- [ ] **Step 2: Chạy để chắc nó fail — trên cả ba backend**

```bash
cargo test --test mod models::objects 2>&1 | tail -10
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo test --test mod models::objects 2>&1 | tail -10
```

Expected: FAIL biên dịch (`list_page` chưa có).

- [ ] **Step 3: Viết**

```rust
/// One page of a bucket listing.
pub struct ListQuery {
    pub bucket_id: i32,
    /// Empty means the whole bucket.
    pub prefix: String,
    /// Exclusive lower bound: the continuation token or `start-after`.
    pub after: Option<String>,
    pub limit: u64,
}

impl Model {
    /// One page of keys, byte-ascending, plus one lookahead row.
    ///
    /// The lookahead is how `IsTruncated` is decided without a second `COUNT` — a count over a
    /// large prefix costs the same as the page itself.
    ///
    /// Uses range comparison rather than `LIKE`: sea-orm's `starts_with` does not escape `%` or
    /// `_`, SQLite's `LIKE` is case-insensitive for ASCII where Postgres's is not, and a range can
    /// use the `(bucket_id, object_key)` index.
    ///
    /// # Errors
    /// Returns an error on DB failure.
    pub async fn list_page(
        db: &DatabaseConnection,
        q: &ListQuery,
    ) -> ModelResult<Vec<Self>> {
        let mut query = Entity::find()
            .filter(Column::BucketId.eq(q.bucket_id))
            .filter(Column::IsLatest.eq(true))
            .order_by_asc(Column::ObjectKey)
            .limit(q.limit + 1);

        if !q.prefix.is_empty() {
            query = query.filter(Column::ObjectKey.gte(q.prefix.as_str()));
            if let Some(upper) = prefix_upper_bound(&q.prefix) {
                query = query.filter(Column::ObjectKey.lt(upper));
            }
        }
        if let Some(after) = &q.after {
            query = query.filter(Column::ObjectKey.gt(after.as_str()));
        }

        Ok(query.all(db).await?)
    }
}
```

`is_latest` xuất hiện ở đây: cột đó do migration versioning của G1 thêm, mặc định `true`. Đưa nó vào query ngay từ đầu nghĩa là bật versioning sau này không phải sửa query — đó là lý do để chỗ trống.

- [ ] **Step 4: Chạy test ba backend và commit**

```bash
cargo test --test mod models::objects 2>&1 | tail -5
DATABASE_URL=postgres://loco:loco@localhost:5432/osg_test cargo test --test mod models::objects 2>&1 | tail -5
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo test --test mod models::objects 2>&1 | tail -5
cargo clippy --all-features --all-targets -- -D warnings -W clippy::pedantic -W clippy::nursery -W rust-2018-idioms
cargo fmt --all
git add src/ tests/
git commit -m "feat(objects): list_page, one page plus a lookahead row

The lookahead decides IsTruncated without a second COUNT, which over a large
prefix costs as much as the page. Range comparison, not LIKE: starts_with does
not escape % or _, SQLite's LIKE is case-insensitive for ASCII where Postgres's
is not, and a range uses the composite index.

A test asserts byte-ascending order across case, which is what the binary
collation from m20260817_000004 buys — without it MySQL interleaves B between a
and b, and a paging client silently skips or repeats keys."
```

---

## Task 2: Delimiter roll-up và token

**Files:**
- Create: `src/controllers/s3/listing.rs`
- Test: unit test trong cùng file

**Interfaces:**
- Consumes: `list_page` (task 1).
- Produces:
  - `listing::Page { contents: Vec<Entry>, common_prefixes: Vec<String>, is_truncated: bool, next_token: Option<String> }`
  - `listing::roll_up(rows, prefix, delimiter, limit) -> Page`
  - `listing::encode_token(&str) -> String`, `listing::decode_token(&str) -> Result<String, S3Error>`

Tách thành hàm thuần để test được không cần DB — đây là chỗ logic dày nhất của plan.

- [ ] **Step 1: Viết test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn rows(keys: &[&str]) -> Vec<Row> {
        keys.iter()
            .map(|k| Row { key: (*k).to_string(), size: 1, etag: "e".into(), modified: ts() })
            .collect()
    }

    #[test]
    fn without_a_delimiter_every_key_is_content() {
        let page = roll_up(rows(&["a/1", "a/2", "b"]), "", None, 10);
        assert_eq!(page.contents.len(), 3);
        assert!(page.common_prefixes.is_empty());
        assert!(!page.is_truncated);
    }

    /// test_bucket.py::test_list_objects_v2_delimiter_rolls_up_common_prefixes
    #[test]
    fn a_delimiter_rolls_up_and_dedups() {
        let page = roll_up(rows(&["a/1", "a/2", "b/1", "top"]), "", Some('/'), 10);

        assert_eq!(page.common_prefixes, vec!["a/", "b/"]);
        assert_eq!(
            page.contents.iter().map(|c| c.key.as_str()).collect::<Vec<_>>(),
            vec!["top"],
            "keys rolled into a common prefix must not also appear in Contents"
        );
    }

    /// The delimiter is searched after the prefix, not from the start of the key — otherwise
    /// listing prefix `img/` with delimiter `/` rolls everything into one entry `img/`.
    #[test]
    fn the_delimiter_is_searched_after_the_prefix() {
        let page = roll_up(
            rows(&["img/2026/a.png", "img/2026/b.png", "img/2025/c.png", "img/top.png"]),
            "img/",
            Some('/'),
            10,
        );

        assert_eq!(page.common_prefixes, vec!["img/2025/", "img/2026/"]);
        assert_eq!(
            page.contents.iter().map(|c| c.key.as_str()).collect::<Vec<_>>(),
            vec!["img/top.png"]
        );
    }

    /// A common prefix counts against max-keys, and truncation stops there.
    #[test]
    fn common_prefixes_count_towards_the_limit() {
        let page = roll_up(rows(&["a/1", "b/1", "c/1", "d/1"]), "", Some('/'), 2);

        assert_eq!(page.common_prefixes.len(), 2);
        assert!(page.is_truncated);
        assert!(page.next_token.is_some());
    }

    /// The token must resume where the page stopped, including mid-roll-up.
    #[test]
    fn the_token_resumes_after_the_last_emitted_thing() {
        let all = rows(&["a/1", "a/2", "b/1", "c/1"]);
        let first = roll_up(all.clone(), "", Some('/'), 2);

        assert_eq!(first.common_prefixes, vec!["a/", "b/"]);
        assert!(first.is_truncated);

        // The token is the last key consumed, so the next page starts after b/1 — not after a/2,
        // which would repeat b/.
        let token = decode_token(first.next_token.as_ref().unwrap()).unwrap();
        assert_eq!(token, "b/1");
    }

    #[test]
    fn a_token_round_trips_and_a_tampered_one_is_rejected() {
        let t = encode_token("img/a.png");
        assert_ne!(t, "img/a.png", "the token must look opaque");
        assert_eq!(decode_token(&t).unwrap(), "img/a.png");
        assert!(decode_token("!!!not base64!!!").is_err());
    }

    /// test_bucket.py::test_list_objects_v2_empty_prefix_has_no_contents_key
    #[test]
    fn an_empty_result_has_no_contents_at_all() {
        let page = roll_up(vec![], "", None, 10);
        assert!(page.contents.is_empty());
        assert!(page.common_prefixes.is_empty());
        assert!(!page.is_truncated);
        assert!(page.next_token.is_none());
    }

    /// A key exactly equal to the prefix is content, not a common prefix — S3 emits it in
    /// Contents, and dropping it loses an object from every listing.
    #[test]
    fn a_key_equal_to_the_prefix_stays_in_contents() {
        let page = roll_up(rows(&["img/", "img/a.png"]), "img/", Some('/'), 10);
        assert!(page.contents.iter().any(|c| c.key == "img/"));
    }
}
```

`the_token_resumes_after_the_last_emitted_thing` là test khó nhất và đáng nhất: token phải là key **cuối cùng đã tiêu thụ**, không phải key cuối trong `Contents`. Nếu lấy sai thì trang sau lặp lại một `CommonPrefixes` và client thấy thư mục xuất hiện hai lần.

`a_key_equal_to_the_prefix_stays_in_contents` bắt một lỗi thật hay xảy ra: `"img/"` với prefix `"img/"` thì phần còn lại là chuỗi rỗng, không có delimiter — dễ bị code roll-up bỏ qua hoàn toàn.

- [ ] **Step 2: Chạy để chắc nó fail**

Run: `cargo test --lib controllers::s3::listing 2>&1 | tail -10`

- [ ] **Step 3: Viết `roll_up`**

```rust
/// Groups a page of keys into `Contents` and `CommonPrefixes`.
///
/// The delimiter is searched in the part of the key *after* the prefix. Searching from the start
/// would roll every key under `img/` into the single entry `img/` when listing that prefix.
///
/// A common prefix counts against `max_keys` the same as a key does, because S3's `KeyCount` is
/// the sum of both and a client sizing its buffer from it would under-allocate otherwise.
///
/// `next_token` is the last key *consumed*, not the last key emitted in `Contents`: a page that
/// ended part-way through a roll-up must resume after the whole group, or the next page repeats
/// the common prefix and the client sees the folder twice.
pub fn roll_up(rows: Vec<Row>, prefix: &str, delimiter: Option<char>, limit: u64) -> Page { /* ... */ }

/// Base64 of the resume key.
///
/// S3's token is opaque and clients must not parse it; encoding keeps anyone from depending on its
/// shape, and it survives a key containing characters that would break a bare query parameter.
#[must_use]
pub fn encode_token(key: &str) -> String { /* URL_SAFE_NO_PAD */ }

/// # Errors
/// `InvalidArgument` when the token is not the shape this gateway issued — a client that
/// hand-crafts one gets a clear refusal rather than a silently wrong page.
pub fn decode_token(token: &str) -> Result<String, S3Error> { /* ... */ }
```

- [ ] **Step 4: Chạy test và commit**

```bash
cargo test --lib controllers::s3::listing 2>&1 | tail -5
cargo clippy --all-features --all-targets -- -D warnings -W clippy::pedantic -W clippy::nursery -W rust-2018-idioms
cargo fmt --all
git add src/
git commit -m "feat(s3): delimiter roll-up and opaque continuation tokens

The delimiter is searched after the prefix, not from the start of the key —
searching from the start rolls everything under img/ into the single entry img/
when that is the prefix being listed. The token is the last key consumed, not the
last key in Contents: a page that stopped mid-roll-up must resume after the whole
group, or the next page repeats the common prefix and the client sees the folder
twice."
```

---

## Task 3: ListObjectsV2

**Files:**
- Modify: `src/controllers/s3/listing.rs`, `src/s3/xml.rs`, `src/controllers/s3/mod.rs`
- Test: `tests/requests/s3/listing.rs`

**Interfaces:**
- Consumes: `list_page`, `roll_up`, `S3Request`.
- Produces: `listing::list_objects_v2`; `xml::list_objects_v2(...) -> String`.

- [ ] **Step 1: Viết test**

Tạo `tests/requests/s3/listing.rs`:

```rust
/// test_bucket.py::test_list_objects_v2_by_prefix — and never a call upstream.
#[tokio::test]
#[serial]
async fn list_reads_from_the_database_only() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.seed_objects("media-cdn", &["img/a.png", "img/b.png", "docs/c.pdf"]).await;

        let res = g.get(&signer, "/media-cdn?list-type=2&prefix=img/").await;

        assert_eq!(res.status_code(), 200);
        let body = res.text();
        assert!(body.contains("<Key>img/a.png</Key>"));
        assert!(body.contains("<Key>img/b.png</Key>"));
        assert!(!body.contains("docs/c.pdf"));
        g.mock.assert_untouched();
    })
    .await;
}

/// test_wire.py::test_list_objects_v2_xml_is_s3_shaped
#[tokio::test]
#[serial]
async fn the_xml_is_s3_shaped() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.seed_objects("media-cdn", &["a.png"]).await;

        let body = g.get(&signer, "/media-cdn?list-type=2").await.text();

        assert!(body.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
        assert!(body.contains("<ListBucketResult"));
        assert!(body.contains("<Name>media-cdn</Name>"));
        assert!(body.contains("<KeyCount>1</KeyCount>"));
        assert!(body.contains("<MaxKeys>1000</MaxKeys>"));
        assert!(body.contains("<IsTruncated>false</IsTruncated>"));
        assert!(body.contains("<Size>"));
        assert!(body.contains("<ETag>"));
        assert!(body.contains("<LastModified>"));
        assert!(body.contains("<StorageClass>STANDARD</StorageClass>"));

        // The bucket name is the logical one; the physical bucket must not appear anywhere.
        assert!(!body.contains("osg-main"));
    })
    .await;
}

/// test_bucket.py::test_list_objects_v2_paginates_with_continuation_token
#[tokio::test]
#[serial]
async fn pagination_walks_every_key_exactly_once() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        let keys: Vec<String> = (0..25).map(|i| format!("k{i:02}")).collect();
        g.seed_objects("media-cdn", &keys.iter().map(String::as_str).collect::<Vec<_>>()).await;

        let mut seen = Vec::new();
        let mut token: Option<String> = None;
        for _ in 0..10 {
            let q = match &token {
                Some(t) => format!("/media-cdn?list-type=2&max-keys=10&continuation-token={t}"),
                None => "/media-cdn?list-type=2&max-keys=10".to_string(),
            };
            let body = g.get(&signer, &q).await.text();
            seen.extend(keys_in(&body));
            token = next_token_in(&body);
            if token.is_none() {
                break;
            }
        }

        assert_eq!(seen.len(), 25, "every key exactly once, got {}", seen.len());
        let mut sorted = seen.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), 25, "a key was repeated across pages");
    })
    .await;
}

/// test_bucket.py::test_list_objects_v2_start_after_excludes_the_marker
#[tokio::test]
#[serial]
async fn start_after_excludes_the_marker() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.seed_objects("media-cdn", &["a", "b", "c"]).await;

        let body = g.get(&signer, "/media-cdn?list-type=2&start-after=b").await.text();

        assert!(!body.contains("<Key>a</Key>"));
        assert!(!body.contains("<Key>b</Key>"));
        assert!(body.contains("<Key>c</Key>"));
    })
    .await;
}

/// test_bucket.py::test_list_objects_v2_empty_prefix_has_no_contents_key
#[tokio::test]
#[serial]
async fn an_empty_listing_omits_contents_entirely() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;

        let body = g.get(&signer, "/media-cdn?list-type=2&prefix=nothing/").await.text();

        assert!(!body.contains("<Contents>"), "S3 omits the tag rather than emitting an empty one");
        assert!(body.contains("<KeyCount>0</KeyCount>"));
    })
    .await;
}

/// test_scoping.py::test_scoped_key_cannot_list_the_whole_bucket
#[tokio::test]
#[serial]
async fn a_scoped_key_cannot_list_the_bucket_root() {
    with_gateway(|g| async move {
        let signer = g.scoped_key("img/").await;
        g.seed_objects("media-cdn", &["img/a.png", "docs/secret.pdf"]).await;

        let res = g.get(&signer, "/media-cdn?list-type=2").await;

        assert_eq!(res.status_code(), 403);
        assert!(res.text().contains("AccessDenied"));
        assert!(!res.text().contains("secret.pdf"));
    })
    .await;
}

/// test_scoping.py::test_scoped_key_cannot_list_another_folder
#[tokio::test]
#[serial]
async fn a_scoped_key_cannot_list_another_folder() {
    with_gateway(|g| async move {
        let signer = g.scoped_key("img/").await;
        g.seed_objects("media-cdn", &["docs/secret.pdf"]).await;

        let res = g.get(&signer, "/media-cdn?list-type=2&prefix=docs/").await;

        assert_eq!(res.status_code(), 403);
        assert!(!res.text().contains("secret.pdf"));
    })
    .await;
}

/// test_scoping.py::test_scoped_key_can_list_its_own_folder
#[tokio::test]
#[serial]
async fn a_scoped_key_can_list_its_own_folder() {
    with_gateway(|g| async move {
        let signer = g.scoped_key("img/").await;
        g.seed_objects("media-cdn", &["img/a.png", "docs/secret.pdf"]).await;

        let body = g.get(&signer, "/media-cdn?list-type=2&prefix=img/").await.text();

        assert!(body.contains("<Key>img/a.png</Key>"));
        assert!(!body.contains("secret.pdf"));
    })
    .await;
}

/// A prefix that is a parent of the allowed one is still refused: listing `im` would return
/// `img/...` and thereby disclose the folder structure the scope is meant to fence off.
#[tokio::test]
#[serial]
async fn a_prefix_above_the_allowed_one_is_refused() {
    with_gateway(|g| async move {
        let signer = g.scoped_key("img/").await;

        let res = g.get(&signer, "/media-cdn?list-type=2&prefix=im").await;

        assert_eq!(res.status_code(), 403);
    })
    .await;
}

/// max-keys is capped at 1000; a client asking for more gets 1000, not an error.
#[tokio::test]
#[serial]
async fn max_keys_is_capped_not_rejected() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.seed_objects("media-cdn", &["a"]).await;

        let body = g.get(&signer, "/media-cdn?list-type=2&max-keys=99999").await.text();

        assert!(body.contains("<MaxKeys>1000</MaxKeys>"));
    })
    .await;
}

/// encoding-type=url is what botocore sends when a key contains characters that break XML.
#[tokio::test]
#[serial]
async fn encoding_type_url_encodes_keys_and_prefixes() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.seed_objects("media-cdn", &["a b/c&d.png"]).await;

        let body = g
            .get(&signer, "/media-cdn?list-type=2&encoding-type=url")
            .await
            .text();

        assert!(body.contains("<EncodingType>url</EncodingType>"));
        assert!(body.contains("a%20b/c%26d.png"));
    })
    .await;
}
```

`a_prefix_above_the_allowed_one_is_refused` không có trong bộ conformance nhưng nó là chỗ rò thật: prefix `im` không khớp `img/` theo luật ở G3, nhưng một implementation "giao prefix yêu cầu với prefix được phép" có thể trả về `img/...` — tức là để lộ cấu trúc thư mục mà scope định che.

- [ ] **Step 2: Chạy để chắc nó fail**

Run: `cargo test --test mod requests::s3::listing 2>&1 | tail -10`

- [ ] **Step 3: Viết handler và XML**

Luật authorize cho list, viết ra rõ vì nó khác các verb khác:

```rust
/// Whether this key may list `prefix`.
///
/// A key with no prefixes may list anything. A scoped key may list only a prefix that is *at or
/// below* one of its own: `img/` and `img/2026/` are fine, `im` and `` are not.
///
/// The check is on the requested prefix rather than on the rows returned. Filtering afterwards
/// would mean the query had already read keys the caller may not see, and one missed filter is a
/// disclosure.
fn may_list(allowed: &[String], prefix: &str) -> bool {
    if allowed.is_empty() {
        return true;
    }
    allowed.iter().any(|a| prefix_allows(a, prefix) || prefix.starts_with(a.as_str()))
}
```

Chú ý `prefix_allows(a, prefix)` xử lý trường hợp `prefix` bằng hoặc dưới `a`; `prefix.starts_with(a)` là trường hợp `prefix = "img/2026/"` với `a = "img/"`. Prefix rỗng và prefix `im` đều không thoả.

`xml::list_objects_v2` dựng đúng thứ tự thẻ mà S3 dùng: `Name`, `Prefix`, `KeyCount`, `MaxKeys`, `Delimiter?`, `IsTruncated`, `ContinuationToken?`, `NextContinuationToken?`, `StartAfter?`, `EncodingType?`, `Contents*`, `CommonPrefixes*`. Escape XML cho mọi key.

`StorageClass` luôn `STANDARD` — gateway không mô hình hoá storage class, và bỏ thẻ này làm một số client báo lỗi.

- [ ] **Step 4: Ba backend và commit**

```bash
cargo test 2>&1 | tail -3
DATABASE_URL=postgres://loco:loco@localhost:5432/osg_test cargo test 2>&1 | tail -3
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo test 2>&1 | tail -3
cargo clippy --all-features --all-targets -- -D warnings -W clippy::pedantic -W clippy::nursery -W rust-2018-idioms
cargo fmt --all
git add src/ tests/
git commit -m "feat(s3): ListObjectsV2 served from the database

No upstream call at all, which is what 'quota is DB-driven, never bucket-scanned'
means for listing too. Prefix scoping is checked on the requested prefix, not by
filtering the rows that came back: filtering afterwards means the query already
read keys the caller may not see, and one missed filter is a disclosure.

A prefix above the allowed one is refused — listing 'im' with a key scoped to
'img/' would disclose the folder structure the scope exists to fence off."
```

---

## Task 4: ListBuckets và HeadBucket

**Files:**
- Modify: `src/controllers/s3/listing.rs`, `src/s3/xml.rs`, `src/controllers/s3/mod.rs`
- Test: `tests/requests/s3/listing.rs`

**Interfaces:**
- Consumes: `S3Request::resolve_bucket_only`, `buckets::Model::list_for_user`.
- Produces: `listing::list_buckets`, `listing::head_bucket`.

- [ ] **Step 1: Viết test**

```rust
/// test_bucket.py::test_list_buckets_returns_logical_buckets
#[tokio::test]
#[serial]
async fn list_buckets_returns_only_this_users_buckets() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.extra_bucket("archive").await;
        g.other_user_bucket("not-mine").await;

        let body = g.get(&signer, "/").await.text();

        assert!(body.contains("<ListAllMyBucketsResult"));
        assert!(body.contains("<Name>media-cdn</Name>"));
        assert!(body.contains("<Name>archive</Name>"));
        assert!(!body.contains("not-mine"), "another user's bucket must not appear");
        assert!(!body.contains("osg-main"), "the physical bucket must not appear");
        assert!(body.contains("<CreationDate>"));
        g.mock.assert_untouched();
    })
    .await;
}

/// test_bucket.py::test_head_bucket
#[tokio::test]
#[serial]
async fn head_bucket_is_200_with_no_body() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;

        let res = g.head(&signer, "/media-cdn").await;

        assert_eq!(res.status_code(), 200);
        assert!(res.text().is_empty());
        g.mock.assert_untouched();
    })
    .await;
}

#[tokio::test]
#[serial]
async fn head_bucket_on_a_missing_bucket_is_a_bare_404() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;

        let res = g.head(&signer, "/nope").await;

        assert_eq!(res.status_code(), 404);
        assert!(res.text().is_empty(), "HEAD carries no body, not even an error one");
    })
    .await;
}

/// test_scoping.py::test_head_bucket_with_a_scoped_key
/// A scope limits objects, not whether the bucket exists.
#[tokio::test]
#[serial]
async fn head_bucket_works_with_a_scoped_key() {
    with_gateway(|g| async move {
        let signer = g.scoped_key("img/").await;

        let res = g.head(&signer, "/media-cdn").await;

        assert_eq!(res.status_code(), 200);
    })
    .await;
}

/// ListBuckets needs no object action: a key with only `read` still enumerates its buckets, the
/// same way an IAM key with s3:ListAllMyBuckets does.
#[tokio::test]
#[serial]
async fn list_buckets_needs_no_object_permission() {
    with_gateway(|g| async move {
        let signer = g.key_with(&["read"], &["img/"]).await;

        let res = g.get(&signer, "/").await;

        assert_eq!(res.status_code(), 200);
        assert!(res.text().contains("<Name>media-cdn</Name>"));
    })
    .await;
}
```

- [ ] **Step 2: Chạy để chắc nó fail, viết, chạy lại**

Run: `cargo test --test mod requests::s3::listing 2>&1 | tail -10`

`xml::list_buckets` dựng `<ListAllMyBucketsResult><Owner><ID/><DisplayName/></Owner><Buckets><Bucket><Name/><CreationDate/></Bucket>…</Buckets></ListAllMyBucketsResult>`.

`Owner.ID` dùng `user.pid`, `DisplayName` dùng `user.name`. Không dùng email — nó lộ địa chỉ của tài khoản vào một response mà bất kỳ key nào của tài khoản đó đọc được, kể cả key đưa cho bên thứ ba.

- [ ] **Step 3: Ba backend và commit**

```bash
cargo test 2>&1 | tail -3
DATABASE_URL=postgres://loco:loco@localhost:5432/osg_test cargo test 2>&1 | tail -3
DATABASE_URL=mysql://loco:loco@localhost:3306/osg_test cargo test 2>&1 | tail -3
cargo clippy --all-features --all-targets -- -D warnings -W clippy::pedantic -W clippy::nursery -W rust-2018-idioms
cargo fmt --all
git add src/ tests/
git commit -m "feat(s3): ListBuckets and HeadBucket

Both read from the database and never touch upstream. Owner.DisplayName is the
account name, not its email: a scoped key handed to a third party can read this
response, and the email does not belong in it. A scope limits objects, not
whether a bucket exists, so HeadBucket with a scoped key is 200."
```

---

## Task 5: Nghiệm thu với client thật

- [ ] **Step 1: Dựng lại MinIO theo `docs/docker.md`, cấu hình pool và bucket**

- [ ] **Step 2: Ghi vài object rồi liệt kê bằng aws-cli**

```bash
export AWS_ACCESS_KEY_ID=OSG… AWS_SECRET_ACCESS_KEY=… H=http://localhost:5150

for p in img/2026/a.png img/2026/b.png img/2025/c.png docs/d.pdf top.txt; do
  echo x | aws s3 cp - "s3://media-cdn/$p" --endpoint-url $H
done

echo "--- recursive ---"
aws s3 ls s3://media-cdn/ --recursive --endpoint-url $H
echo "--- top level, delimiter roll-up ---"
aws s3 ls s3://media-cdn/ --endpoint-url $H
echo "--- inside a prefix ---"
aws s3 ls s3://media-cdn/img/ --endpoint-url $H
echo "--- buckets ---"
aws s3 ls --endpoint-url $H
```

Expected: `aws s3 ls s3://media-cdn/` hiện `PRE docs/`, `PRE img/`, và file `top.txt`. `aws s3 ls s3://media-cdn/img/` hiện `PRE 2025/`, `PRE 2026/`.

`PRE` là cách aws-cli in `CommonPrefixes`. Nếu nó liệt kê phẳng hết mọi key thì delimiter roll-up không chạy — mà test unit vẫn xanh, vì aws-cli gửi `delimiter=/` còn test gửi tham số khác tên.

- [ ] **Step 3: Phân trang với hơn 1000 object**

```bash
for i in $(seq 1 1200); do echo x | aws s3 cp - "s3://media-cdn/bulk/k$i" --endpoint-url $H; done
aws s3 ls s3://media-cdn/bulk/ --recursive --endpoint-url $H | wc -l
```

Expected: `1200`. Đây là bước duy nhất kiểm token thật với client thật — aws-cli tự lặp `continuation-token`, và một token lệch sẽ ra số khác 1200 chứ không ra lỗi.

- [ ] **Step 4: rclone**

```bash
rclone config create osg s3 provider=Other env_auth=false \
  access_key_id=$AWS_ACCESS_KEY_ID secret_access_key=$AWS_SECRET_ACCESS_KEY \
  endpoint=$H
rclone lsd osg:media-cdn
rclone ls osg:media-cdn/img
```

rclone dựng canonical request khác botocore một chút, nên nó là client thứ hai độc lập — và `FUTURE.md` nêu tên nó.

- [ ] **Step 5: Commit ghi chú**

```bash
git add docs/
git commit -m "docs: listing checks with aws-cli and rclone"
```

---

## Self-review

**Phủ spec.** Mục 9 toàn bộ → task 1–4. Bảng đối chiếu spec mục 20: `test_bucket.py` (7) → task 3, 4; ba test list của `test_scoping.py` → task 3; `test_list_objects_v2_xml_is_s3_shaped` của `test_wire.py` → task 3.

**Chưa phủ, cố ý.** `ListObjects` V1 trả 501 (G3 đã làm). Multipart/Copy/Presigned → G6. `is_latest` có trong query nhưng versioning chưa bật — đúng như spec mục 8 quyết định.

**Nhất quán kiểu.** `ListQuery` khai task 1. `Page`/`Row`/`roll_up`/`encode_token`/`decode_token` khai task 2, dùng task 3. `may_list` khai task 3. `prefix_allows` từ G3 task 1.

**Rủi ro đã biết.**

1. **Trôi giữa DB và store.** Listing đọc DB, nên object ghi trực tiếp lên physical bucket không xuất hiện. `reconcile_quota` sửa counter nhưng không sinh row `objects`. Đã ghi trong spec mục 9; cần một dòng trong `docs/docker.md`: credential của pool không được chia sẻ cho công cụ khác.
2. **`may_list` có hai điều kiện `or`.** Đó là chỗ dễ viết sai theo hướng quá rộng. Test `a_prefix_above_the_allowed_one_is_refused` canh hướng đó, nhưng nó là test tự viết — không có đáp án bên ngoài. Đáng thêm một test cho `prefix = "img/2026/"` với key scoped `img/` để chốt cả hai chiều.
3. **`aws s3 ls` không cùng tham số với test.** aws-cli gửi `delimiter=/` và `prefix=` theo cách riêng. Bước 2 của task 5 là cái duy nhất bắt được sai lệch đó; test unit không bắt được.
4. **Thứ tự thẻ XML.** botocore parse theo tên nên thứ tự không quan trọng với nó, nhưng một số client cũ thì có. Giữ đúng thứ tự của S3 là rẻ, nên làm.
