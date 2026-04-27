import { createClient, type Client, type Transport } from "@connectrpc/connect";
import { createConnectTransport } from "@connectrpc/connect-web";
import { createConnectTransport as createNodeConnectTransport } from "@connectrpc/connect-node";
import type { DescService } from "@bufbuild/protobuf";

import { mf, mfUrl } from "./mf";

import { ClockService } from "../gen/workers/clock/v1/clock_pb.js";
import { EchoService } from "../gen/workers/echo/v1/echo_pb.js";
import { GreetService } from "../gen/workers/greet/v1/greet_pb.js";
import { HeartbeatService } from "../gen/workers/heartbeat/v1/heartbeat_pb.js";
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

export const clockClient = (opts?: TransportOptions) =>
  clientFor(ClockService, opts);

// connect-node speaks streaming uploads; connect-web / fetch can't on HTTP/1.1.
export function makeNodeTransport(opts: TransportOptions = {}): Transport {
  return createNodeConnectTransport({
    httpVersion: "1.1",
    baseUrl: mfUrl,
    useBinaryFormat: opts.useBinaryFormat ?? true,
  });
}

export const echoClient = (opts?: TransportOptions) =>
  createClient(EchoService, makeNodeTransport(opts));
export const heartbeatClient = (opts?: TransportOptions) =>
  createClient(HeartbeatService, makeNodeTransport(opts));
export const greetClient = (opts?: TransportOptions) =>
  clientFor(GreetService, opts);
export const reverseClient = (opts?: TransportOptions) =>
  clientFor(ReverseService, opts);
export const todoClient = (opts?: TransportOptions) =>
  clientFor(TodoService, opts);
