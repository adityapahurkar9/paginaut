// The Android viewer: a library of recent files, and a reader. No editing, no accounts, no network.
import "./polyfills";
import "../preact-px";
import { render } from "preact";
import { effect } from "@preact/signals";
import { useEffect, useRef, useState } from "preact/hooks";
import { Lock } from "lucide-preact";
import { Library } from "./Library";
import { native } from "./native";
import { listen } from "@tauri-apps/api/event";
import { checkIntent, leaveReader, passwordAsk, savePosition } from "./open";
import { Reader } from "./Reader";
import { Sheets } from "./Sheets";
import { tidyCache } from "./records";
import "./theme";
import { scanIfStale, scanShelf } from "./shelf";
import { busy, chrome, doc, isDark, look, say, searching, settings, sheet, toast } from "./state";
import { catchPageErrors } from "../errors";
import { afterCrash, crashedLastTime } from "./ReportSheet";
import { openSheet } from "./Sheets";

// Script errors and promises nobody caught go to the error log (Settings › About › Share error log).
catchPageErrors();

function App() {
  const d = doc.value;
  return (
    <>
      {d ? <Reader d={d} key={d.info.docId} /> : <Library />}
      <Sheets />
      {passwordAsk.value && <PasswordDialog />}
      {busy.value && (
        <div class="busy">
          <span class="spin" />
          {busy.value}
        </div>
      )}
      {toast.value && <div class="toast">{toast.value}</div>}
    </>
  );
}

function PasswordDialog() {
  const ask = passwordAsk.value!;
  const [pw, setPw] = useState("");
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => input.current?.focus(), [ask]);
  return (
    <div class="sheet-scrim center">
      <form
        class="dialog"
        onSubmit={(e) => {
          e.preventDefault();
          ask.answer(pw);
        }}
      >
        <Lock size={26} />
        <h3>This file is protected</h3>
        <p>{ask.wrong ? "That password isn't right. Try again." : `Enter the password to open ${ask.name}.`}</p>
        <input ref={input} type="password" autocomplete="off" value={pw} onInput={(e) => setPw((e.target as HTMLInputElement).value)} />
        <div class="dialog-btns">
          <button type="button" class="btn" onClick={() => ask.answer(null)}>
            Cancel
          </button>
          <button type="submit" class="btn pri" disabled={!pw}>
            Open
          </button>
        </div>
      </form>
    </div>
  );
}

render(<App />, document.getElementById("app")!);

// The app draws under the status and navigation bars (Android 15 and later always do this): it keeps
// its content inside the safe area Android reports, and paints the bars' background itself.
const setSafeArea = (t: number, r: number, b: number, l: number) => {
  const s = document.documentElement.style;
  s.setProperty("--safe-top", `${t}px`);
  s.setProperty("--safe-right", `${r}px`);
  s.setProperty("--safe-bottom", `${b}px`);
  s.setProperty("--safe-left", `${l}px`);
};
(window as unknown as { paginautSafeArea: typeof setSafeArea }).paginautSafeArea = setSafeArea;
native.safeArea().then((a) => setSafeArea(a.top, a.right, a.bottom, a.left), () => {});

// The bars take the colour of what's under them, and step aside while you read.
effect(() => {
  const reading = !!doc.value;
  const immersive = reading && !chrome.value && !sheet.value && !searching.value;
  const dark = isDark.value || (reading && settings.value.night);
  void look.value; // a new colour theme repaints the bars too
  // Read after the theme's colours apply.
  requestAnimationFrame(() => {
    const css = getComputedStyle(document.documentElement);
    const color = css.getPropertyValue(reading ? (settings.value.night ? "--night-bar" : "--bar") : "--bg").trim();
    document.documentElement.style.background = color;
    void native.setBars(dark, immersive);
  });
});

// Hide from screenshots (the View sheet). Android remembers it too, so it's on before the first frame.
effect(() => void native.setSecure(settings.value.secure));

// A document that stopped the reading engine twice is closed by it (engine_link.rs): leave it and say why.
void listen<number>("engine-refused", (e) => {
  if (doc.value?.info.docId !== e.payload) return;
  leaveReader();
  say("This document stopped Paginaut's reading engine twice, so it was closed. The file may be damaged.");
});

// Copies the app no longer needs go (records.ts tidyCache), and Recent drops files that can't be
// opened any more: checked from the library, so never under a document being opened.
effect(() => {
  if (doc.value || busy.value) return;
  void tidyCache();
});

// Files from other apps ("Open with", Share): at start, whenever the app comes back, and when
// Android hands one over while it's open (PaginautPlugin.onNewIntent calls this).
void checkIntent();
void scanShelf();
// The last session crashed: Report a problem is offered once (nothing is sent unless the user sends it).
void crashedLastTime().then((crashed) => {
  if (!crashed || sheet.value) return;
  afterCrash.value = true;
  openSheet("report");
});
(window as unknown as { paginautIntent: () => void }).paginautIntent = () => void checkIntent();
document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "visible") {
    void checkIntent();
    // Access may have changed meanwhile (all-files access turned on or off, say), and books come and go.
    if (!doc.value) void tidyCache().then(scanIfStale);
  }
  else savePosition();
});
