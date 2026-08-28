//! Multipart upload over the whole stack, against a mock store.
use serial_test::serial;

use super::{canned, header, with_gateway};
use crate::support::mock_upstream::Canned;

fn xml_ok(body: &str) -> Canned {
    Canned {
        status: 200,
        headers: vec![("content-type".to_string(), "application/xml".to_string())],
        body: body.as_bytes().to_vec(),
    }
}

fn initiate(upload_id: &str) -> Canned {
    xml_ok(&format!(
        "<InitiateMultipartUploadResult><UploadId>{upload_id}</UploadId></InitiateMultipartUploadResult>"
    ))
}

fn tag_in(body: &str, name: &str) -> String {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let Some(i) = body.find(&open) else {
        return String::new();
    };
    let rest = &body[i + open.len()..];
    let Some(j) = rest.find(&close) else {
        return String::new();
    };
    rest[..j].to_string()
}

/// The `UploadId` a client receives is the gateway's own, never the store's.
#[tokio::test]
#[serial]
async fn create_returns_our_upload_id_not_the_upstream_one() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(initiate("UPSTREAM-SECRET-ID"));

        let res = g
            .request(&signer, "POST", "/media-cdn/big.bin?uploads=", b"", &[])
            .await;

        assert_eq!(res.status_code(), 200, "{}", res.text());
        let body = res.text();
        assert!(body.contains("<InitiateMultipartUploadResult"), "{body}");
        assert!(body.contains("<Bucket>media-cdn</Bucket>"));
        assert!(body.contains("<Key>big.bin</Key>"));
        assert!(
            !body.contains("UPSTREAM-SECRET-ID"),
            "the upstream UploadId leaked: {body}"
        );
        assert!(!tag_in(&body, "UploadId").is_empty());
        g.mock.assert_key(0, &g.physical("big.bin"));
    })
    .await;
}

/// The whole flow: create, two parts, complete — and the object lands with the store's size.
#[tokio::test]
#[serial]
async fn a_full_multipart_upload_records_the_object_once() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(initiate("U1"));
        let created = g
            .request(&signer, "POST", "/media-cdn/big.bin?uploads=", b"", &[])
            .await;
        let upload_id = tag_in(&created.text(), "UploadId");

        for etag in ["\"p1\"", "\"p2\""] {
            g.mock.push(Canned {
                status: 200,
                headers: vec![("etag".to_string(), etag.to_string())],
                body: Vec::new(),
            });
        }
        for n in [1, 2] {
            let res = g
                .request(
                    &signer,
                    "PUT",
                    &format!("/media-cdn/big.bin?uploadId={upload_id}&partNumber={n}"),
                    &vec![0u8; 300],
                    &[],
                )
                .await;
            assert_eq!(res.status_code(), 200, "part {n}: {}", res.text());
        }

        // Both parts are held, nothing is stored yet.
        let b = g.bucket_row("media-cdn").await;
        assert_eq!(b.reserved_bytes, 600);
        assert_eq!(b.used_bytes, 0);

        g.mock.push(xml_ok(
            "<CompleteMultipartUploadResult><ETag>\"final\"</ETag></CompleteMultipartUploadResult>",
        ));
        g.mock.push(Canned {
            status: 200,
            headers: vec![("content-length".to_string(), "600".to_string())],
            body: Vec::new(),
        });

        let res = g
            .request(
                &signer,
                "POST",
                &format!("/media-cdn/big.bin?uploadId={upload_id}"),
                b"<CompleteMultipartUpload><Part><PartNumber>1</PartNumber><ETag>\"p1\"</ETag></Part><Part><PartNumber>2</PartNumber><ETag>\"p2\"</ETag></Part></CompleteMultipartUpload>",
                &[],
            )
            .await;

        assert_eq!(res.status_code(), 200, "{}", res.text());
        let body = res.text();
        // Escaped, because a quoted ETag inside XML is what S3 emits too.
        assert!(body.contains("<ETag>&quot;final&quot;</ETag>"), "{body}");
        assert!(body.contains("<Location>/media-cdn/big.bin</Location>"), "{body}");
        assert!(!body.contains("osg-main"), "physical bucket leaked: {body}");

        let b = g.bucket_row("media-cdn").await;
        assert_eq!(b.reserved_bytes, 0, "the hold was not converted");
        assert_eq!(b.used_bytes, 600, "charged once, not twice");
        assert_eq!(b.object_count, 1);
        let row = g.object_row("media-cdn", "big.bin").await.unwrap();
        assert_eq!(row.size, 600);
        assert_eq!(row.etag, "\"final\"");
    })
    .await;
}

/// Abort gives back exactly what the parts reserved.
#[tokio::test]
#[serial]
async fn abort_releases_every_part_that_was_held() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(initiate("U1"));
        let created = g
            .request(&signer, "POST", "/media-cdn/big.bin?uploads=", b"", &[])
            .await;
        let upload_id = tag_in(&created.text(), "UploadId");

        g.mock.push(Canned {
            status: 200,
            headers: vec![("etag".to_string(), "\"p1\"".to_string())],
            body: Vec::new(),
        });
        g.request(
            &signer,
            "PUT",
            &format!("/media-cdn/big.bin?uploadId={upload_id}&partNumber=1"),
            &vec![0u8; 400],
            &[],
        )
        .await;
        assert_eq!(g.bucket_row("media-cdn").await.reserved_bytes, 400);

        g.mock.push(canned(204, b""));
        let res = g
            .request(
                &signer,
                "DELETE",
                &format!("/media-cdn/big.bin?uploadId={upload_id}"),
                b"",
                &[],
            )
            .await;

        assert_eq!(res.status_code(), 204, "{}", res.text());
        let b = g.bucket_row("media-cdn").await;
        assert_eq!(b.reserved_bytes, 0, "the hold leaked");
        assert_eq!(b.used_bytes, 0);
        assert!(g.object_row("media-cdn", "big.bin").await.is_none());
    })
    .await;
}

