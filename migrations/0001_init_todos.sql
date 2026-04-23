-- D1 migration: create the todos table.
-- The Worker also runs `CREATE TABLE IF NOT EXISTS` on startup via
-- `D1TodoStore::ensure_schema` so local `wrangler dev` works before this
-- migration is applied. In managed environments apply with
-- `wrangler d1 migrations apply workers-connectrpc-todos`.

CREATE TABLE IF NOT EXISTS todos (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  text TEXT NOT NULL,
  done INTEGER NOT NULL DEFAULT 0
);
