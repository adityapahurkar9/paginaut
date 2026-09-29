// Frontend build: bundles src/ into dist/ with esbuild.
//   node build.mjs          development build
//   node build.mjs --prod   minified release build
//   node build.mjs --watch  rebuild on change
import * as esbuild from "esbuild";
import fs from "node:fs";

const prod = process.argv.includes("--prod");
const watch = process.argv.includes("--watch");

fs.mkdirSync("dist", { recursive: true });
const copyStatic = () => {
  for (const f of ["index.html", "styles.css"]) fs.copyFileSync(`src/${f}`, `dist/${f}`);
};
copyStatic();

const options = {
  entryPoints: { app: "src/main.tsx" },
  outdir: "dist",
  bundle: true,
  format: "esm",
  platform: "browser",
  target: "chrome120",
  jsx: "automatic",
  jsxImportSource: "preact",
  minify: prod,
  sourcemap: prod ? false : "linked",
  legalComments: "none",
  logLevel: "info",
  define: { "process.env.NODE_ENV": JSON.stringify(prod ? "production" : "development") },
};

if (watch) {
  const ctx = await esbuild.context(options);
  await ctx.watch();
  fs.watch("src", (_e, f) => { if (f === "index.html" || f === "styles.css") copyStatic(); });
} else {
  await esbuild.build(options);
}
