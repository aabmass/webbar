import { createSubscriber } from "svelte/reactivity";

/**
 * Wraps a `window.webbar.*` external store (`subscribe` + `get`) as a reactive
 * Svelte 5 getter. Subscribes lazily when read inside a reactive context and
 * unsubscribes automatically when no longer tracked.
 */
export function fromExternalStore<T>(
  subscribe: (listener: () => void) => () => void,
  get: () => T,
): { readonly current: T } {
  const track = createSubscriber((update) => subscribe(update));
  return {
    get current() {
      track();
      return get();
    },
  };
}
