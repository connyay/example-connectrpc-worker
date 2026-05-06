//! Todo service: CRUD backed by a pluggable store.
//!
//! The service is generic over a `TodoStore` trait so the same handler code
//! runs against Cloudflare D1 in production and an in-memory map in tests.

use std::fmt;

use buffa::MessageField;
use buffa::view::OwnedView;
use connectrpc::{ConnectError, RequestContext, Response, ServiceResult};

use crate::proto::workers::todo::v1::{
    CreateTodoRequestView, CreateTodoResponse, DeleteTodoRequestView, DeleteTodoResponse,
    GetTodoRequestView, GetTodoResponse, ListTodosRequestView, ListTodosResponse, Todo,
    TodoService, UpdateTodoRequestView, UpdateTodoResponse,
};

/// A persisted todo record, decoupled from the generated proto `Todo` so the
/// store layer doesn't carry buffa's unknown-fields / cached-size internals.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TodoRecord {
    pub id: i64,
    pub text: String,
    pub done: bool,
}

impl From<TodoRecord> for Todo {
    fn from(record: TodoRecord) -> Self {
        Todo {
            id: record.id,
            text: record.text,
            done: record.done,
            ..Default::default()
        }
    }
}

#[derive(Debug)]
pub struct StoreError(String);

impl StoreError {
    pub fn new(msg: impl Into<String>) -> Self {
        Self(msg.into())
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<StoreError> for ConnectError {
    fn from(err: StoreError) -> Self {
        ConnectError::internal(err.0)
    }
}

pub trait TodoStore: Send + Sync + 'static {
    fn create(
        &self,
        text: String,
    ) -> impl std::future::Future<Output = Result<TodoRecord, StoreError>> + Send;

    fn get(
        &self,
        id: i64,
    ) -> impl std::future::Future<Output = Result<Option<TodoRecord>, StoreError>> + Send;

    fn list(&self)
    -> impl std::future::Future<Output = Result<Vec<TodoRecord>, StoreError>> + Send;

    fn update(
        &self,
        id: i64,
        text: Option<String>,
        done: Option<bool>,
    ) -> impl std::future::Future<Output = Result<Option<TodoRecord>, StoreError>> + Send;

    fn delete(&self, id: i64)
    -> impl std::future::Future<Output = Result<bool, StoreError>> + Send;
}

#[cfg(not(target_arch = "wasm32"))]
use std::collections::BTreeMap;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Mutex;

#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
pub struct InMemoryTodoStore {
    inner: Mutex<InMemoryInner>,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct InMemoryInner {
    next_id: i64,
    todos: BTreeMap<i64, TodoRecord>,
}

#[cfg(not(target_arch = "wasm32"))]
impl InMemoryTodoStore {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(InMemoryInner {
                next_id: 1,
                todos: BTreeMap::new(),
            }),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl TodoStore for InMemoryTodoStore {
    async fn create(&self, text: String) -> Result<TodoRecord, StoreError> {
        let mut g = self.inner.lock().unwrap();
        let id = g.next_id;
        g.next_id += 1;
        let record = TodoRecord {
            id,
            text,
            done: false,
        };
        g.todos.insert(id, record.clone());
        Ok(record)
    }

    async fn get(&self, id: i64) -> Result<Option<TodoRecord>, StoreError> {
        let g = self.inner.lock().unwrap();
        Ok(g.todos.get(&id).cloned())
    }

    async fn list(&self) -> Result<Vec<TodoRecord>, StoreError> {
        let g = self.inner.lock().unwrap();
        Ok(g.todos.values().cloned().collect())
    }

    async fn update(
        &self,
        id: i64,
        text: Option<String>,
        done: Option<bool>,
    ) -> Result<Option<TodoRecord>, StoreError> {
        let mut g = self.inner.lock().unwrap();
        let Some(existing) = g.todos.get_mut(&id) else {
            return Ok(None);
        };
        if let Some(t) = text {
            existing.text = t;
        }
        if let Some(d) = done {
            existing.done = d;
        }
        Ok(Some(existing.clone()))
    }

    async fn delete(&self, id: i64) -> Result<bool, StoreError> {
        let mut g = self.inner.lock().unwrap();
        Ok(g.todos.remove(&id).is_some())
    }
}

// Only compiled for wasm32 — the JsFuture that backs D1 calls is `!Send`
// outside wasm, so the futures can't satisfy the `+ Send` bound on `TodoStore`
// on native targets. Unit tests run on native and use `InMemoryTodoStore`.
#[cfg(target_arch = "wasm32")]
mod d1 {
    use super::{StoreError, TodoRecord, TodoStore};
    use serde::Deserialize;
    // `IntoSendFuture::into_send()` wraps a `!Send` JsFuture in a
    // `SendFuture` so the outer async state machine satisfies the `+ Send`
    // bound on `TodoStore`'s method futures. Workers is single-threaded, so
    // the `unsafe impl Send` on `SendFuture` is sound in this runtime.
    use worker::send::IntoSendFuture;
    use worker::{D1Database, D1Type};

