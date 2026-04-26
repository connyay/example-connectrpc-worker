//! Non-RPC HTTP routes (OAuth callbacks, webhooks, health checks)
//! short-circuited ahead of the RPC stack.
//!
//! For a handful of routes, `match (method, path)` is simpler than
//! pulling in a full HTTP router. If the count grows, swap this for
//! `axum::Router` with `connect_router.into_axum_service()` as the
//! fallback.

use bytes::Bytes;
use connectrpc::ConnectRpcBody;
use http::{Method, Response, StatusCode};
use http_body_util::Full;
use worker::HttpRequest;

pub fn try_handle(req: &HttpRequest) -> Option<Response<ConnectRpcBody>> {
    match (req.method(), req.uri().path()) {
        (&Method::GET, "/healthz") => Some(healthz()),
        (&Method::GET, "/oauth/callback") => Some(oauth_callback(req.uri().query())),
        _ => None,
    }
}

fn text(status: StatusCode, body: impl Into<Bytes>) -> Response<ConnectRpcBody> {
    Response::builder()
        .status(status)
        .header(http::header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(ConnectRpcBody::Full(Full::new(body.into())))
        .expect("static response builder inputs are valid")
}

fn healthz() -> Response<ConnectRpcBody> {
    text(StatusCode::OK, "ok")
}

fn oauth_callback(query: Option<&str>) -> Response<ConnectRpcBody> {
    let (mut code, mut state) = (None, None);
    for (k, v) in query.map(parse_query).into_iter().flatten() {
        match k {
            "code" => code = Some(v),
            "state" => state = Some(v),
            _ => {}
        }
    }
    let Some(code) = code else {
        return text(StatusCode::BAD_REQUEST, "missing `code` query parameter");
    };
    let state = state.unwrap_or("");
    text(
        StatusCode::OK,
        format!("authorized: code={code}, state={state}"),
    )
}

fn parse_query(query: &str) -> impl Iterator<Item = (&str, &str)> {
    // No percent-decoding; a production OAuth callback would need it.
    query.split('&').filter_map(|pair| pair.split_once('='))
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor::block_on;
    use http_body_util::BodyExt;

    fn request(method: Method, uri: &str) -> HttpRequest {
        http::Request::builder()
            .method(method)
            .uri(uri)
            .body(worker::Body::empty())
            .unwrap()
    }

    fn read_body(resp: Response<ConnectRpcBody>) -> (StatusCode, String) {
        let status = resp.status();
        let bytes = block_on(resp.into_body().collect()).unwrap().to_bytes();
        (status, String::from_utf8(bytes.to_vec()).unwrap())
    }

    #[test]
    fn healthz_returns_ok() {
        let resp = try_handle(&request(Method::GET, "/healthz")).expect("healthz must match");
        let (status, body) = read_body(resp);
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, "ok");
    }

    #[test]
    fn healthz_wrong_method_defers() {
        assert!(try_handle(&request(Method::POST, "/healthz")).is_none());
    }

    #[test]
    fn oauth_callback_echoes_code_and_state() {
        let resp = try_handle(&request(Method::GET, "/oauth/callback?code=abc&state=xyz")).unwrap();
        let (status, body) = read_body(resp);
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, "authorized: code=abc, state=xyz");
    }

    #[test]
    fn oauth_callback_tolerates_missing_state() {
        let resp = try_handle(&request(Method::GET, "/oauth/callback?code=abc")).unwrap();
        let (status, body) = read_body(resp);
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, "authorized: code=abc, state=");
    }

    #[test]
    fn oauth_callback_rejects_missing_code() {
        let resp = try_handle(&request(Method::GET, "/oauth/callback?state=xyz")).unwrap();
        let (status, body) = read_body(resp);
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("missing `code`"));
    }

    #[test]
    fn rpc_paths_defer_to_dispatcher() {
        assert!(
            try_handle(&request(
                Method::POST,
                "/workers.greet.v1.GreetService/Greet"
            ))
            .is_none()
        );
    }
}
