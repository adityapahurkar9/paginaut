// Dev helper: drives a running Folio window over the Chrome DevTools Protocol.
// Start Folio with WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333, then:
//   node tools/cdp.mjs eval "<js expression>"   evaluate (awaits promises), print JSON result
//   node tools/cdp.mjs shot out.png             screenshot of the window contents
//   node tools/cdp.mjs logs [ms]                print console messages/errors for a while
const port = process.env.CDP_PORT || 9333;
const [cmd, arg] = process.argv.slice(2);

const targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
const page = targets.find((t) => t.type === "page");
if (!page) throw new Error("no page target");
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
