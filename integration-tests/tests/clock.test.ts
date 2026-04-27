import { describe, it, expect } from "vitest";
import { ConnectError, Code } from "@connectrpc/connect";
import { clockClient } from "./helpers";

async function collectTicks(
  stream: AsyncIterable<{ sequence: number }>,
): Promise<number[]> {
  const seqs: number[] = [];
  for await (const msg of stream) {
    seqs.push(msg.sequence);
  }
  return seqs;
}

describe("ClockService.Tick (server-streaming)", () => {
  it("emits the requested number of sequential ticks", async () => {
    const client = clockClient();
    const seqs = await collectTicks(client.tick({ count: 5 }));
    expect(seqs).toEqual([0, 1, 2, 3, 4]);
  });

  it("closes immediately when count is zero", async () => {
    const client = clockClient();
    const seqs = await collectTicks(client.tick({ count: 0 }));
    expect(seqs).toEqual([]);
  });

  it("works over the JSON codec too", async () => {
    const client = clockClient({ useBinaryFormat: false });
    const seqs = await collectTicks(client.tick({ count: 3 }));
    expect(seqs).toEqual([0, 1, 2]);
  });

  it("rejects counts above the server limit", async () => {
    const client = clockClient();
    const err = await collectTicks(client.tick({ count: 5000 })).then(
      () => {
        throw new Error("expected rejection");
      },
      (e) => e,
    );
    expect(err).toBeInstanceOf(ConnectError);
    expect(ConnectError.from(err).code).toBe(Code.InvalidArgument);
  });
});
