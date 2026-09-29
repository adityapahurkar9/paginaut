// Generates benchmark PDFs in ./testfiles:
//   scan-300.pdf  – 300 pages, each a distinct ~300dpi grayscale "scan" JPEG
//   text-1000.pdf – 1000 pages of vector text
import * as mupdf from "mupdf";
import fs from "node:fs";
import path from "node:path";

const outDir = path.resolve("testfiles");
fs.mkdirSync(outDir, { recursive: true });

const LOREM =
  "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. ";

function textPageContent(n, lines = 48) {
  let s = "BT /F1 11 Tf 14 TL 56 780 Td\n";
  s += `/F1 18 Tf (Page ${n}) Tj T* T* /F1 11 Tf\n`;
  for (let i = 0; i < lines; i++) {
    const off = (i * 37 + n * 11) % (LOREM.length - 90);
    s += `(${LOREM.slice(off, off + 88).replace(/[()\\]/g, "")}) Tj T*\n`;
  }
  return s + "ET\n";
}

// ---- text-1000.pdf
if (!process.argv.includes("--scan-only"))
{
  const doc = new mupdf.PDFDocument();
  const font = doc.addSimpleFont(new mupdf.Font("Times-Roman"));
  for (let i = 1; i <= 1000; i++) {
    const res = doc.addObject({ Font: { F1: font } });
    const page = doc.addPage([0, 0, 595, 842], 0, res, textPageContent(i));
    doc.insertPage(-1, page);
  }
  doc.save(path.join(outDir, "text-1000.pdf"), "compress");
  console.log("text-1000.pdf written");
}

// ---- scan-300.pdf: render a text page at 300dpi, add paper noise + skew-ish shading, JPEG it.
{
  let src = new mupdf.PDFDocument();
  let sfont = src.addSimpleFont(new mupdf.Font("Times-Roman"));
  const doc = new mupdf.PDFDocument();
  const dpi = 300;
  const W = Math.round(8.27 * dpi), H = Math.round(11.69 * dpi);
  let seed = 1;
  const rnd = () => ((seed = (seed * 1103515245 + 12345) & 0x7fffffff) / 0x7fffffff);
  for (let i = 1; i <= 300; i++) {
    const res = src.addObject({ Font: { F1: sfont } });
    const pobj = src.addPage([0, 0, 595, 842], 0, res, textPageContent(i, 50));
    src.insertPage(-1, pobj);
    const page = src.loadPage(src.countPages() - 1);
    const pix = page.toPixmap(mupdf.Matrix.scale(dpi / 72, dpi / 72), mupdf.ColorSpace.DeviceGray, false, false);
    const px = pix.getPixels();
    // paper tone + noise
    const tone = 225 + Math.floor(rnd() * 20);
    for (let k = 0; k < px.length; k++) {
      const v = px[k] === 255 ? tone : px[k];
      px[k] = Math.max(0, Math.min(255, v + ((rnd() * 10) | 0) - 5));
    }
    const jpg = pix.asJPEG(60); pix.destroy(); page.destroy();
    const img = doc.addImage(new mupdf.Image(jpg));
    const r = doc.addObject({ XObject: { Im0: img } });
    const pdfPage = doc.addPage([0, 0, 595, 842], 0, r, "q 595 0 0 842 0 0 cm /Im0 Do Q");
    doc.insertPage(-1, pdfPage);
    if (i % 25 === 0) { console.log("scan page", i, "jpg bytes", jpg.length); src.destroy(); src = new mupdf.PDFDocument(); sfont = src.addSimpleFont(new mupdf.Font("Times-Roman")); }
  }
  doc.save(path.join(outDir, "scan-300.pdf"), "");
  console.log("scan-300.pdf written", fs.statSync(path.join(outDir, "scan-300.pdf")).size);
  void W; void H;
}
