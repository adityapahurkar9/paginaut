// site/og.png (1280 x 640): the picture shown when the website or the repository is shared (og:image, and
// GitHub's social preview, uploaded by hand in the repository's Settings). Drawn as assets/play/render.mjs draws
// the Play graphics: Edge headless, cut to size with Pillow (PYTHON names another python than "python").
//   node assets/render-og.mjs
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";

const edge = ["ProgramFiles(x86)", "ProgramFiles"]
  .map((v) => process.env[v] && path.join(process.env[v], "Microsoft", "Edge", "Application", "msedge.exe"))
  .find((p) => p && fs.existsSync(p));
if (!edge) throw new Error("Microsoft Edge not found");
const icon = pathToFileURL(path.resolve("assets/icon.svg")).href;
const python = process.env.PYTHON || "python";
const [w, h] = [1280, 640];
const out = path.resolve("site/og.png");
const page = path.join(os.tmpdir(), "paginaut-og.html");
fs.writeFileSync(page, `<!doctype html><html><head><meta charset="utf-8"></head><body style="margin:0">
<div style="width:${w}px;height:${h}px;box-sizing:border-box;display:flex;align-items:center;gap:64px;padding:0 96px;
  background:linear-gradient(135deg,#0f2a5c 0%,#1d3f8f 55%,#2a56c6 100%);font-family:'Segoe UI',Roboto,sans-serif;color:#fff">
  <div style="flex:none;width:300px;height:300px;border-radius:66px;background:#fff;display:flex;align-items:center;justify-content:center;box-shadow:0 16px 40px rgba(0,0,0,.35)">
    <img src="${icon}" width="244" height="244">
  </div>
  <div>
    <div style="font-size:88px;font-weight:700;letter-spacing:-1.5px;line-height:1">Paginaut</div>
    <div style="font-size:34px;margin-top:22px;opacity:.95;line-height:1.3">Free, open-source PDF reader and editor<br>for Windows and Linux.</div>
    <div style="font-size:26px;margin-top:18px;opacity:.8">PDF · e-books · comics · XPS · Markdown</div>
  </div>
</div></body></html>`);
execFileSync(edge, [
  "--headless=new", "--disable-gpu", "--hide-scrollbars", "--allow-file-access-from-files", "--force-device-scale-factor=1",
  "--default-background-color=0F2A5CFF", `--window-size=${w + 200},${h + 200}`, `--screenshot=${out}`, pathToFileURL(page).href,
], { stdio: "ignore" });
fs.unlinkSync(page);
execFileSync(python, ["-c", `from PIL import Image; im = Image.open(r"${out}").convert("RGB").crop((0, 0, ${w}, ${h})); im.save(r"${out}", optimize=True)`]);
console.log("og.png", fs.statSync(out).size, "bytes");
