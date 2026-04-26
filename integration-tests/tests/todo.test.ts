import { describe, it, expect } from "vitest";
import { ConnectError, Code } from "@connectrpc/connect";
import { todoClient } from "./helpers";

// Tests in this file share a single miniflare D1 instance, so each test
// creates its own todos and checks by id rather than by global list state.

describe("TodoService", () => {
  it("create → get round-trip", async () => {
    const client = todoClient();
    const created = await client.createTodo({ text: "buy milk" });
    expect(created.todo).toBeDefined();
    expect(created.todo!.text).toBe("buy milk");
    expect(created.todo!.done).toBe(false);
    expect(typeof created.todo!.id).toBe("bigint");

    const got = await client.getTodo({ id: created.todo!.id });
    expect(got.todo?.text).toBe("buy milk");
    expect(got.todo?.id).toBe(created.todo!.id);
  });

  it("update sets text and done independently", async () => {
    const client = todoClient();
    const { todo } = await client.createTodo({ text: "draft" });
    const id = todo!.id;

    const renamed = await client.updateTodo({ id, text: "final" });
    expect(renamed.todo?.text).toBe("final");
    expect(renamed.todo?.done).toBe(false);

    const completed = await client.updateTodo({ id, done: true });
    expect(completed.todo?.text).toBe("final");
    expect(completed.todo?.done).toBe(true);
  });

  it("list contains created items", async () => {
    const client = todoClient();
    const [a, b] = await Promise.all([
      client.createTodo({ text: "list-item-a" }),
      client.createTodo({ text: "list-item-b" }),
    ]);

    const { todos } = await client.listTodos({});
    const texts = todos.map((t) => t.text);
    expect(texts).toContain("list-item-a");
    expect(texts).toContain("list-item-b");
    const ids = todos.map((t) => t.id);
    expect(ids).toContain(a.todo!.id);
    expect(ids).toContain(b.todo!.id);
  });

  it("delete removes the todo", async () => {
    const client = todoClient();
    const { todo } = await client.createTodo({ text: "to delete" });
    const id = todo!.id;

    await client.deleteTodo({ id });

    await expect(client.getTodo({ id })).rejects.toBeInstanceOf(ConnectError);
  });

  it("createTodo rejects empty text with InvalidArgument", async () => {
    const client = todoClient();
    await expect(client.createTodo({ text: "" })).rejects.toMatchObject({
      name: "ConnectError",
      code: Code.InvalidArgument,
    });
  });

  it("getTodo for unknown id returns NotFound", async () => {
    const client = todoClient();
    await expect(
      client.getTodo({ id: 999_999_999n }),
    ).rejects.toMatchObject({
      name: "ConnectError",
      code: Code.NotFound,
    });
  });

  it("works over the JSON codec (int64 is wire-encoded as string)", async () => {
    const client = todoClient({ useBinaryFormat: false });
    const { todo } = await client.createTodo({ text: "json-codec" });
    expect(todo?.text).toBe("json-codec");
    expect(typeof todo?.id).toBe("bigint");

    const got = await client.getTodo({ id: todo!.id });
    expect(got.todo?.text).toBe("json-codec");
  });
});