    pub const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS todos (\
        id INTEGER PRIMARY KEY AUTOINCREMENT,\
        text TEXT NOT NULL,\
        done INTEGER NOT NULL DEFAULT 0\
    )";

    pub struct D1TodoStore {
        db: D1Database,
    }

    impl D1TodoStore {
        pub fn new(db: D1Database) -> Self {
            Self { db }
        }

        /// Create the `todos` table if it doesn't exist. Convenient for local
        /// `wrangler dev` where migrations haven't been applied yet.
        pub async fn ensure_schema(&self) -> Result<(), StoreError> {
            self.db
                .exec(SCHEMA)
                .into_send()
                .await
                .map_err(|e| StoreError::new(format!("d1 exec schema: {e}")))?;
            Ok(())
        }
    }

    #[derive(Deserialize)]
    struct TodoRow {
        id: i64,
        text: String,
        // D1 stores bool as INTEGER 0/1.
        done: i64,
    }

    impl From<TodoRow> for TodoRecord {
        fn from(row: TodoRow) -> Self {
            TodoRecord {
                id: row.id,
                text: row.text,
                done: row.done != 0,
            }
        }
    }

    fn map_err(context: &'static str, err: worker::Error) -> StoreError {
        StoreError::new(format!("d1 {context}: {err}"))
    }

    // `D1Type::Integer` is `i32` in worker-rs 0.8, so route i64 ids through
    // `Real` to preserve JS Number's 53-bit integer range. SQLite's column
    // affinity stores whole-valued reals as INTEGER.
    fn id_arg<'a>(id: i64) -> D1Type<'a> {
        D1Type::Real(id as f64)
    }

    impl TodoStore for D1TodoStore {
        async fn create(&self, text: String) -> Result<TodoRecord, StoreError> {
            let row: TodoRow = self
                .db
                .prepare("INSERT INTO todos (text) VALUES (?) RETURNING id, text, done")
                .bind_refs(&[D1Type::Text(&text)])
                .map_err(|e| map_err("bind create", e))?
                .first(None)
                .into_send()
                .await
                .map_err(|e| map_err("create", e))?
                .ok_or_else(|| StoreError::new("create: no row returned"))?;
            Ok(row.into())
        }

        async fn get(&self, id: i64) -> Result<Option<TodoRecord>, StoreError> {
            let row: Option<TodoRow> = self
                .db
                .prepare("SELECT id, text, done FROM todos WHERE id = ?")
                .bind_refs(&[id_arg(id)])
                .map_err(|e| map_err("bind get", e))?
                .first(None)
                .into_send()
                .await
                .map_err(|e| map_err("get", e))?;
            Ok(row.map(Into::into))
        }

        async fn list(&self) -> Result<Vec<TodoRecord>, StoreError> {
            let rows: Vec<TodoRow> = self
                .db
                .prepare("SELECT id, text, done FROM todos ORDER BY id")
                .all()
                .into_send()
                .await
                .map_err(|e| map_err("list", e))?
                .results()
                .map_err(|e| map_err("list results", e))?;
            Ok(rows.into_iter().map(Into::into).collect())
        }

        async fn update(
            &self,
            id: i64,
            text: Option<String>,
            done: Option<bool>,
        ) -> Result<Option<TodoRecord>, StoreError> {
            // COALESCE lets a single statement handle any combination of
            // partial field updates — Null params pass through the existing
            // column value.
            let text_arg = text.as_deref().map_or(D1Type::Null, D1Type::Text);
            let done_arg = done.map_or(D1Type::Null, D1Type::Boolean);
            let row: Option<TodoRow> = self
                .db
                .prepare(
                    "UPDATE todos SET \
                        text = COALESCE(?, text), \
                        done = COALESCE(?, done) \
                     WHERE id = ? \
                     RETURNING id, text, done",
                )
                .bind_refs(&[text_arg, done_arg, id_arg(id)])
                .map_err(|e| map_err("bind update", e))?
                .first(None)
                .into_send()
                .await
                .map_err(|e| map_err("update", e))?;
            Ok(row.map(Into::into))
        }

