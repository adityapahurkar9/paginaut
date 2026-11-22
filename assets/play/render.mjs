// Play store graphics from assets/icon.svg, with Edge headless (as assets/render-icon.mjs):
// assets/play/icon-512.png (on white, as the launcher's adaptive icon; Play draws it as a full square) and
// assets/play/feature-graphic.png (1024 x 500, opaque). Edge's viewport is smaller than its window, so each is drawn
// in a bigger window and cut to size with Pillow (E:/Anaconda/python.exe has it).
//   node assets/play/render.mjs
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

const python = process.env.PYTHON || "E:/Anaconda/python.exe";

function shot(name, w, h, html, bg) {
  const page = path.join(os.tmpdir(), `paginaut-${name}.html`);
  const out = path.resolve("assets/play", name);
  fs.writeFileSync(page, `<!doctype html><html><head><meta charset="utf-8"></head><body style="margin:0">${html}</body></html>`);
  execFileSync(edge, [
    "--headless=new", "--disable-gpu", "--hide-scrollbars", "--allow-file-access-from-files", "--force-device-scale-factor=1",
    `--default-background-color=${bg}`, `--window-size=${w + 200},${h + 200}`, `--screenshot=${out}`, pathToFileURL(page).href,
  ], { stdio: "ignore" });
  fs.unlinkSync(page);
  execFileSync(python, ["-c", `from PIL import Image; im = Image.open(r"${out}").convert("RGB").crop((0, 0, ${w}, ${h})); im.save(r"${out}")`]);
  console.log(name, fs.statSync(out).size, "bytes");
}

shot("icon-512.png", 512, 512, `<div style="width:512px;height:512px;background:#fff;display:flex;align-items:center;justify-content:center"><img src="${icon}" width="400" height="400"></div>`, "FFFFFFFF");

shot("feature-graphic.png", 1024, 500, `
<div style="width:1024px;height:500px;box-sizing:border-box;display:flex;align-items:center;gap:56px;padding:0 72px;
  background:linear-gradient(135deg,#0f2a5c 0%,#1d3f8f 55%,#2a56c6 100%);font-family:'Segoe UI',Roboto,sans-serif;color:#fff">
  <div style="flex:none;width:272px;height:272px;border-radius:60px;background:#fff;display:flex;align-items:center;justify-content:center;box-shadow:0 14px 36px rgba(0,0,0,.35)">
    <img src="${icon}" width="220" height="220">
  </div>
  <div>
    <div style="font-size:76px;font-weight:700;letter-spacing:-1px;line-height:1">Paginaut</div>
    <div style="font-size:31px;margin-top:18px;opacity:.95;line-height:1.3">PDF, e-books and comics.<br>No ads, no account, no internet.</div>
  </div>
</div>`, "0F2A5CFF");
