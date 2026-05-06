//! ConnectRPC on Cloudflare Workers.
//!
//! The Worker runtime speaks web_sys Request/Response. With `worker`'s `http`
//! feature those convert to/from `http::Request<worker::Body>` and
//! `http::Response<B: http_body::Body<Data = Bytes>>`, which are exactly the
//! shapes `connectrpc::ConnectRpcService` consumes and produces. So the fetch
//! handler's job is one line: drive the tower service and return its response.

#![allow(refining_impl_trait)]

use std::sync::{Arc, LazyLock};

use connectrpc::{
    ConnectRpcBody, ConnectRpcService, RequestContext, Response, Router as RpcRouter, ServiceResult,
};
use tower::{Layer, Service};
use worker::{Context, Env, HttpRequest, event};

use buffa::view::OwnedView;

use proto::workers::clock::v1::ClockServiceExt;
use proto::workers::echo::v1::EchoServiceExt;
use proto::workers::greet::v1::{GreetRequestView, GreetResponse, GreetService, GreetServiceExt};
use proto::workers::heartbeat::v1::HeartbeatServiceExt;
use proto::workers::reverse::v1::{
    ReverseRequestView, ReverseResponse, ReverseService, ReverseServiceExt,
};
use proto::workers::todo::v1::TodoServiceExt;

// `connectrpc-build` unified mode emits `super::`-relative paths that resolve
// against this module name.
#[allow(warnings, unused)]
pub(crate) mod proto {
    connectrpc::include_generated!();
}

mod clock;
mod echo;
mod heartbeat;
mod middleware;
mod routes;
mod todo;

use clock::Clock;
use echo::Echoer;
use heartbeat::Heartbeat;
use middleware::{RequestId, RequestIdLayer};
use todo::TodoServer;

struct Greeter;

impl GreetService for Greeter {
    async fn greet(
        &self,
        ctx: RequestContext,
        request: OwnedView<GreetRequestView<'static>>,
    ) -> ServiceResult<GreetResponse> {
        let name = if request.name.is_empty() {
            "world"
        } else {
            request.name
        };
        let mut res = Response::new(GreetResponse {
            greeting: format!("Hello, {name}!"),
            ..Default::default()
        });
        if let Some(id) = ctx.extensions.get::<RequestId>() {
            res = res.with_trailer(
                http::header::HeaderName::from_static(middleware::HEADER_NAME),
                id.0.clone(),
            );
        }
        Ok(res)
    }
}

struct Reverser;

impl ReverseService for Reverser {
    async fn reverse(
        &self,
        _ctx: RequestContext,
        request: OwnedView<ReverseRequestView<'static>>,
    ) -> ServiceResult<ReverseResponse> {
        // Codepoint reversal; grapheme clusters (emoji with combining marks) will split.
        let reversed: String = request.text.chars().rev().collect();
        Response::ok(ReverseResponse {
            reversed,
            ..Default::default()
        })
    }
}

#[event(fetch, respond_with_errors)]
async fn fetch(
    req: HttpRequest,
    env: Env,
    _ctx: Context,
) -> worker::Result<http::Response<ConnectRpcBody>> {
    if let Some(resp) = routes::try_handle(&req) {
        return Ok(resp);
    }

    // The layer is hoisted out of the per-request path so its monotonic
    // counter persists across requests within the same isolate.
    static LAYER: LazyLock<RequestIdLayer> = LazyLock::new(RequestIdLayer::new);

    let router = RpcRouter::new();
    let router = Arc::new(Greeter).register(router);
    let router = Arc::new(Reverser).register(router);
    let router = Arc::new(Clock).register(router);
    let router = Arc::new(Echoer).register(router);
    let router = Arc::new(Heartbeat).register(router);
    let router = register_todo_service(router, &env).await?;
    let mut svc = LAYER.layer(ConnectRpcService::new(router));
    let response = svc.call(req).await.unwrap();
    Ok(response)
}

