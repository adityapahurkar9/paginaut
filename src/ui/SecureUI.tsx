// Protect (passwords and permissions), reduce file size, and repair.
import { signal } from "@preact/signals";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import type { ComponentChildren } from "preact";
import { useEffect, useState } from "preact/hooks";
import { X } from "lucide-preact";
import { api, type DocInfo, type SaveOptions, type Security } from "../api";
import { saveWithOptions } from "../pages";
import { baseName, showToast, type Tab } from "../store";
import { docSigs } from "../dsc";
import { testHook } from "./PagesUI";

type Kind = "protect" | "compress" | "repair";
export const secureDialog = signal<{ tab: Tab; kind: Kind } | null>(null);

export function startSecure(tab: Tab, kind: Kind) {
  if (!tab.info.value?.isPdf) return showToast("Only PDFs can be protected, made smaller or repaired. Save this file as a PDF first (Save as PDF).", 4500);
  secureDialog.value = { tab, kind };
}

export function SecureDialog() {
  const d = secureDialog.value;
  return d ? <SecureInner key={`${d.tab.key}-${d.kind}`} tab={d.tab} kind={d.kind} /> : null;
}

// PDF permission bits.
const PRINT = (1 << 2) | (1 << 11);
const COPY = 1 << 4;
const COMMENT = (1 << 5) | (1 << 8);
const PAGES = 1 << 10;
const MODIFY = 1 << 3;
const RIGHTS: [number, string][] = [
  [PRINT, "Printing"],
  [COPY, "Copying text and pictures"],
  [COMMENT, "Comments, filling in forms and signing"],
  [PAGES, "Adding, deleting and rotating pages"],
  [MODIFY, "Other changes"],
];

const mb = (b: number) => (b >= 1 << 20 ? `${(b / (1 << 20)).toFixed(1)} MB` : `${Math.max(1, Math.round(b / 1024))} KB`);

function describe(s: Security) {
  if (!s.method) return "Not protected.";
  const off = RIGHTS.filter(([bit]) => (s.perms & bit) !== bit).map(([, name]) => name.toLowerCase());
  return [
    `Protected (${s.method} ${s.bits}-bit)`,
    s.opens ? "needs a password to open" : "opens without a password",
    off.length ? `not allowed: ${off.join(", ")}` : "",
  ]
    .filter(Boolean)
    .join(" · ");
}

function SecureInner({ tab, kind }: { tab: Tab; kind: Kind }) {
  const info = tab.info.value!;
  const [sec, setSec] = useState<Security | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState("");
  const close = () => !busy && (secureDialog.value = null);
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        close();
      }
    };
    window.addEventListener("keydown", key, true);
    return () => window.removeEventListener("keydown", key, true);
  }, [busy]);
  useEffect(() => {
    api.security(info.docId).then(setSec, (e) => setError(String(e)));
  }, []);

  /** Saves with `opts` to a copy (asks where) or in place; reports the result. */
  const run = async (opts: SaveOptions, copy: boolean, suffix: string, doing: string, report: (saved: DocInfo, before: number) => string) => {
    let target: string | null = null;
    // A PDF made from an Office file lives in the app's cache: it's always saved as a new file.
    if (copy || info.converted) {
      const stem = baseName(tab.path).replace(/\.[^.]+$/, "");
      target =
        testHook()?.save ??
        (await saveDialog({
          title: "Save a copy as",
          defaultPath: tab.path.replace(/[^\\/]+$/, `${stem} (${suffix}).pdf`),
          filters: [{ name: "PDF", extensions: ["pdf"] }],
        }));
      if (!target) return;
    }
    setError("");
    setBusy(doing);
    const before = info.fileSize;
    const saved = await saveWithOptions(tab, opts, target);
    setBusy("");
    if (saved) {
      secureDialog.value = null;
      showToast(report(saved, before), 7000);
    }
  };

  const title = { protect: "Protect", compress: "Reduce file size", repair: "Repair" }[kind];
  return (
    <div class="overlay" onPointerDown={(e) => e.target === e.currentTarget && close()}>
      <div class="dialog tool-dialog secure-dialog" role="dialog" aria-label={title}>
        <button class="ib sm close" aria-label="Close" onClick={close}>
          <X size={16} />
        </button>
        <h2>{title}</h2>
        {docSigs(tab).value?.some((s) => s.signed) && (
          <span class="secure-warn">
            This file is digitally signed. {title} writes the whole file again, so its signatures will no longer be valid. Save a copy to keep the signed
            original as it is.
          </span>
        )}
        {!sec ? (
          <span class="hint">{error || "Reading the file…"}</span>
        ) : kind === "protect" ? (
          <Protect tab={tab} sec={sec} setSec={setSec} busy={busy} run={run} setError={setError} />
        ) : kind === "compress" ? (
          <Compress info={info} busy={busy} run={run} />
        ) : (
          <Repair sec={sec} pages={tab.edits.value?.order.length ?? info.pageCount} busy={busy} run={run} />
        )}
        {error && sec && <span class="error-text">{error}</span>}
      </div>
    </div>
  );
}

