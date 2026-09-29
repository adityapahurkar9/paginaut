// Extra test files in ./testfiles for format coverage:
//   outline-links.pdf  – bookmarks tree + internal and external links
//   broken.pdf         – truncated copy of text-1000.pdf (tests repair)
//   book.epub          – small reflowable e-book
//   comic.cbz          – zip of page images
//   photo.png          – single image
import * as mupdf from "mupdf";
import fs from "node:fs";
import zlib from "node:zlib";

const out = "testfiles";
fs.mkdirSync(out, { recursive: true });

// ---------------------------------------------------------------- tiny zip writer (store/deflate)
function crc32(buf) {
  let c, crc = 0xffffffff;
  for (let n = 0; n < buf.length; n++) {
    c = (crc ^ buf[n]) & 0xff;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    crc = (crc >>> 8) ^ c;
  }
  return (crc ^ 0xffffffff) >>> 0;
}
function zip(entries) {
  const parts = [], central = [];
  let offset = 0;
  for (const { name, data, store } of entries) {
    const raw = Buffer.from(data);
    const comp = store ? raw : zlib.deflateRawSync(raw);
    const nameBuf = Buffer.from(name);
    const crc = crc32(raw);
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0); local.writeUInt16LE(20, 4); local.writeUInt16LE(0, 6);
    local.writeUInt16LE(store ? 0 : 8, 8); local.writeUInt32LE(0, 10); local.writeUInt32LE(crc, 14);
    local.writeUInt32LE(comp.length, 18); local.writeUInt32LE(raw.length, 22); local.writeUInt16LE(nameBuf.length, 26); local.writeUInt16LE(0, 28);
    parts.push(local, nameBuf, comp);
    const cen = Buffer.alloc(46);
    cen.writeUInt32LE(0x02014b50, 0); cen.writeUInt16LE(20, 4); cen.writeUInt16LE(20, 6); cen.writeUInt16LE(0, 8);
    cen.writeUInt16LE(store ? 0 : 8, 10); cen.writeUInt32LE(0, 12); cen.writeUInt32LE(crc, 16); cen.writeUInt32LE(comp.length, 20);
    cen.writeUInt32LE(raw.length, 24); cen.writeUInt16LE(nameBuf.length, 28); cen.writeUInt32LE(offset, 42);
    central.push(cen, nameBuf);
    offset += 30 + nameBuf.length + comp.length;
  }
  const cenBuf = Buffer.concat(central);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0); end.writeUInt16LE(entries.length, 8); end.writeUInt16LE(entries.length, 10);
  end.writeUInt32LE(cenBuf.length, 12); end.writeUInt32LE(offset, 16);
  return Buffer.concat([...parts, cenBuf, end]);
}

// ---------------------------------------------------------------- outline + links PDF
{
  const doc = new mupdf.PDFDocument();
  const font = doc.addSimpleFont(new mupdf.Font("Helvetica"));
  const chapters = ["Introduction", "Getting started", "Reading", "Marking up", "Signing", "Tools", "Appendix"];
  for (let i = 0; i < 20; i++) {
    const res = doc.addObject({ Font: { F1: font } });
    const ch = chapters[Math.floor(i / 3)] ?? "Appendix";
    let content = `BT /F1 24 Tf 60 760 Td (${ch}) Tj ET\nBT /F1 11 Tf 60 730 Td (Page ${i + 1}. Links on the first page jump to each chapter.) Tj ET\n`;
    if (i === 0) {
      chapters.forEach((c, k) => { content += `BT /F1 13 Tf 0 0 1 rg 72 ${680 - k * 24} Td (${k + 1}. ${c}) Tj ET\n`; });
      content += `BT /F1 13 Tf 0 0 1 rg 72 480 Td (Visit mupdf.com) Tj ET\n`;
    }
    doc.insertPage(-1, doc.addPage([0, 0, 612, 792], 0, res, content));
  }
  const p0 = doc.loadPage(0);
  chapters.forEach((c, k) => {
    const y = 792 - (680 - k * 24);
    p0.createLink([70, y - 14, 300, y + 4], doc.formatLinkURI({ chapter: 0, page: Math.min(19, k * 3), type: "Fit", x: 0, y: 0, width: 0, height: 0, zoom: 0 }));
  });
  p0.createLink([70, 792 - 480 - 14, 200, 792 - 480 + 4], "https://mupdf.com");
  const it = doc.outlineIterator();
  chapters.forEach((c, k) => {
    it.insert({ title: c, uri: doc.formatLinkURI({ chapter: 0, page: Math.min(19, k * 3), type: "Fit", x: 0, y: 0, width: 0, height: 0, zoom: 0 }), open: true });
  });
  doc.save(`${out}/outline-links.pdf`, "compress");
  console.log("outline-links.pdf");
}

