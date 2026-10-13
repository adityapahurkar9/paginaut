// Times the Android engine from inside the app's page (run with tools/cdp.mjs against the WebView):
// opening a document, drawing pages at phone size, page text and a search. Used to compare the engine
// in the app's own process with the engine in its isolated process (engine.rs).
//
//   CDP_PORT=9334 node tools/cdp.mjs eval "$(cat tools/android-bench.js)"
//
// Opens the first e-book or PDF Recent/the shelf can reach again; pass window.benchUri to choose.
(async () => {
  const I = window.__TAURI_INTERNALS__.invoke;
  const ms = () => performance.now();
  const sc = await I("scan_books").catch(() => ({ items: [] }));
  const uri = window.benchUri || sc.items[0]?.id;
  if (!uri) return "nothing to open";
  const p = await I("reopen", { uri });
  const out = { file: p.name };
  let t = ms();
  const d = await I("open_document", { path: p.path });
  out.open = Math.round(ms() - t);
  out.pages = d.pageCount;
  const n = Math.min(d.pageCount, 12);
  const sizes = await I("page_sizes", { doc: d.docId });
  // A phone-width page: 1080 px across.
  const draw = async (page) => {
    const s = 1080 / sizes[page][0];
    const r = await fetch(`http://paginaut.localhost/render?d=${d.docId}&p=${page}&s=${s}&r=0&e=1&id=${page + 1}&q=0`);
    return (await r.arrayBuffer()).byteLength;
  };
  t = ms();
  let bytes = 0;
  for (let i = 0; i < n; i++) bytes += await draw(i);
  out.drawEach = Math.round((ms() - t) / n);
  out.mbPerPage = +(bytes / n / 1048576).toFixed(1);
  t = ms();
  await Promise.all(Array.from({ length: n }, (_, i) => draw(i)));
  out.drawParallel = Math.round((ms() - t) / n);
  t = ms();
  for (let i = 0; i < n; i++) await I("page_text", { doc: d.docId, page: i });
  out.textEach = Math.round((ms() - t) / n);
  t = ms();
  await new Promise((done) => {
    // A Tauri channel by hand (the page's own code imports Channel from @tauri-apps/api).
    const id = window.__TAURI_INTERNALS__.transformCallback((m) => {
      const e = m && m.message !== undefined ? m.message : m;
      if (e && e.kind === "done") done();
    });
    I("search", { doc: d.docId, needle: "the", opts: null, sid: 991, onEvent: `__CHANNEL__:${id}` }).catch(done);
  });
  out.search = Math.round(ms() - t);
  await I("close_document", { doc: d.docId });
  if (p.inPlace) await I("release", { path: p.path });
  return JSON.stringify(out);
})()
