import { describe, it, expect } from "vitest";
import { reverseClient } from "./helpers";

describe("ReverseService", () => {
  it("reverses ASCII", async () => {
    const res = await reverseClient().reverse({ text: "hello" });
    expect(res.reversed).toBe("olleh");
  });

  it("reverses multibyte codepoints", async () => {
    const res = await reverseClient().reverse({ text: "héllo" });
    expect(res.reversed).toBe("olléh");
  });

  it("is involutive on ASCII", async () => {
    const client = reverseClient();
    const once = (await client.reverse({ text: "ConnectRPC" })).reversed;
    const twice = (await client.reverse({ text: once })).reversed;
    expect(twice).toBe("ConnectRPC");
  });

  it("returns empty for empty input", async () => {
    const res = await reverseClient().reverse({ text: "" });
    expect(res.reversed).toBe("");
  });
});
