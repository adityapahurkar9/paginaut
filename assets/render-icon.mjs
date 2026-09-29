// Renders the app icon (1024x1024 PNG) from vector drawing commands.
import * as mupdf from "mupdf";
import fs from "node:fs";
const content = `
1 0 0 -1 0 1024 cm
0.169 0.345 0.820 rg
232 64 m 632 64 l 856 288 l 856 896 l 856 931 827 960 792 960 c 232 960 l 197 960 168 931 168 896 c 168 128 l 168 93 197 64 232 64 c h f
0.541 0.651 1 rg
632 64 m 632 224 l 632 259 661 288 696 288 c 856 288 l h f
1 1 1 RG 72 w 1 J
336 560 m 688 560 l S
336 704 m 576 704 l S
`;
const doc = new mupdf.PDFDocument();
doc.insertPage(-1, doc.addPage([0, 0, 1024, 1024], 0, {}, content));
const pix = doc.loadPage(0).toPixmap(mupdf.Matrix.identity, mupdf.ColorSpace.DeviceRGB, true, true);
fs.writeFileSync("assets/icon-1024.png", pix.asPNG());
console.log("icon", pix.getWidth(), pix.getHeight());