// The backing store differs per target: wasm32 uses D1 (real deployment);
// native falls back to an in-memory store so `cargo test` / `cargo check`
// on the host can exercise the handler code without wasm-only bindings.
#[cfg(target_arch = "wasm32")]
async fn register_todo_service(router: RpcRouter, env: &Env) -> worker::Result<RpcRouter> {
    use std::sync::atomic::{AtomicBool, Ordering};
    // `CREATE TABLE IF NOT EXISTS` on every request would cost a D1 roundtrip
    // per fetch. Workers reuses isolates across requests, so gate it behind a
    // per-isolate flag — races are harmless (the DDL is idempotent).
    static SCHEMA_READY: AtomicBool = AtomicBool::new(false);

    let db = env.d1("DB")?;
    let store = todo::D1TodoStore::new(db);
    if !SCHEMA_READY.load(Ordering::Relaxed) {
        store
            .ensure_schema()
            .await
            .map_err(|e| worker::Error::RustError(format!("todo schema: {e}")))?;
        SCHEMA_READY.store(true, Ordering::Relaxed);
    }
    Ok(Arc::new(TodoServer::new(store)).register(router))
}

#[cfg(not(target_arch = "wasm32"))]
async fn register_todo_service(router: RpcRouter, _env: &Env) -> worker::Result<RpcRouter> {
    let store = todo::InMemoryTodoStore::new();
    Ok(Arc::new(TodoServer::new(store)).register(router))
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor::block_on;

    use proto::workers::greet::v1::GreetRequest;
    use proto::workers::reverse::v1::ReverseRequest;

    fn greet(name: &str) -> GreetResponse {
        let req = GreetRequest {
            name: name.to_owned(),
            ..Default::default()
        };
        let view = OwnedView::<GreetRequestView<'static>>::from_owned(&req)
            .expect("build GreetRequest view");
        block_on(Greeter.greet(RequestContext::default(), view))
            .expect("greet should not error")
            .body
            .into()
    }

    fn reverse(text: &str) -> ReverseResponse {
        let req = ReverseRequest {
            text: text.to_owned(),
            ..Default::default()
        };
        let view = OwnedView::<ReverseRequestView<'static>>::from_owned(&req)
            .expect("build ReverseRequest view");
        block_on(Reverser.reverse(RequestContext::default(), view))
            .expect("reverse should not error")
            .body
            .into()
    }

    #[test]
    fn greet_named_user() {
        assert_eq!(greet("Ada").greeting, "Hello, Ada!");
    }

    #[test]
    fn greet_empty_name_falls_back_to_world() {
        assert_eq!(greet("").greeting, "Hello, world!");
    }

    #[test]
    fn greet_preserves_unicode_name() {
        assert_eq!(greet("世界").greeting, "Hello, 世界!");
    }

    #[test]
    fn reverse_ascii() {
        assert_eq!(reverse("hello").reversed, "olleh");
    }

    #[test]
    fn reverse_empty_string() {
        assert_eq!(reverse("").reversed, "");
    }

    #[test]
    fn reverse_is_involutive_on_ascii() {
        let once = reverse("ConnectRPC").reversed;
        let twice = reverse(&once).reversed;
        assert_eq!(twice, "ConnectRPC");
    }

    #[test]
    fn reverse_handles_multibyte_codepoints() {
        // Each char is a single codepoint, so chars().rev() round-trips cleanly.
        assert_eq!(reverse("héllo").reversed, "olléh");
        assert_eq!(reverse("日本語").reversed, "語本日");
    }

    #[test]
    fn greet_echoes_request_id_trailer_when_present() {
        let mut ctx = RequestContext::default();
        ctx.extensions
            .insert(RequestId(http::HeaderValue::from_static("trace-xyz")));
        let req = GreetRequest {
            name: "Ada".into(),
            ..Default::default()
        };
        let view = OwnedView::<GreetRequestView<'static>>::from_owned(&req).unwrap();
        let resp = block_on(Greeter.greet(ctx, view)).unwrap();
        assert_eq!(
            resp.trailers
                .get("x-request-id")
                .expect("trailer must be set")
                .to_str()
                .unwrap(),
            "trace-xyz"
        );
    }

    #[test]
    fn greet_omits_trailer_when_no_request_id_in_extensions() {
        // If the middleware never ran (e.g. a handler invoked directly in a
        // unit test), the trailer stays absent rather than defaulting.
        assert!(!block_on(async {
            let req = GreetRequest::default();
            let view = OwnedView::<GreetRequestView<'static>>::from_owned(&req).unwrap();
            let resp = Greeter
                .greet(RequestContext::default(), view)
                .await
                .unwrap();
            resp.trailers.contains_key("x-request-id")
        }));
    }
}
