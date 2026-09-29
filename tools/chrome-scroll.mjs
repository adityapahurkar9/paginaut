// Scrolls Chrome's PDF viewer (DevTools on port 9444) with mouse-wheel events, like read-scrolling.
const targets = await (await fetch("http://127.0.0.1:9444/json")).json();
const page = targets.find((t) => t.type === "page");
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((r) => ws.addEventListener("open", r));
let id = 0;
const send = (method, params) => new Promise((res) => {
  const my = ++id;
  const on = (m) => { const d = JSON.parse(m.data); if (d.id === my) { ws.removeEventListener("message", on); res(d); } };
  ws.addEventListener("message", on);
  ws.send(JSON.stringify({ id: my, method, params }));
});
await send("Page.bringToFront", {});
for (let i = 0; i < 240; i++) {
  await send("Input.dispatchMouseEvent", { type: "mouseWheel", x: 640, y: 450, deltaX: 0, deltaY: 100 });
  await new Promise((r) => setTimeout(r, 40));
}
console.log("scrolled");
ws.close();
