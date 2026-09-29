(async (mode) => {
  // Draw 60 fresh 1000x1300 bitmaps into one visible canvas, like pages arriving.
  const c = document.createElement('canvas'); c.style.cssText = 'position:fixed;left:0;top:0;width:500px;height:650px;z-index:999';
  document.body.appendChild(c);
  const ctx = mode === 'transfer' ? c.getContext('bitmaprenderer') : c.getContext('2d');
  for (let i = 0; i < 60; i++) {
    const d = new ImageData(1000, 1300); d.data.fill(128 + (i % 100));
    if (mode === 'put') { c.width = 1000; c.height = 1300; ctx.putImageData(d, 0, 0); await new Promise(r => requestAnimationFrame(r)); continue; }
    const b = await createImageBitmap(d);
    if (mode === 'transfer') ctx.transferFromImageBitmap(b); else { c.width = 1000; c.height = 1300; ctx.drawImage(b, 0, 0); b.close(); }
    await new Promise(r => requestAnimationFrame(r));
  }
  await new Promise(r => setTimeout(r, 1500));
  c.remove();
  return mode + ' done';
})
