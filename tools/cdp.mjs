// Dev helper: drives a running Paginaut window over the Chrome DevTools Protocol.
// Start Paginaut with WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333, then:
//   node tools/cdp.mjs eval "<js expression>"   evaluate (awaits promises), print JSON result
//   node tools/cdp.mjs shot out.png             screenshot of the window contents
//   node tools/cdp.mjs logs [ms]                print console messages/errors for a while
//   node tools/cdp.mjs click x y [right]        a real mouse click (counts as a user gesture)
//   node tools/cdp.mjs drag "x0 y0" "x1 y1"     a real left-button drag
//   node tools/cdp.mjs key <Key>                a real key press (e.g. Escape)
// CDP_EDGE=1 drives an Edge dialog WebView2 opened (e.g. a permission prompt) instead of the app.
// CDP_WINDOW=<label> drives that Paginaut window ("main", "w-1"…; the first one found without it).
//   node tools/cdp.mjs windows                  lists the windows' labels
const port = process.env.CDP_PORT || 9333;
const [cmd, arg, arg2, arg3] = process.argv.slice(2);

const targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
const edge = (t) => t.url.startsWith("edge://");
const pages = targets.filter((t) => t.type === "page" && (process.env.CDP_EDGE ? edge(t) : !edge(t)));
if (!pages.length) throw new Error("no page target");
/** The window label a page belongs to. */
async function labelOf(t) {
  const w = new WebSocket(t.webSocketDebuggerUrl);
  await new Promise((r) => w.addEventListener("open", r));
  const v = await new Promise((r) => {
    w.addEventListener("message", (m) => r(JSON.parse(m.data).result?.result?.value));
    w.send(JSON.stringify({ id: 1, method: "Runtime.evaluate", params: { expression: "window.__TAURI_INTERNALS__?.metadata?.currentWindow?.label", returnByValue: true } }));
  });
  await new Promise((r) => {
    w.addEventListener("close", r);
    w.close();
  });
  return v;
}
if (cmd === "windows") for (const t of pages) console.log(await labelOf(t));
else await drive();

async function drive() {
  let page = pages[0];
  if (process.env.CDP_WINDOW && !process.env.CDP_EDGE) {
    page = null;
    for (const t of pages) if ((await labelOf(t)) === process.env.CDP_WINDOW) page = t;
    if (!page) throw new Error(`no window ${process.env.CDP_WINDOW}`);
  }
  const ws = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((r) => ws.addEventListener("open", r));
  let seq = 0;
  const pending = new Map();
  const events = [];
  ws.addEventListener("message", (m) => {
    const msg = JSON.parse(m.data);
    if (msg.id && pending.has(msg.id)) {
      pending.get(msg.id)(msg);
      pending.delete(msg.id);
    } else if (msg.method) events.push(msg);
  });
  const send = (method, params = {}) =>
    new Promise((resolve, reject) => {
      const id = ++seq;
      pending.set(id, (msg) => (msg.error ? reject(new Error(msg.error.message)) : resolve(msg.result)));
      ws.send(JSON.stringify({ id, method, params }));
    });

  if (cmd === "eval") {
    const r = await send("Runtime.evaluate", { expression: arg, awaitPromise: true, returnByValue: true });
    if (r.exceptionDetails) console.log("EXCEPTION", r.exceptionDetails.exception?.description ?? r.exceptionDetails.text);
    else console.log(JSON.stringify(r.result.value, null, 1));
  } else if (cmd === "shot") {
    const r = await send("Page.captureScreenshot", { format: "png" });
    (await import("node:fs")).writeFileSync(arg || "shot.png", Buffer.from(r.data, "base64"));
    console.log("saved", arg || "shot.png");
  } else if (cmd === "click") {
    const [x, y, button] = [Number(arg), Number(arg2), arg3 === "right" ? "right" : "left"];
    await send("Input.dispatchMouseEvent", { type: "mouseMoved", x, y });
    await send("Input.dispatchMouseEvent", { type: "mousePressed", x, y, button, clickCount: 1 });
    await send("Input.dispatchMouseEvent", { type: "mouseReleased", x, y, button, clickCount: 1 });
    console.log("clicked", x, y, button);
  } else if (cmd === "drag") {
    // drag "x0 y0" "x1 y1"
    const [x0, y0] = arg.split(" ").map(Number);
    const [x1, y1] = arg2.split(" ").map(Number);
    await send("Input.dispatchMouseEvent", { type: "mouseMoved", x: x0, y: y0 });
    await send("Input.dispatchMouseEvent", { type: "mousePressed", x: x0, y: y0, button: "left", buttons: 1, clickCount: 1 });
    for (let i = 1; i <= 8; i++)
      await send("Input.dispatchMouseEvent", { type: "mouseMoved", x: x0 + ((x1 - x0) * i) / 8, y: y0 + ((y1 - y0) * i) / 8, button: "left", buttons: 1 });
    await send("Input.dispatchMouseEvent", { type: "mouseReleased", x: x1, y: y1, button: "left", buttons: 0, clickCount: 1 });
    console.log("dragged", x0, y0, "to", x1, y1);
  } else if (cmd === "key") {
    await send("Input.dispatchKeyEvent", { type: "keyDown", key: arg, code: arg, windowsVirtualKeyCode: arg === "Escape" ? 27 : arg === "Enter" ? 13 : 0 });
    await send("Input.dispatchKeyEvent", { type: "keyUp", key: arg, code: arg, windowsVirtualKeyCode: arg === "Escape" ? 27 : arg === "Enter" ? 13 : 0 });
    console.log("pressed", arg);
  } else if (cmd === "logs") {
    await send("Runtime.enable");
    await send("Log.enable");
    await new Promise((r) => setTimeout(r, Number(arg) || 1500));
    for (const e of events) {
      if (e.method === "Runtime.consoleAPICalled") console.log(e.params.type, e.params.args.map((a) => a.value ?? a.description).join(" "));
      if (e.method === "Runtime.exceptionThrown") console.log("EXCEPTION", e.params.exceptionDetails.exception?.description);
      if (e.method === "Log.entryAdded") console.log(e.params.entry.level, e.params.entry.text, e.params.entry.url ?? "");
    }
  }
  ws.close();
}
