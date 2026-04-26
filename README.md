# example-connectrpc-worker

A Rust Cloudflare Worker that serves [ConnectRPC](https://connectrpc.com/)
over HTTP, using [`connect-rust`](https://github.com/anthropics/connect-rust)
on top of the [`worker`](https://crates.io/crates/worker) crate. Exists to
prove out what the integration actually looks like end to end: fetch
handler, codegen, tower middleware, D1 persistence.

## What's in here

Three services, all defined as proto under `proto/workers/`:

- `GreetService`: takes a name, returns `"Hello, {name}!"`. Reads a
  middleware-assigned `RequestId` out of `ctx.extensions` and echoes it as
  an `x-request-id` response trailer.
- `ReverseService`: reverses a string (codepoint-wise).
- `TodoService`: full CRUD (create / get / list / update / delete),
  backed by Cloudflare D1 in production and an in-memory `BTreeMap` in
  tests. The handler is generic over a `TodoStore` trait so the same code
  runs against either store.

One tower middleware:

- `RequestIdLayer`: stamps each incoming `http::Request` with a
  `RequestId` in its extensions before `ConnectRpcService` dispatches.
  Honors an incoming `x-request-id` header if set, otherwise draws from a
  per-isolate monotonic counter. `ConnectRpcService` moves request
  extensions into `Context::extensions`, so handlers read the id with
  `ctx.extensions.get::<RequestId>()`.

And a couple of non-RPC HTTP routes (`src/routes.rs`):

- `GET /healthz`: plain-text liveness probe.
- `GET /oauth/callback`: dummy OAuth callback that parses `code` /
  `state` from the query string. The `fetch` handler matches these
  before falling through to the RPC stack — a pattern for one-off
  endpoints (webhooks, SSO callbacks, admin pages) that don't want RPC
  dispatch or the middleware stack wrapping it.

## How the pieces fit

```
fetch (event handler)
  ├─ routes::try_handle        ← one-off HTTP routes (OAuth cb, /healthz, ...)
  │                              short-circuit here if matched
  └─ RequestIdLayer            ← inserts RequestId into req.extensions
       └─ ConnectRpcService    ← parses path, decodes body, dispatches
            └─ Router          ← GreetService, ReverseService, TodoService
                 └─ TodoServer<S: TodoStore>
                      └─ D1TodoStore (wasm) | InMemoryTodoStore (native)
```

The `worker` crate's `http` feature converts web_sys `Request`/`Response`
to/from `http::Request<worker::Body>` / `http::Response<B>`, which is
exactly what `ConnectRpcService` consumes and produces. The fetch handler
is therefore just `svc.call(req).await`.

## Layout

```
proto/workers/{greet,reverse,todo}/v1/*.proto   # service definitions
build.rs                                        # connectrpc-build driver
src/lib.rs                                      # fetch handler, Greeter, Reverser
src/middleware.rs                               # RequestId tower layer
src/routes.rs                                   # non-RPC HTTP routes (OAuth cb, healthz)
src/todo.rs                                     # TodoStore trait + impls + TodoServer
migrations/0001_init_todos.sql                  # D1 schema
wrangler.toml                                   # worker + D1 binding config
```

## Running tests

Two layers:

```
cargo test                       # native unit tests (handlers, store, tower layer)
cd integration-tests && npm test # wasm worker under miniflare, exercised through a
                                 # generated TypeScript Connect client
```

Native unit tests cover service handlers, the `TodoStore` in-memory impl,
and the tower layer (23 tests). The D1 store is
`#[cfg(target_arch = "wasm32")]` only. Its futures wrap `JsFuture` with
`worker::send::IntoSendFuture::into_send()` to satisfy the `+ Send` bound
on the generated service traits, and that machinery only lines up on
wasm32.

The integration harness builds the wasm worker, loads it into miniflare
with a real D1 binding, and drives it through a Connect-ES client
generated from the same `proto/` tree the Rust server consumes — so each
test runs an end-to-end request: protobuf-es encode → Connect HTTP envelope
→ wasm fetch handler → connectrpc dispatch → Rust handler → wasm reply →
protobuf-es decode. Both binary and JSON codecs are exercised. See
[`integration-tests/`](integration-tests/) for layout.

## Building for Cloudflare

```
cargo check --target wasm32-unknown-unknown   # type-check the wasm build
wrangler dev                                  # local dev server
wrangler deploy                               # ship it
```

The `[build]` section in `wrangler.toml` runs `worker-build` during
`wrangler dev` / `wrangler deploy`, which compiles the `cdylib` to wasm
and emits the glue JS under `build/`.

## D1 setup

`wrangler.toml` declares a `DB` binding for the Todo service with a
placeholder `database_id`. To wire up a real database:

```
wrangler d1 create workers-connectrpc-todos
# paste the returned id into wrangler.toml
wrangler d1 migrations apply workers-connectrpc-todos
```

`D1TodoStore::ensure_schema` also runs `CREATE TABLE IF NOT EXISTS` once
per isolate, so local `wrangler dev` works before migrations are applied.

## Notes on the `+ Send` bound

`connectrpc-build` generates service traits whose handler futures require
`+ Send`. On wasm32 that's fine for pure-Rust handlers but breaks for
anything holding a `JsFuture` (`Rc<RefCell<_>>` under the hood) across an
`.await`. The workaround lives in `src/todo.rs`: every D1 call is wrapped
with `.into_send()`, which the `worker` crate provides specifically for
this single-threaded-wasm context.
