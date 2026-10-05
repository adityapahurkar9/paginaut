// Android's back button: the WebView goes back in history when it can, so everything shown over
// the library (the reader, a sheet, search) adds a history entry, and going back closes the top one.

const stack: (() => void)[] = [];

/** Shows a layer; `close` runs when it's taken off (back button, or `back()`). */
export function pushLayer(close: () => void) {
  stack.push(close);
  history.pushState({ depth: stack.length }, "");
}

/** Closes the top layer as the back button would. */
export function back() {
  if (stack.length) history.back();
}

/** Layers open now. */
export const depth = () => stack.length;

window.addEventListener("popstate", () => {
  stack.pop()?.();
});