/// An `UploadId` issued for one key must not be usable against another.
#[tokio::test]
#[serial]
async fn an_upload_id_cannot_be_replayed_against_another_key() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(initiate("U1"));
        let created = g
            .request(&signer, "POST", "/media-cdn/big.bin?uploads=", b"", &[])
            .await;
        let upload_id = tag_in(&created.text(), "UploadId");

        let res = g
            .request(
                &signer,
                "PUT",
                &format!("/media-cdn/other.bin?uploadId={upload_id}&partNumber=1"),
                b"bytes",
                &[],
            )
            .await;

        assert_eq!(res.status_code(), 404, "{}", res.text());
        assert!(res.text().contains("NoSuchUpload"), "{}", res.text());
        assert_eq!(
            g.mock.requests().len(),
            1,
            "only the create should have gone upstream"
        );
    })
    .await;
}

/// An unknown `UploadId` is `NoSuchUpload`, not a 500.
#[tokio::test]
#[serial]
async fn an_unknown_upload_id_is_no_such_upload() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;

        let res = g
            .request(
                &signer,
                "PUT",
                "/media-cdn/big.bin?uploadId=00000000-0000-0000-0000-000000000000&partNumber=1",
                b"bytes",
                &[],
            )
            .await;

        assert_eq!(res.status_code(), 404, "{}", res.text());
        assert!(res.text().contains("NoSuchUpload"));
        g.mock.assert_untouched();
    })
    .await;
}

/// A part that the store refuses gives its reservation straight back.
#[tokio::test]
#[serial]
async fn a_failed_part_releases_its_reservation() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(initiate("U1"));
        let created = g
            .request(&signer, "POST", "/media-cdn/big.bin?uploads=", b"", &[])
            .await;
        let upload_id = tag_in(&created.text(), "UploadId");

        g.mock.push(canned(503, b"upstream unavailable"));
        let res = g
            .request(
                &signer,
                "PUT",
                &format!("/media-cdn/big.bin?uploadId={upload_id}&partNumber=1"),
                &vec![0u8; 400],
                &[],
            )
            .await;

        assert_eq!(res.status_code(), 500, "{}", res.text());
        assert_eq!(g.bucket_row("media-cdn").await.reserved_bytes, 0);
    })
    .await;
}

/// A key without the multipart action cannot start one.
#[tokio::test]
#[serial]
async fn a_key_without_multipart_cannot_start_an_upload() {
    with_gateway(|g| async move {
        let signer = g.key_with(&["read", "write"], &[]).await;

        let res = g
            .request(&signer, "POST", "/media-cdn/big.bin?uploads=", b"", &[])
            .await;

        assert_eq!(res.status_code(), 403, "{}", res.text());
        g.mock.assert_untouched();
    })
    .await;
}

/// `ListMultipartUploads` reads the gateway's own table: the store's answer would carry physical keys and would list other tenants' uploads in the same physical bucket.
#[tokio::test]
#[serial]
async fn list_multipart_uploads_reads_the_gateways_own_table() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(initiate("U1"));
        let created = g
            .request(&signer, "POST", "/media-cdn/big.bin?uploads=", b"", &[])
            .await;
        let upload_id = tag_in(&created.text(), "UploadId");

        let before = g.mock.requests().len();
        let res = g
            .request(&signer, "GET", "/media-cdn?uploads=", b"", &[])
            .await;

        assert_eq!(res.status_code(), 200, "{}", res.text());
        let body = res.text();
        assert!(body.contains("<ListMultipartUploadsResult"), "{body}");
        assert!(body.contains("<Key>big.bin</Key>"), "{body}");
        assert!(
            body.contains(&format!("<UploadId>{upload_id}</UploadId>")),
            "{body}"
        );
        assert!(!body.contains("osg-main"));
        assert_eq!(
            g.mock.requests().len(),
            before,
            "listing uploads must not call the store"
        );
    })
    .await;
}

/// The gateway re-renders `ListParts` rather than forwarding the store's body, which names the physical key.
#[tokio::test]
#[serial]
async fn list_parts_re_renders_the_upstream_answer() {
    with_gateway(|g| async move {
        let signer = g.full_key().await;
        g.mock.push(initiate("U1"));
        let created = g
            .request(&signer, "POST", "/media-cdn/big.bin?uploads=", b"", &[])
            .await;
        let upload_id = tag_in(&created.text(), "UploadId");

        g.mock.push(xml_ok(
            "<ListPartsResult><Bucket>osg-main</Bucket><Key>osg-main/1111/media-cdn/big.bin</Key><Part><PartNumber>1</PartNumber><ETag>\"p1\"</ETag><Size>300</Size></Part></ListPartsResult>",
        ));

        let res = g
            .request(
                &signer,
                "GET",
                &format!("/media-cdn/big.bin?uploadId={upload_id}"),
                b"",
                &[],
            )
            .await;

        assert_eq!(res.status_code(), 200, "{}", res.text());
        let body = res.text();
        assert!(body.contains("<PartNumber>1</PartNumber>"), "{body}");
        assert!(body.contains("<ETag>&quot;p1&quot;</ETag>"), "{body}");
        assert!(body.contains("<Size>300</Size>"), "{body}");
        assert!(body.contains("<Bucket>media-cdn</Bucket>"), "{body}");
        assert!(!body.contains("osg-main"), "physical layout leaked: {body}");
        assert_eq!(header(&res, "content-type"), "application/xml");
    })
    .await;
}
