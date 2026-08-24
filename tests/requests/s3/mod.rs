//! The S3 data-plane test harness.
//!
//! Every test here runs against a real app instance with a mock object store behind it, so a test can assert the thing that matters most: that the store was never touched.
mod read;
mod scoping;
mod wire;

use std::future::Future;

use axum_test::{TestResponse, TestServer};
use chrono::{DateTime, Utc};
use loco_rs::{app::AppContext, testing::prelude::*};
use object_storage_gate::{
    app::App,
    models::{access_keys, buckets, pools, users},
};

use crate::support::{
    mock_upstream::{Canned, MockUpstream},
    signer::{encode_path, TestSigner},
};

pub struct TestGateway {
    pub request: TestServer,
    pub ctx: AppContext,
    pub mock: MockUpstream,
    pub user: users::Model,
    pub bucket: buckets::Model,
    pub pool: pools::Model,
}

/// Boots an app, a mock upstream, a pool pointing at it, a user and one bucket.
pub async fn with_gateway<F, Fut>(f: F)
where
    F: FnOnce(TestGateway) -> Fut,
    Fut: Future<Output = ()>,
{
    request::<App, _, _>(|request, ctx| async move {
        let mock = MockUpstream::start().await;
        let pool = pools::Model::create(
            &ctx.db,
            &pools::CreateParams {
                name: "main".to_string(),
                provider: pools::PROVIDER_CUSTOM.to_string(),
                region: Some("us-east-1".to_string()),
                api_endpoint: Some(mock.base_url.clone()),
                physical_bucket: "osg-main".to_string(),
                access_id: Some("FIXTUREUPSTREAMID".to_string()),
                access_secret: Some("fixture-upstream-secret".to_string()),
            },
        )
        .await
        .unwrap();

        let user = crate::requests::prepare_data::create_user(
            &ctx,
            "tenant@osg.vn",
            "12341234",
            "Tenant",
            users::ROLE_USER,
        )
        .await;
        let bucket = buckets::Model::create(&ctx.db, user.id, pool.id, "media-cdn", 0)
            .await
            .unwrap();

        f(TestGateway {
            request,
            ctx,
            mock,
            user,
            bucket,
            pool,
        })
        .await;
    })
    .await;
}

impl TestGateway {
    /// A key with every action and no prefix confinement.
    pub async fn full_key(&self) -> TestSigner {
        self.key_with(
            &[
                access_keys::ACTION_READ,
                access_keys::ACTION_WRITE,
                access_keys::ACTION_DELETE,
                access_keys::ACTION_LIST,
                access_keys::ACTION_MULTIPART,
                access_keys::ACTION_PRESIGNED,
            ],
            &[],
        )
        .await
    }

    /// A read-only key confined to one prefix.
    pub async fn scoped_key(&self, prefix: &str) -> TestSigner {
        self.key_with(
            &[access_keys::ACTION_READ, access_keys::ACTION_LIST],
            &[prefix],
        )
        .await
    }

    pub async fn key_with(&self, actions: &[&str], prefixes: &[&str]) -> TestSigner {
        let (key, secret) = access_keys::Model::create_key(
            &self.ctx.db,
            self.user.id,
            &access_keys::CreateKeyParams {
                label: "primary".to_string(),
                expires_at: None,
                permissions: actions.iter().map(|s| (*s).to_string()).collect(),
                prefixes: prefixes.iter().map(|s| (*s).to_string()).collect(),
            },
        )
        .await
        .unwrap();
        TestSigner::new(&key.access_key_id, &secret)
    }

    pub async fn revoke_key(&self, signer: &TestSigner) {
        let key = access_keys::Model::find_by_access_key_id(&self.ctx.db, &signer.access_key_id)
            .await
            .unwrap();
        key.revoke(&self.ctx.db).await.unwrap();
    }

    /// Puts the pool back into the state the backfill migration leaves it in.
    pub async fn strip_pool_credentials(&self) {
        use sea_orm::{ActiveModelTrait, ActiveValue};
        let mut am: pools::ActiveModel = self.pool.clone().into();
        am.access_id = ActiveValue::set(None);
        am.access_secret_encrypted = ActiveValue::set(None);
        am.update(&self.ctx.db).await.unwrap();
    }

    /// A bucket owned by somebody else, to prove it reads as absent.
    pub async fn other_user_bucket(&self, name: &str) -> buckets::Model {
        let other = crate::requests::prepare_data::create_user(
            &self.ctx,
            "other@osg.vn",
            "12341234",
            "Other",
            users::ROLE_USER,
        )
        .await;
        buckets::Model::create(&self.ctx.db, other.id, self.pool.id, name, 0)
            .await
            .unwrap()
    }

