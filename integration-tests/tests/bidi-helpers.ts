// Async iterable input queue for bidi RPCs — push-driven so tests can
// interleave sends with awaited responses.
export function makeInputQueue<T>() {
  type Yielded =
    | { value: T; done: false }
    | { value: undefined; done: true };

  const items: T[] = [];
  const waiters: Array<(v: Yielded) => void> = [];
  let closed = false;

  return {
    push(item: T) {
      const w = waiters.shift();
      if (w) w({ value: item, done: false });
      else items.push(item);
    },
    close() {
      closed = true;
      while (waiters.length > 0) {
        waiters.shift()!({ value: undefined, done: true });
      }
    },
    async *iter(): AsyncIterableIterator<T> {
      while (true) {
        if (items.length > 0) {
          yield items.shift()!;
          continue;
        }
        if (closed) return;
        const next = await new Promise<Yielded>((resolve) =>
          waiters.push(resolve),
        );
        if (next.done) return;
        yield next.value;
      }
    },
  };
}
