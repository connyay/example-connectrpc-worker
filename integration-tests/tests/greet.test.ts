import { describe, it, expect } from "vitest";
import { greetClient } from "./helpers";

describe("GreetService", () => {
  it("greets a named user", async () => {
    const client = greetClient();
    const res = await client.greet({ name: "Ada" });
    expect(res.greeting).toBe("Hello, Ada!");
  });

  it("falls back to 'world' when name is empty", async () => {
    const client = greetClient();
    const res = await client.greet({ name: "" });
    expect(res.greeting).toBe("Hello, world!");
  });

  it("preserves multibyte unicode names", async () => {
    const client = greetClient();
    const res = await client.greet({ name: "世界" });
    expect(res.greeting).toBe("Hello, 世界!");
  });

  it("works over the JSON codec too", async () => {
    const client = greetClient({ useBinaryFormat: false });
    const res = await client.greet({ name: "Grace" });
    expect(res.greeting).toBe("Hello, Grace!");
  });

  it("echoes a caller-supplied x-request-id back as a trailer", async () => {
    const client = greetClient({
      defaultHeaders: { "x-request-id": "trace-from-client" },
    });
    let trailer: Headers | undefined;
    const res = await client.greet(
      { name: "Ada" },
      { onTrailer: (t) => (trailer = t) },
    );
    expect(res.greeting).toBe("Hello, Ada!");
    expect(trailer?.get("x-request-id")).toBe("trace-from-client");
  });

  it("synthesizes an x-request-id trailer when the caller omits one", async () => {
    const client = greetClient();
    let trailer: Headers | undefined;
    await client.greet({ name: "Ada" }, { onTrailer: (t) => (trailer = t) });
    expect(trailer?.get("x-request-id")).toMatch(/^req-\d+$/);
  });
});
