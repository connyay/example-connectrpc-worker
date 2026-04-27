import { describe, it, expect } from "vitest";
import { echoClient } from "./helpers";
import { makeInputQueue } from "./bidi-helpers";

describe("EchoService.Echo (bidi-streaming)", () => {
  it("echoes each request with a monotonically increasing sequence", async () => {
    const client = echoClient();
    const inputs = makeInputQueue<{ text: string }>();
    inputs.push({ text: "hello" });
    inputs.push({ text: "world" });
    inputs.push({ text: "!" });
    inputs.close();

    const out: { text: string; sequence: number }[] = [];
    for await (const msg of client.echo(inputs.iter())) {
      out.push({ text: msg.text, sequence: msg.sequence });
    }
    expect(out).toEqual([
      { text: "hello", sequence: 0 },
      { text: "world", sequence: 1 },
      { text: "!", sequence: 2 },
    ]);
  });

  it("closes cleanly when the client sends no messages", async () => {
    const client = echoClient();
    const inputs = makeInputQueue<{ text: string }>();
    inputs.close();
    const out: unknown[] = [];
    for await (const msg of client.echo(inputs.iter())) {
      out.push(msg);
    }
    expect(out).toEqual([]);
  });
});
