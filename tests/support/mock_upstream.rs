//! An axum server standing in for the object store.
//!
//! It records what it received, so a test can assert on the physical key the gateway sent — and, more importantly, can assert that it received *nothing at all*.
//! "Upstream saw no request" is a stronger claim than "the client got a 403": it catches the case where the gateway called upstream and only then refused, which means the bytes had already crossed the isolation boundary.
use std::sync::{Arc, Mutex};

use axum::{
    body::Bytes,
    extract::{Request, State},
    response::Response,
};

#[derive(Debug, Clone)]
pub struct Seen {
    pub method: String,
    pub path: String,
    pub query: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Seen {
    #[must_use]
    pub fn header(&self, name: &str) -> String {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    }
}

#[derive(Clone, Debug)]
pub struct Canned {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Canned {
    #[must_use]
    pub fn ok(body: &[u8]) -> Self {
        Self {
            status: 200,
            headers: Vec::new(),
            body: body.to_vec(),
        }
    }
}

#[derive(Clone)]
struct Shared {
    seen: Arc<Mutex<Vec<Seen>>>,
    canned: Arc<Mutex<Vec<Canned>>>,
}

pub struct MockUpstream {
    pub base_url: String,
    shared: Shared,
    handle: tokio::task::JoinHandle<()>,
}

impl Drop for MockUpstream {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn record(State(shared): State<Shared>, req: Request) -> Response {
    let method = req.method().to_string();
    let path = req.uri().path().to_string();
    let query = req.uri().query().unwrap_or_default().to_string();
    let headers = req
        .headers()
        .iter()
        .map(|(k, v)| {
            (
                k.as_str().to_string(),
                v.to_str().unwrap_or_default().to_string(),
            )
        })
        .collect();

    let body = axum::body::to_bytes(req.into_body(), usize::MAX)
        .await
        .map(|b| b.to_vec())
        .unwrap_or_default();

    shared.seen.lock().unwrap().push(Seen {
        method,
        path,
        query,
        headers,
        body,
    });

    let next = {
        let mut q = shared.canned.lock().unwrap();
        if q.is_empty() {
            None
        } else {
            Some(q.remove(0))
        }
    };

    let canned = next.unwrap_or_else(|| Canned::ok(b""));
    let mut builder = Response::builder().status(canned.status);
    for (k, v) in &canned.headers {
        builder = builder.header(k, v);
    }
    builder
        .body(axum::body::Body::from(Bytes::from(canned.body)))
        .expect("canned response builds")
}

impl MockUpstream {
    /// Binds an ephemeral port and serves until dropped.
    pub async fn start() -> Self {
        let shared = Shared {
            seen: Arc::new(Mutex::new(Vec::new())),
            canned: Arc::new(Mutex::new(Vec::new())),
        };
        let app = axum::Router::new()
            .fallback(record)
            .with_state(shared.clone());

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind an ephemeral port");
        let addr = listener.local_addr().expect("local addr");
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        Self {
            base_url: format!("http://{addr}"),
            shared,
            handle,
        }
    }

    /// Queues one response.
    /// Requests beyond the queue get 200 with an empty body.
    pub fn push(&self, canned: Canned) {
        self.shared.canned.lock().unwrap().push(canned);
    }

    #[must_use]
    pub fn requests(&self) -> Vec<Seen> {
        self.shared.seen.lock().unwrap().clone()
    }

    /// The assertion that matters most: the gateway refused before touching the store.
    pub fn assert_untouched(&self) {
        let seen = self.requests();
        assert!(
            seen.is_empty(),
            "upstream received {} request(s) it should never have seen: {:?}",
            seen.len(),
            seen.iter()
                .map(|s| format!("{} {}", s.method, s.path))
                .collect::<Vec<_>>()
        );
    }

    /// Asserts the physical key the gateway addressed, which is the rewrite under test.
    ///
    /// Compares decoded paths: what is under test is which object was addressed, not how it was spelled on the wire.
    /// The encoding itself is covered by the `SigV4` vectors.
    pub fn assert_key(&self, n: usize, expected: &str) {
        let seen = self.requests();
        let got = seen
            .get(n)
            .unwrap_or_else(|| panic!("no request at index {n}; upstream saw {}", seen.len()));
        let decoded = percent_encoding::percent_decode_str(got.path.trim_start_matches('/'))
            .decode_utf8_lossy()
            .to_string();
        assert_eq!(decoded, expected);
    }
}
