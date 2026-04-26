import { describe, it, expect } from "vitest";
import { mf, mfUrl } from "./mf";

describe("non-RPC routes", () => {
  it("GET /healthz returns 200 + plaintext body", async () => {
    const res = await mf.dispatchFetch(`${mfUrl}/healthz`);
    expect(res.status).toBe(200);
    expect(await res.text()).toMatch(/ok/i);
  });

  it("GET /oauth/callback echoes code + state from query string", async () => {
    const res = await mf.dispatchFetch(
      `${mfUrl}/oauth/callback?code=abc123&state=xyz`,
    );
    expect(res.status).toBe(200);
    const body = await res.text();
    expect(body).toContain("abc123");
    expect(body).toContain("xyz");
  });

  it("unknown path falls through to RPC dispatcher and is rejected", async () => {
    // ConnectRpcService doesn't expose a strict status code for unknown paths
    // (could be 404 / 405 / 415 depending on shape). Just assert it didn't 200.
    const res = await mf.dispatchFetch(`${mfUrl}/does-not-exist`);
    expect(res.status).toBeGreaterThanOrEqual(400);
  });
});
