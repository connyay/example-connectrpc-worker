import { describe, it, expect } from "vitest";
import { heartbeatClient } from "./helpers";
import { makeInputQueue } from "./bidi-helpers";

describe("HeartbeatService.Heartbeat (bidi full-duplex)", () => {
  it("emits the initial pong before the client sends anything", async () => {
    const client = heartbeatClient();
    const inputs = makeInputQueue<{ note: string }>();

    const stream = client.heartbeat(inputs.iter());
    const iter = stream[Symbol.asyncIterator]();

    const initial = await iter.next();
    expect(initial.done).toBe(false);
    expect(initial.value).toMatchObject({ sequence: 0, note: "open" });

    inputs.close();
    const close = await iter.next();
    expect(close.value).toMatchObject({ sequence: 1, note: "close" });
    const end = await iter.next();
    expect(end.done).toBe(true);
  });

  it("returns each input's echo before the client closes its send half", async () => {
    const client = heartbeatClient();
    const inputs = makeInputQueue<{ note: string }>();

    const stream = client.heartbeat(inputs.iter());
    const iter = stream[Symbol.asyncIterator]();

    const initial = await iter.next();
    expect(initial.value).toMatchObject({ sequence: 0, note: "open" });

    for (const [i, note] of ["one", "two", "three"].entries()) {
      inputs.push({ note });
      const echo = await iter.next();
      expect(echo.done).toBe(false);
      expect(echo.value).toMatchObject({ sequence: i + 1, note });
    }

    inputs.close();
    const close = await iter.next();
    expect(close.value).toMatchObject({ sequence: 4, note: "close" });
    const end = await iter.next();
    expect(end.done).toBe(true);
  });
});
