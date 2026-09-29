// Writes testfiles/form.pdf: a fillable form exercising what Phase 3 supports
// (text, required, date and number formats, a calculated read-only total,
// checkbox, radio group, dropdown, multi-line, comb, signature field).
//   node tools/make-form-pdf.mjs
import { writeFileSync, mkdirSync } from "node:fs";

const objs = [];
const add = (body) => (objs.push(body), objs.length);
const pdfStr = (s) => "(" + s.replace(/[\\()]/g, "\\$&") + ")";
const js = (code) => `<< /S /JavaScript /JS ${pdfStr(code)} >>`;

const catalog = add(null); // filled in later
const pages = add(null);
const page = add(null);
const content = add(null);
const helv = add("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>");
const empty = add("<< /Length 0 >>\nstream\n\nendstream");

const fields = [], annots = [];
const labels = [];
let y = 700;
const row = (label, h = 22) => {
  labels.push([label, y + 6]);
  const r = [180, y + 22 - h, 540, y + 22];
  y -= h + 18;
  return r;
};
const widget = (dict, rect) => add(`<< /Type /Annot /Subtype /Widget /P ${page} 0 R /F 4 /Rect [${rect.join(" ")}] /MK << /BC [0.55 0.55 0.6] >> ${dict} >>`);
const field = (dict, rect) => {
  const n = widget(dict, rect);
  fields.push(n);
  annots.push(n);
  return n;
};
const DA = "/DA (/Helv 11 Tf 0 g)";

field(`/FT /Tx /T (name) /TU (Full name) ${DA}`, row("Full name"));
field(`/FT /Tx /T (email) /Ff 2 ${DA}`, row("Email (required)"));
field(`/FT /Tx /T (dob) ${DA} /AA << /K ${js('AFDate_KeystrokeEx("dd/mm/yyyy");')} /F ${js('AFDate_FormatEx("dd/mm/yyyy");')} >>`, row("Date of birth"));
const qty = field(`/FT /Tx /T (qty) ${DA} /AA << /K ${js('AFNumber_Keystroke(0, 0, 0, 0, "", true);')} /F ${js('AFNumber_Format(0, 0, 0, 0, "", true);')} >>`, row("Quantity"));
const price = field(`/FT /Tx /T (price) ${DA} /AA << /K ${js('AFNumber_Keystroke(2, 0, 0, 0, "", true);')} /F ${js('AFNumber_Format(2, 0, 0, 0, "", true);')} >>`, row("Price (Rs)"));
const total = field(`/FT /Tx /T (total) /Ff 1 ${DA} /AA << /C ${js('AFSimple_Calculate("PRD", new Array("qty", "price"));')} /F ${js('AFNumber_Format(2, 0, 0, 0, "Rs ", true);')} >>`, row("Total (calculated)"));
void qty; void price;

const agree = row("I agree", 16);
agree[2] = agree[0] + 16;
field(`/FT /Btn /T (agree) /V /Off /AS /Off /DA (/ZaDb 0 Tf 0 g) /AP << /N << /Yes ${empty} 0 R /Off ${empty} 0 R >> >>`, agree);

// Radio group: a parent field with three widgets.
const gy = y;
labels.push(["Gender", y + 6]);
y -= 34;
const radio = add(null);
fields.push(radio);
const kids = ["Male", "Female", "Other"].map((opt, i) => {
  labels.push([opt, gy + 4, 204 + i * 110]);
  const n = widget(`/Parent ${radio} 0 R /AS /Off /DA (/ZaDb 0 Tf 0 g) /AP << /N << /${opt} ${empty} 0 R /Off ${empty} 0 R >> >>`, [180 + i * 110, gy, 196 + i * 110, gy + 16]);
  annots.push(n);
  return n;
});
objs[radio - 1] = `<< /FT /Btn /T (gender) /Ff 49152 /V /Off /Kids [${kids.map((k) => `${k} 0 R`).join(" ")}] >>`;

field(`/FT /Ch /T (country) /Ff 131072 /Opt [(India) (United States) (United Kingdom) (Singapore)] /V (India) ${DA}`, row("Country"));
const pin = row("PIN code");
pin[2] = pin[0] + 150;
field(`/FT /Tx /T (pin) /Ff 16777216 /MaxLen 6 ${DA}`, pin);
field(`/FT /Tx /T (comments) /Ff 4096 ${DA}`, row("Comments", 70));
field(`/FT /Sig /T (signature)`, row("Signature", 50));

let stream = "BT /Helv 20 Tf 72 745 Td (Folio test form) Tj ET\n";
for (const [text, ty, tx] of labels) stream += `BT /Helv 11 Tf ${tx ?? 72} ${ty} Td ${pdfStr(text)} Tj ET\n`;
objs[content - 1] = `<< /Length ${stream.length} >>\nstream\n${stream}endstream`;
objs[page - 1] = `<< /Type /Page /Parent ${pages} 0 R /MediaBox [0 0 612 792] /Contents ${content} 0 R /Resources << /Font << /Helv ${helv} 0 R >> >> /Annots [${annots.map((a) => `${a} 0 R`).join(" ")}] >>`;
objs[pages - 1] = `<< /Type /Pages /Kids [${page} 0 R] /Count 1 >>`;
objs[catalog - 1] = `<< /Type /Catalog /Pages ${pages} 0 R /AcroForm << /Fields [${fields.map((f) => `${f} 0 R`).join(" ")}] /NeedAppearances true /DA (/Helv 0 Tf 0 g) /DR << /Font << /Helv ${helv} 0 R >> >> /CO [${total} 0 R] >> >>`;

let out = "%PDF-1.7\n%\xE2\xE3\xCF\xD3\n";
const offsets = [];
objs.forEach((body, i) => {
  offsets.push(Buffer.byteLength(out, "latin1"));
  out += `${i + 1} 0 obj\n${body}\nendobj\n`;
});
const xref = Buffer.byteLength(out, "latin1");
out += `xref\n0 ${objs.length + 1}\n0000000000 65535 f \n` + offsets.map((o) => `${String(o).padStart(10, "0")} 00000 n \n`).join("");
out += `trailer\n<< /Size ${objs.length + 1} /Root ${catalog} 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
mkdirSync("testfiles", { recursive: true });
writeFileSync("testfiles/form.pdf", Buffer.from(out, "latin1"));
console.log(`testfiles/form.pdf: ${fields.length} fields`);