        async fn delete(&self, id: i64) -> Result<bool, StoreError> {
            let result = self
                .db
                .prepare("DELETE FROM todos WHERE id = ?")
                .bind_refs(&[id_arg(id)])
                .map_err(|e| map_err("bind delete", e))?
                .run()
                .into_send()
                .await
                .map_err(|e| map_err("delete", e))?;
            let changes = result
                .meta()
                .map_err(|e| map_err("delete meta", e))?
                .and_then(|m| m.changes)
                .unwrap_or(0);
            Ok(changes > 0)
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub use d1::D1TodoStore;

pub struct TodoServer<S> {
    store: S,
}

impl<S> TodoServer<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }
}

fn not_found(id: i64) -> ConnectError {
    ConnectError::not_found(format!("todo {id} not found"))
}

impl<S: TodoStore> TodoService for TodoServer<S> {
    async fn create_todo(
        &self,
        _ctx: RequestContext,
        request: OwnedView<CreateTodoRequestView<'static>>,
    ) -> ServiceResult<CreateTodoResponse> {
        if request.text.is_empty() {
            return Err(ConnectError::invalid_argument("text must not be empty"));
        }
        let record = self.store.create(request.text.to_owned()).await?;
        Response::ok(CreateTodoResponse {
            todo: MessageField::some(record.into()),
            ..Default::default()
        })
    }

    async fn get_todo(
        &self,
        _ctx: RequestContext,
        request: OwnedView<GetTodoRequestView<'static>>,
    ) -> ServiceResult<GetTodoResponse> {
        let id = request.id;
        let record = self.store.get(id).await?.ok_or_else(|| not_found(id))?;
        Response::ok(GetTodoResponse {
            todo: MessageField::some(record.into()),
            ..Default::default()
        })
    }

    async fn list_todos(
        &self,
        _ctx: RequestContext,
        _request: OwnedView<ListTodosRequestView<'static>>,
    ) -> ServiceResult<ListTodosResponse> {
        let records = self.store.list().await?;
        Response::ok(ListTodosResponse {
            todos: records.into_iter().map(Into::into).collect(),
            ..Default::default()
        })
    }

    async fn update_todo(
        &self,
        _ctx: RequestContext,
        request: OwnedView<UpdateTodoRequestView<'static>>,
    ) -> ServiceResult<UpdateTodoResponse> {
        let id = request.id;
        let text = request.text.map(|t| t.to_owned());
        let done = request.done;
        let record = self
            .store
            .update(id, text, done)
            .await?
            .ok_or_else(|| not_found(id))?;
        Response::ok(UpdateTodoResponse {
            todo: MessageField::some(record.into()),
            ..Default::default()
        })
    }

