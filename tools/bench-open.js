// Evaluated in the app: open a file and time the first preview / first sharp page.
(async (path) => {
  const f = window.folio;
  const t0 = performance.now();
  f.openFiles([path]);
  const until = async (fn, ms = 20000) => { const s = performance.now(); while (!fn()) { if (performance.now() - s > ms) return false; await new Promise(r => setTimeout(r, 3)); } return true; };
  await until(() => f.activeTab.value?.viewer);
  const v = f.activeTab.value.viewer; const ready = performance.now() - t0;
  await until(() => v.stats.firstHi);
  const c = v.stats.created - t0;
  return { viewerReadyMs: Math.round(ready), firstPreviewMs: Math.round(c + v.stats.firstLo), firstSharpMs: Math.round(c + v.stats.firstHi), pages: v.pageCount };
})