type Run = (opts: SaveOptions, copy: boolean, suffix: string, doing: string, report: (saved: DocInfo, before: number) => string) => Promise<void>;

function Footer({ busy, onCopy, onReplace, replace = "Replace this file", extra }: { busy: string; onCopy: () => void; onReplace: () => void; replace?: string; extra?: ComponentChildren }) {
  return (
    <div class="row">
      {extra}
      <span style={{ flex: 1 }} />
      {busy ? (
        <span class="secure-busy">
          <span class="spin" /> {busy}
        </span>
      ) : (
        <>
          <button class="btn" onClick={onCopy}>
            Save a copy…
          </button>
          <button class="btn pri" onClick={onReplace}>
            {replace}
          </button>
        </>
      )}
    </div>
  );
}

// ---------------------------------------------------------------- protect

function Protect({ tab, sec, setSec, busy, run, setError }: { tab: Tab; sec: Security; setSec: (s: Security) => void; busy: string; run: Run; setError: (s: string) => void }) {
  const docId = tab.info.value!.docId;
  const [ownerTry, setOwnerTry] = useState("");
  const [openPw, setOpenPw] = useState(sec.opens);
  const [user, setUser] = useState(["", ""]);
  const [restrict, setRestrict] = useState(!!sec.method && sec.perms !== -1 && (sec.perms & 0xf3c) !== 0xf3c);
  const [owner, setOwner] = useState(["", ""]);
  const [allowed, setAllowed] = useState(sec.method ? sec.perms : ~(COPY | PAGES | MODIFY));
  const [show, setShow] = useState(false);

  if (sec.method && !sec.owner) {
    const unlock = async () => {
      if (await api.checkOwner(docId, ownerTry)) setSec(await api.security(docId));
      else setError("That isn't the permissions password.");
    };
    return (
      <>
        <span class="hint">{describe(sec)}</span>
        <span class="hint">To change or remove the protection, enter the file's permissions password (the owner's, which can differ from the one that opens it).</span>
        <div class="ps-line">
          <input class="inp sm" type="password" placeholder="Permissions password" autoFocus value={ownerTry} onInput={(e) => setOwnerTry((e.target as HTMLInputElement).value)} onKeyDown={(e) => e.key === "Enter" && void unlock()} />
          <button class="btn pri sm" disabled={!ownerTry} onClick={() => void unlock()}>
            Continue
          </button>
        </div>
      </>
    );
  }

  const pw = (pair: string[], set: (f: (p: string[]) => string[]) => void, what: string) => {
    const input = (i: number) => (e: Event) => {
      const v = (e.target as HTMLInputElement).value;
      set((p) => (i ? [p[0], v] : [v, p[1]]));
    };
    // Shown or hidden as typed. (Preact types a text field and a password field apart, so each is written out.)
    const field = (i: number, placeholder: string) => {
      const p = { class: "inp sm", placeholder, autoComplete: "new-password", value: pair[i], onInput: input(i) };
      return show ? <input type="text" {...p} /> : <input type="password" {...p} />;
    };
    return (
      <div class="secure-pw">
        {field(0, what)}
        {field(1, "Type it again")}
      </div>
    );
  };

  const protect = (copy: boolean) => {
    if (!openPw && !restrict) return setError("Choose a password to open the file, or restrictions, or both.");
    if (openPw && !user[0]) return setError("Type the password that will open the file.");
    if (openPw && user[0] !== user[1]) return setError("The two open passwords don't match.");
    if (restrict && !owner[0]) return setError("Restrictions need a permissions password, or anyone could lift them.");
    if (restrict && owner[0] !== owner[1]) return setError("The two permissions passwords don't match.");
    if (openPw && restrict && user[0] === owner[0]) return setError("Use a different password for permissions than for opening, or anyone who can open the file could lift the restrictions.");
    const perms = restrict ? allowed : -1;
    // Without restrictions the open password is the owner's too (it can change the protection).
    const protect = { user: openPw ? user[0] : "", owner: restrict ? owner[0] : user[0], perms };
    void run({ protect }, copy, "protected", "Protecting…", (s) => `Protected ${baseName(s.path)}${openPw ? " · it asks for the password when opened" : ""}`);
  };

  return (
    <>
      <span class="hint">{describe(sec)}</span>
      <label class="check-row secure-head">
        <input type="checkbox" checked={openPw} onChange={(e) => setOpenPw((e.target as HTMLInputElement).checked)} /> Require a password to open the file
      </label>
      {openPw && pw(user, setUser, "Password to open")}
      <label class="check-row secure-head">
        <input type="checkbox" checked={restrict} onChange={(e) => setRestrict((e.target as HTMLInputElement).checked)} /> Restrict printing and changes
      </label>
      {restrict && (
        <>
          {pw(owner, setOwner, "Permissions password")}
          <div class="secure-rights">
            <span>Allow:</span>
            {RIGHTS.map(([bit, name]) => (
              <label key={bit} class="check-row">
                <input type="checkbox" checked={(allowed & bit) === bit} onChange={(e) => { const on = (e.target as HTMLInputElement).checked; setAllowed((a) => (on ? a | bit : a & ~bit)); }} /> {name}
              </label>
            ))}
          </div>
          <span class="hint">Most PDF apps respect these restrictions, but a few ignore them. For anything confidential, use a password to open.</span>
        </>
      )}
      {(openPw || restrict) && (
        <label class="check-row">
          <input type="checkbox" checked={show} onChange={(e) => setShow((e.target as HTMLInputElement).checked)} /> Show passwords
        </label>
      )}
      <span class="hint">Protected with AES-256. A forgotten open password can't be recovered, so keep it somewhere safe.</span>
      <Footer
        busy={busy}
        onCopy={() => protect(true)}
        onReplace={() => protect(false)}
        replace="Protect this file"
        extra={
          sec.method ? (
            <button class="btn ghost" disabled={!!busy} onClick={() => void run({ protect: { remove: true } }, false, "unprotected", "Removing protection…", (s) => `Protection removed from ${baseName(s.path)}`)}>
              Remove protection
            </button>
          ) : undefined
        }
      />
    </>
  );
}