// ---------------------------------------------------------------- broken PDF
{
  const src = fs.readFileSync(`${out}/text-1000.pdf`);
  fs.writeFileSync(`${out}/broken.pdf`, src.subarray(0, Math.floor(src.length * 0.7)));
  console.log("broken.pdf");
}

// ---------------------------------------------------------------- EPUB
{
  const chapter = (n) => `<?xml version="1.0" encoding="utf-8"?><html xmlns="http://www.w3.org/1999/xhtml"><head><title>Chapter ${n}</title></head><body><h1>Chapter ${n}</h1>${
    Array.from({ length: 12 }, (_, i) => `<p>This is paragraph ${i + 1} of chapter ${n}. Reflowable books are laid out to a page size by Paginaut, so text wraps like an e-reader. हिंदी पाठ भी सही दिखना चाहिए।</p>`).join("")
  }</body></html>`;
  const files = [
    { name: "mimetype", data: "application/epub+zip", store: true },
    { name: "META-INF/container.xml", data: `<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>` },
    { name: "OEBPS/content.opf", data: `<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Test Book</dc:title><dc:identifier id="id">paginaut-test</dc:identifier><dc:language>en</dc:language></metadata><manifest>${[1, 2, 3].map((n) => `<item id="c${n}" href="c${n}.xhtml" media-type="application/xhtml+xml"/>`).join("")}<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/></manifest><spine toc="ncx">${[1, 2, 3].map((n) => `<itemref idref="c${n}"/>`).join("")}</spine></package>` },
    { name: "OEBPS/toc.ncx", data: `<?xml version="1.0"?><ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1"><head><meta name="dtb:uid" content="paginaut-test"/></head><docTitle><text>Test Book</text></docTitle><navMap>${[1, 2, 3].map((n) => `<navPoint id="n${n}" playOrder="${n}"><navLabel><text>Chapter ${n}</text></navLabel><content src="c${n}.xhtml"/></navPoint>`).join("")}</navMap></ncx>` },
    ...[1, 2, 3].map((n) => ({ name: `OEBPS/c${n}.xhtml`, data: chapter(n) })),
  ];
  fs.writeFileSync(`${out}/book.epub`, zip(files));
  console.log("book.epub");
}

// ---------------------------------------------------------------- CBZ + PNG
{
  const imgs = [];
  for (let i = 0; i < 6; i++) {
    const pix = new mupdf.Pixmap(mupdf.ColorSpace.DeviceRGB, [0, 0, 800, 1200], false);
    const px = pix.getPixels();
    for (let y = 0; y < 1200; y++)
      for (let x = 0; x < 800; x++) {
        const o = (y * 800 + x) * 3;
        px[o] = (x * 255 / 800 + i * 40) & 255; px[o + 1] = (y * 255 / 1200) & 255; px[o + 2] = ((x ^ y) & 64) ? 220 : 90;
      }
    imgs.push({ name: `page-${String(i + 1).padStart(2, "0")}.jpg`, data: pix.asJPEG(85), store: true });
    if (i === 0) fs.writeFileSync(`${out}/photo.png`, pix.asPNG());
  }
  fs.writeFileSync(`${out}/comic.cbz`, zip(imgs));
  console.log("comic.cbz, photo.png");
}

// ---------------------------------------------------------------- page labels (i, ii, iii, then 1, 2, … and A-1 appendix)
{
  const doc = new mupdf.PDFDocument();
  const font = doc.addSimpleFont(new mupdf.Font("Helvetica"));
  for (let i = 0; i < 12; i++) {
    const res = doc.addObject({ Font: { F1: font } });
    doc.insertPage(-1, doc.addPage([0, 0, 612, 792], 0, res, `BT /F1 20 Tf 72 700 Td (Physical page ${i + 1}) Tj ET`));
  }
  doc.setPageLabels(0, "r", "", 1);
  doc.setPageLabels(3, "D", "", 1);
  doc.setPageLabels(10, "D", "A-", 1);
  doc.save(`${out}/labels.pdf`, "compress");
  console.log("labels.pdf");
}