    /// The physical key the gateway should have addressed for this bucket.
    #[must_use]
    pub fn physical(&self, logical_key: &str) -> String {
        format!(
            "osg-main/{}/{}/{}",
            self.user.pid, self.bucket.name, logical_key
        )
    }

    pub async fn request(
        &self,
        signer: &TestSigner,
        method: &str,
        path: &str,
        body: &[u8],
        extra: &[(&str, &str)],
    ) -> TestResponse {
        self.request_encoded(signer, method, &encode_path(path), body, extra)
            .await
    }

    /// Sends a path exactly as given, without encoding it first.
    ///
    /// Needed for traversal tests: a literal `..` in a request line is normalised away before the server ever sees it, so the only reachable form is the percent-encoded one — which is also the only form an attacker would use.
    pub async fn request_encoded(
        &self,
        signer: &TestSigner,
        method: &str,
        encoded_path: &str,
        body: &[u8],
        extra: &[(&str, &str)],
    ) -> TestResponse {
        let headers = signer.sign(method, encoded_path, &[], body, extra);
        self.send(method, encoded_path, &headers, body).await
    }

    pub async fn get(&self, signer: &TestSigner, path: &str) -> TestResponse {
        self.request(signer, "GET", path, b"", &[]).await
    }

    pub async fn get_with(
        &self,
        signer: &TestSigner,
        path: &str,
        extra: &[(&str, &str)],
    ) -> TestResponse {
        self.request(signer, "GET", path, b"", extra).await
    }

    pub async fn head(&self, signer: &TestSigner, path: &str) -> TestResponse {
        self.request(signer, "HEAD", path, b"", &[]).await
    }

    pub async fn put(&self, signer: &TestSigner, path: &str, body: &[u8]) -> TestResponse {
        self.request(signer, "PUT", path, body, &[]).await
    }

    pub async fn get_at(&self, signer: &TestSigner, path: &str, at: DateTime<Utc>) -> TestResponse {
        let encoded = encode_path(path);
        let headers = signer.sign_at(at, "GET", &encoded, &[], b"", &[]);
        self.send("GET", &encoded, &headers, b"").await
    }

    /// A request with a deliberately wrong signature.
    pub async fn get_tampered(&self, signer: &TestSigner, path: &str) -> TestResponse {
        let encoded = encode_path(path);
        let headers = signer.sign_tampered("GET", &encoded, b"");
        self.send("GET", &encoded, &headers, b"").await
    }

    /// A request with a payload hash the client chose, for the aws-chunked case.
    pub async fn put_with_payload_hash(
        &self,
        signer: &TestSigner,
        path: &str,
        payload_hash: &str,
        body: &[u8],
    ) -> TestResponse {
        let encoded = encode_path(path);
        let mut headers = signer.sign("PUT", &encoded, &[], body, &[]);
        for h in &mut headers {
            if h.0 == "x-amz-content-sha256" {
                h.1 = payload_hash.to_string();
            }
        }
        self.send("PUT", &encoded, &headers, body).await
    }

    /// No signing at all — a browser navigation, as far as the gateway is concerned.
    pub async fn raw_get(&self, path: &str, headers: &[(String, String)]) -> TestResponse {
        self.send("GET", path, headers, b"").await
    }

    /// An S3 client that presented credentials the gateway cannot use.
    ///
    /// This is what "unauthenticated S3 request" has to mean here: a request carrying no `SigV4` credentials at all is indistinguishable from a browser asking for a console page, and the console owns those paths.
    pub async fn unauthenticated_get(&self, path: &str) -> TestResponse {
        self.send(
            "GET",
            &encode_path(path),
            &[(
                "authorization".to_string(),
                "AWS4-HMAC-SHA256 not-a-real-credential".to_string(),
            )],
            b"",
        )
        .await
    }

    async fn send(
        &self,
        method: &str,
        path: &str,
        headers: &[(String, String)],
        body: &[u8],
    ) -> TestResponse {
        let mut req = match method {
            "GET" => self.request.get(path),
            "HEAD" => self.request.method(axum::http::Method::HEAD, path),
            "PUT" => self.request.put(path),
            "POST" => self.request.post(path),
            "DELETE" => self.request.delete(path),
            other => panic!("unsupported method {other}"),
        };
        for (k, v) in headers {
            req = req.add_header(
                axum::http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                axum::http::HeaderValue::from_str(v).unwrap(),
            );
        }
        req.bytes(body.to_vec().into()).await
    }
}

#[must_use]
pub fn header(res: &TestResponse, name: &str) -> String {
    res.headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string()
}

#[must_use]
pub fn canned(status: u16, body: &[u8]) -> Canned {
    Canned {
        status,
        headers: Vec::new(),
        body: body.to_vec(),
    }
}