    async fn delete_todo(
        &self,
        _ctx: RequestContext,
        request: OwnedView<DeleteTodoRequestView<'static>>,
    ) -> ServiceResult<DeleteTodoResponse> {
        let id = request.id;
        if !self.store.delete(id).await? {
            return Err(not_found(id));
        }
        Response::ok(DeleteTodoResponse::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::workers::todo::v1::{
        CreateTodoRequest, DeleteTodoRequest, GetTodoRequest, ListTodosRequest, UpdateTodoRequest,
    };
    use connectrpc::ErrorCode;
    use futures::executor::block_on;

    fn server() -> TodoServer<InMemoryTodoStore> {
        TodoServer::new(InMemoryTodoStore::new())
    }

    fn view<V>(msg: &V::Owned) -> OwnedView<V>
    where
        V: buffa::MessageView<'static>,
        V::Owned: buffa::Message,
    {
        OwnedView::<V>::from_owned(msg).expect("build request view")
    }

    async fn create(srv: &TodoServer<InMemoryTodoStore>, text: &str) -> Todo {
        let req = CreateTodoRequest {
            text: text.into(),
            ..Default::default()
        };
        let resp: CreateTodoResponse = srv
            .create_todo(RequestContext::default(), view(&req))
            .await
            .expect("create_todo")
            .body
            .into();
        resp.todo.into_option().expect("todo present")
    }

    #[test]
    fn create_assigns_monotonic_ids_and_defaults_done_false() {
        let srv = server();
        block_on(async {
            let a = create(&srv, "buy milk").await;
            let b = create(&srv, "walk dog").await;
            assert_eq!(a.id, 1);
            assert_eq!(b.id, 2);
            assert_eq!(a.text, "buy milk");
            assert!(!a.done);
            assert!(!b.done);
        });
    }

    #[test]
    fn create_rejects_empty_text() {
        let srv = server();
        block_on(async {
            let req = CreateTodoRequest::default();
            let err = srv
                .create_todo(RequestContext::default(), view(&req))
                .await
                .expect_err("empty text must be rejected");
            assert_eq!(err.code, ErrorCode::InvalidArgument);
        });
    }

    #[test]
    fn get_returns_existing_todo() {
        let srv = server();
        block_on(async {
            let created = create(&srv, "read book").await;
            let req = GetTodoRequest {
                id: created.id,
                ..Default::default()
            };
            let resp: GetTodoResponse = srv
                .get_todo(RequestContext::default(), view(&req))
                .await
                .unwrap()
                .body
                .into();
            let fetched = resp.todo.into_option().unwrap();
            assert_eq!(fetched.id, created.id);
            assert_eq!(fetched.text, "read book");
        });
    }

    #[test]
    fn get_missing_returns_not_found() {
        let srv = server();
        block_on(async {
            let req = GetTodoRequest {
                id: 999,
                ..Default::default()
            };
            let err = srv
                .get_todo(RequestContext::default(), view(&req))
                .await
                .expect_err("must not find todo 999");
            assert_eq!(err.code, ErrorCode::NotFound);
        });
    }

    #[test]
    fn list_returns_todos_in_id_order() {
        let srv = server();
        block_on(async {
            create(&srv, "one").await;
            create(&srv, "two").await;
            create(&srv, "three").await;
            let req = ListTodosRequest::default();
            let resp: ListTodosResponse = srv
                .list_todos(RequestContext::default(), view(&req))
                .await
                .unwrap()
                .body
                .into();
            let ids: Vec<i64> = resp.todos.iter().map(|t| t.id).collect();
            let texts: Vec<&str> = resp.todos.iter().map(|t| t.text.as_str()).collect();
            assert_eq!(ids, vec![1, 2, 3]);
            assert_eq!(texts, vec!["one", "two", "three"]);
        });
    }

    #[test]
    fn list_on_empty_store_returns_empty() {
        let srv = server();
        block_on(async {
            let req = ListTodosRequest::default();
            let resp: ListTodosResponse = srv
                .list_todos(RequestContext::default(), view(&req))
                .await
                .unwrap()
                .body
                .into();
            assert!(resp.todos.is_empty());
        });
    }

    #[test]
    fn update_can_change_text_only() {
        let srv = server();
        block_on(async {
            let created = create(&srv, "old").await;
            let req = UpdateTodoRequest {
                id: created.id,
                text: Some("new".into()),
                done: None,
                ..Default::default()
            };
            let resp: UpdateTodoResponse = srv
                .update_todo(RequestContext::default(), view(&req))
                .await
                .unwrap()
                .body
                .into();
            let updated = resp.todo.into_option().unwrap();
            assert_eq!(updated.text, "new");
            assert!(!updated.done);
        });
    }

    #[test]
    fn update_can_toggle_done_only() {
        let srv = server();
        block_on(async {
            let created = create(&srv, "task").await;
            let req = UpdateTodoRequest {
                id: created.id,
                text: None,
                done: Some(true),
                ..Default::default()
            };
            let resp: UpdateTodoResponse = srv
                .update_todo(RequestContext::default(), view(&req))
                .await
                .unwrap()
                .body
                .into();
            let updated = resp.todo.into_option().unwrap();
            assert_eq!(updated.text, "task");
            assert!(updated.done);
        });
    }

    #[test]
    fn update_missing_returns_not_found() {
        let srv = server();
        block_on(async {
            let req = UpdateTodoRequest {
                id: 42,
                text: Some("x".into()),
                ..Default::default()
            };
            let err = srv
                .update_todo(RequestContext::default(), view(&req))
                .await
                .expect_err("must not update missing todo");
            assert_eq!(err.code, ErrorCode::NotFound);
        });
    }

    #[test]
    fn delete_removes_todo() {
        let srv = server();
        block_on(async {
            let created = create(&srv, "doomed").await;
            let req = DeleteTodoRequest {
                id: created.id,
                ..Default::default()
            };
            srv.delete_todo(RequestContext::default(), view(&req))
                .await
                .expect("delete");
            let get_req = GetTodoRequest {
                id: created.id,
                ..Default::default()
            };
            let err = srv
                .get_todo(RequestContext::default(), view(&get_req))
                .await
                .expect_err("get must fail after delete");
            assert_eq!(err.code, ErrorCode::NotFound);
        });
    }

    #[test]
    fn delete_missing_returns_not_found() {
        let srv = server();
        block_on(async {
            let req = DeleteTodoRequest {
                id: 7,
                ..Default::default()
            };
            let err = srv
                .delete_todo(RequestContext::default(), view(&req))
                .await
                .expect_err("must not delete missing todo");
            assert_eq!(err.code, ErrorCode::NotFound);
        });
    }
}
