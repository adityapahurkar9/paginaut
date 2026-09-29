// Evaluated in the app via tools/cdp.mjs: scroll test at a given speed (px per frame).
(async (step, frames) => {
  const v = window.paginaut.activeTab.value.viewer; const el = v.el;
  el.scrollTop = 0; await new Promise(r => setTimeout(r, 400));
  const times = []; let last = performance.now(); let blank = 0, lowOnly = 0, checks = 0;
  for (let i = 0; i < frames; i++) {
    await new Promise(r => requestAnimationFrame(r));
    const now = performance.now(); times.push(now - last); last = now;
    el.scrollTop += step;
    for (const p of el.querySelectorAll('.pg')) {
      const r = p.getBoundingClientRect(); if (r.bottom <= 0 || r.top >= innerHeight) continue;
      checks++;
      const lo = p.querySelector('.pg-lo').style.visibility === 'visible', hi = p.querySelector('.pg-hi').style.visibility === 'visible';
      if (!lo && !hi) blank++; else if (!hi) lowOnly++;
    }
  }
  times.shift(); times.sort((a, b) => a - b);
  const q = x => times[Math.floor(times.length * x)].toFixed(1);
  const pageH = el.scrollHeight / v.pageCount;
  return { speedPagesPerSec: (step * 60 / pageH).toFixed(1), frameMedianMs: q(.5), frameP99Ms: q(.99), blankPct: (100 * blank / checks).toFixed(1), previewOnlyPct: (100 * lowOnly / checks).toFixed(1) };
})
