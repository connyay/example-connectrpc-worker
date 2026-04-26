import { createClient, type Client, type Transport } from "@connectrpc/connect";
import { createConnectTransport } from "@connectrpc/connect-web";
import type { DescService } from "@bufbuild/protobuf";

import { mf, mfUrl } from "./mf";

import { GreetService } from "../gen/workers/greet/v1/greet_pb.js";
import { ReverseService } from "../gen/workers/reverse/v1/reverse_pb.js";
import { TodoService } from "../gen/workers/todo/v1/todo_pb.js";

export interface TransportOptions {
  /** false → JSON codec, true (default) → binary protobuf. */
  useBinaryFormat?: boolean;
  /** Extra headers attached to every request from this transport. */
  defaultHeaders?: Record<string, string>;
}

export function makeTransport(opts: TransportOptions = {}): Transport {
  // miniflare's `dispatchFetch` is signature-compatible with `globalThis.fetch`.
  return createConnectTransport({
    baseUrl: mfUrl,
    useBinaryFormat: opts.useBinaryFormat ?? true,
    fetch: ((input, init) => {
      const headers = new Headers(init?.headers);
      for (const [k, v] of Object.entries(opts.defaultHeaders ?? {})) {
        if (!headers.has(k)) headers.set(k, v);
      }
      return mf.dispatchFetch(input as string, { ...init, headers });
    }) as typeof globalThis.fetch,
  });
}

function clientFor<S extends DescService>(
  service: S,
  opts?: TransportOptions,
): Client<S> {
  return createClient(service, makeTransport(opts));
}

export const greetClient = (opts?: TransportOptions) =>
  clientFor(GreetService, opts);
export const reverseClient = (opts?: TransportOptions) =>
  clientFor(ReverseService, opts);
export const todoClient = (opts?: TransportOptions) =>
  clientFor(TodoService, opts);
