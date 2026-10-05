// Renders the app icon (assets/icon.svg) to a 1024x1024 PNG with a transparent background,
// using Edge headless (MuPDF's wasm build doesn't read SVG). Then: npx tauri icon assets/icon-1024.png
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";

const edge = ["ProgramFiles(x86)", "ProgramFiles"]
  .map((v) => process.env[v] && path.join(process.env[v], "Microsoft", "Edge", "Application", "msedge.exe"))
  .find((p) => p && fs.existsSync(p));
if (!edge) throw new Error("Microsoft Edge not found");

const svg = path.resolve("assets/icon.svg");
const out = path.resolve("assets/icon-1024.png");
const page = path.join(os.tmpdir(), "paginaut-icon.html");
fs.writeFileSync(page, `<html><body style="margin:0"><img src="${pathToFileURL(svg)}" width="1024" height="1024" style="display:block"></body></html>`);
execFileSync(edge, [
  "--headless=new", "--disable-gpu", "--hide-scrollbars", "--allow-file-access-from-files",
  "--default-background-color=00000000", "--window-size=1024,1024", `--screenshot=${out}`, pathToFileURL(page).href,
], { stdio: "ignore" });
fs.unlinkSync(page);
console.log("icon", out, fs.statSync(out).size, "bytes");
