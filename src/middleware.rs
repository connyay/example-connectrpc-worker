//! Request-id tower middleware.
//!
//! Demonstrates the canonical "stash a value in request extensions before the
//! RPC dispatcher runs, read it from `ctx.extensions` inside the handler"
//! pattern. `ConnectRpcService` moves `http::Request::extensions` into
//! `Context::extensions` during dispatch, so anything a tower layer inserts
//! on the `http::Request` side is visible to handlers.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};

use http::HeaderValue;
use tower::{Layer, Service};

pub const HEADER_NAME: &str = "x-request-id";

/// Per-request identifier, inserted into `http::Request::extensions` by
/// [`RequestIdLayer`] and read from `Context::extensions` by handlers.
///
/// Wraps `HeaderValue` so the id can be written to a header/trailer without
/// a fallible re-parse at the use site.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestId(pub HeaderValue);

/// Layer that assigns each incoming request a [`RequestId`].
///
/// If the request carries an `x-request-id` header the value is honored so
/// callers can propagate their own trace id; otherwise a per-isolate
/// monotonic counter (`req-{n}`) is used. The counter is shared across the
/// cloned services produced by `Layer::layer`, so every wrapped copy of the
/// inner service draws from the same sequence.
#[derive(Clone, Default)]
pub struct RequestIdLayer {
    counter: Arc<AtomicU64>,
}

impl RequestIdLayer {
    pub fn new() -> Self {
        Self::default()
    }
}

impl<S> Layer<S> for RequestIdLayer {
    type Service = RequestIdService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RequestIdService {
            inner,
            counter: Arc::clone(&self.counter),
        }
    }
}

#[derive(Clone)]
pub struct RequestIdService<S> {
    inner: S,
    counter: Arc<AtomicU64>,
}

impl<S, B> Service<http::Request<B>> for RequestIdService<S>
where
    S: Service<http::Request<B>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: http::Request<B>) -> Self::Future {
        let value = req.headers().get(HEADER_NAME).cloned().unwrap_or_else(|| {
            let n = self.counter.fetch_add(1, Ordering::Relaxed);
            // `req-{n}` is ASCII by construction, so it's a valid HeaderValue.
            HeaderValue::try_from(format!("req-{n}")).expect("counter id is ascii")
        });
        req.extensions_mut().insert(RequestId(value));
        self.inner.call(req)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor::block_on;
    use std::convert::Infallible;
    use tower::service_fn;

    async fn capture(req: http::Request<()>) -> Result<RequestId, Infallible> {
        Ok(req
            .extensions()
            .get::<RequestId>()
            .cloned()
            .expect("middleware must insert RequestId"))
    }

    #[test]
    fn assigns_monotonic_id_when_header_missing() {
        let layer = RequestIdLayer::new();
        let mut svc = layer.layer(service_fn(capture));
        let req1 = http::Request::builder().body(()).unwrap();
        let req2 = http::Request::builder().body(()).unwrap();
        let id1 = block_on(svc.call(req1)).unwrap();
        let id2 = block_on(svc.call(req2)).unwrap();
        assert_eq!(id1.0.to_str().unwrap(), "req-0");
        assert_eq!(id2.0.to_str().unwrap(), "req-1");
    }

    #[test]
    fn honors_incoming_x_request_id_header() {
        let mut svc = RequestIdLayer::new().layer(service_fn(capture));
        let req = http::Request::builder()
            .header(HEADER_NAME, "client-trace-abc")
            .body(())
            .unwrap();
        let id = block_on(svc.call(req)).unwrap();
        assert_eq!(id.0.to_str().unwrap(), "client-trace-abc");
    }

    #[test]
    fn passes_non_utf8_header_through_unchanged() {
        // HeaderValue accepts bytes that aren't valid UTF-8; pass them through
        // so the handler sees exactly what the client sent.
        let mut svc = RequestIdLayer::new().layer(service_fn(capture));
        let bytes = HeaderValue::from_bytes(&[0xff, 0xfe]).unwrap();
        let mut req = http::Request::builder().body(()).unwrap();
        req.headers_mut().insert(HEADER_NAME, bytes.clone());
        let id = block_on(svc.call(req)).unwrap();
        assert_eq!(id.0, bytes);
    }
}
