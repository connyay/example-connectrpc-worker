import { Miniflare } from "miniflare";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const __dirname = dirname(fileURLToPath(import.meta.url));
const buildDir = resolve(__dirname, "..", "..", "build");

const jsCode = readFileSync(resolve(buildDir, "index.js"), "utf-8");
const wasmBytes = readFileSync(resolve(buildDir, "index_bg.wasm"));

export const mf = new Miniflare({
  workers: [
    {
      name: "workers-connectrpc",
      modules: [
        { type: "ESModule", path: "index.js", contents: jsCode },
        {
          type: "CompiledWasm",
          path: "index_bg.wasm",
          contents: wasmBytes,
        },
      ],
      compatibilityDate: "2026-04-22",
      d1Databases: ["DB"],
    },
  ],
});

export const mfUrl = (await mf.ready).toString().replace(/\/$/, "");
