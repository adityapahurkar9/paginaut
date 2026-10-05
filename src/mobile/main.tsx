// The Android viewer: a library of recent files, and a reader. No editing, no accounts, no network.
import "./polyfills";
import { render } from "preact";
import { effect } from "@preact/signals";
import { useEffect, useRef, useState } from "preact/hooks";
import { Lock } from "lucide-preact";
import { Library } from "./Library";
import { native } from "./native";
import { checkIntent, passwordAsk, savePosition } from "./open";
import { Reader } from "./Reader";
import { Sheets } from "./Sheets";
import { tidyCache } from "./records";
import { scanIfStale, scanShelf } from "./shelf";
import { busy, chrome, doc, isDark, searching, settings, sheet, toast } from "./state";

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
  // Read after the theme's colours apply.
  requestAnimationFrame(() => {
    const css = getComputedStyle(document.documentElement);
    const color = css.getPropertyValue(reading ? (settings.value.night ? "--night-bar" : "--bar") : "--bg").trim();
    document.documentElement.style.background = color;
    void native.setBars(dark, immersive);
  });
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
(window as unknown as { paginautIntent: () => void }).paginautIntent = () => void checkIntent();
document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "visible") {
    void checkIntent();
    // Access may have changed meanwhile (all-files access turned on or off, say), and books come and go.
    if (!doc.value) void tidyCache().then(scanIfStale);
  }
  else savePosition();
});