// ---------------------------------------------------------------- compress

const LEVELS: [0 | 1 | 2, string, string][] = [
  [0, "Tidy up (same quality)", "Removes duplicate and unused parts and packs the rest tightly. Nothing is lost."],
  [1, "Smaller", "Pictures and scans at 150 dpi, photos as JPEG. Good for reading on screen and printing."],
  [2, "Smallest", "Pictures and scans at 100 dpi, all as JPEG. Good for email and uploads; fine print in scans may blur."],
];

function Compress({ info, busy, run }: { info: DocInfo; busy: string; run: Run }) {
  const [level, setLevel] = useState<0 | 1 | 2>(1);
  const go = (copy: boolean) =>
    void run({ compress: level }, copy, "smaller", "Reducing size… (scans can take a minute)", (s, before) => {
      const pct = Math.round((1 - s.fileSize / before) * 100);
      return pct > 0 ? `${baseName(s.path)}: ${mb(before)} → ${mb(s.fileSize)} (${pct}% smaller)` : `${baseName(s.path)} is ${mb(s.fileSize)}: it was already about as small as it gets at this setting.`;
    });
  return (
    <>
      <span class="hint">Now {mb(info.fileSize)}, {info.pageCount} {info.pageCount === 1 ? "page" : "pages"}.</span>
      <div class="secure-levels">
        {LEVELS.map(([v, name, note]) => (
          <label key={v} class={`secure-level ${level === v ? "on" : ""}`}>
            <input type="radio" name="level" checked={level === v} onChange={() => setLevel(v)} />
            <span>
              <b>{name}</b>
              <small>{note}</small>
            </span>
          </label>
        ))}
      </div>
      <Footer busy={busy} onCopy={() => go(true)} onReplace={() => go(false)} />
    </>
  );
}

// ---------------------------------------------------------------- repair

function Repair({ sec, pages, busy, run }: { sec: Security; pages: number; busy: string; run: Run }) {
  const go = (copy: boolean) =>
    void run({ rewrite: true }, copy, "repaired", "Repairing…", (s) => {
      const lost = pages - s.pageCount;
      return lost > 0
        ? `Repaired ${baseName(s.path)}: ${s.pageCount} pages kept; ${lost} ${lost === 1 ? "page was" : "pages were"} too damaged to read.`
        : `Repaired ${baseName(s.path)}: written again from scratch, all ${s.pageCount} pages kept.`;
    });
  return (
    <>
      <span class="hint">
        {sec.repaired
          ? "This file is damaged. Paginaut rebuilt what it could when opening it; save it to keep the repair, so other apps can open it too."
          : "No damage found. Saving writes the whole file again from scratch, which can fix files that other apps have trouble with."}
      </span>
      <Footer busy={busy} onCopy={() => go(true)} onReplace={() => go(false)} />
    </>
  );
}
