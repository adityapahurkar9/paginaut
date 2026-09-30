// Digital signatures (DSC tokens and other certificates in the Windows store):
// signing a document, and checking the signatures a document has.
import { signal, type Signal } from "@preact/signals";
import { invoke } from "@tauri-apps/api/core";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { annotTool, registerBoxer } from "./annots";
import { openSignPane } from "./sign";
import type { DocInfo, Widget } from "./api";
import { adoptSaved, onDocReady, save } from "./pages";
import { onSignatureClick } from "./forms";
import { ask, baseName, settings, showToast, updateSettings, type Tab } from "./store";

export interface CertSummary {
  thumb: string;
  name: string;
  issuer: string;
  email: string;
  org: string;
  notBefore: number;
  notAfter: number;
  provider: string;
  hardware: boolean;
  current: boolean;
  /** Looks meant for signing documents (others are hidden until asked for). */
  likely: boolean;
}

export interface SigCheck {
  intact: boolean;
  signer: string;
  issuer: string;
  email: string;
  trust: "trusted" | "expired" | "untrusted" | "incomplete" | "revoked" | "invalid" | "unknown";
  detail: string;
  notAfter: number;
}

export interface SigInfo {
  i: number;
  name: string;
  page: number;
  signed: boolean;
  /** Something was saved after this signature (another signature, comments, form entries…). */
  changed: boolean;
  invalidAt: number;
  end: number;
  reason: string;
  location: string;
  m: string;
  sub: string;
  check?: SigCheck;
  /** Signing time (seconds since 1970), from the signature. */
  time?: number;
}

/** Everything about how the next signature is made, chosen in the dialog. */
export interface SignSetup {
  cert: CertSummary;
  reason: string;
  location: string;
  /** Saved handwritten signature (base64 PNG) to show in the box. */
  image: string | null;
  showDate: boolean;
  lock: boolean;
  /** "box": draw it on a page; "invisible": no box. */
  place: "box" | "invisible";
  /** Save as a new file (recommended) or sign this one in place. */
  newFile: boolean;
}

/** An empty signature field of a form you clicked: the next signature goes in it. */
export const signField = signal<{ tab: Tab; src: number; w: Widget } | null>(null);

// ---------------------------------------------------------------- checking

const lists = new WeakMap<Tab, Signal<SigInfo[] | null>>();
export function docSigs(tab: Tab): Signal<SigInfo[] | null> {
  let s = lists.get(tab);
  if (!s) {
    s = signal(null);
    lists.set(tab, s);
  }
  return s;
}

/** Reads and checks the document's signatures (in the background). */
export async function loadSignatures(tab: Tab) {
  const info = tab.info.value;
  if (!info?.isPdf) {
    docSigs(tab).value = [];
    return;
  }
  try {
    docSigs(tab).value = await invoke<SigInfo[]>("signatures", { doc: info.docId });
  } catch {
    docSigs(tab).value = [];
  }
}

export type Verdict = "valid" | "warn" | "invalid";

/** Adobe-style verdict for one signature. */
export function verdictOf(s: SigInfo): Verdict {
  const c = s.check;
  if (!c || !c.intact || c.trust === "invalid" || c.trust === "revoked") return "invalid";
  return c.trust === "trusted" || c.trust === "expired" ? "valid" : "warn";
}

export function describe(s: SigInfo): string {
  const c = s.check;
  if (!c) return "Not checked.";
  if (!c.intact) return c.detail || "The signature doesn't match the document.";
  const parts: string[] = [];
  parts.push(
    c.trust === "trusted" ? "Signature is valid: the document hasn't been altered since it was signed, and Windows trusts the certificate."
    : c.trust === "expired" ? "Signature is valid: the document hasn't been altered since it was signed. " + c.detail
    : "The document hasn't been altered since it was signed, but the signer's identity couldn't be confirmed. " + c.detail,
  );
  if (s.changed) parts.push("Changes were saved after this signature (for example more signatures, comments or form entries). Open the signed version to see exactly what was signed.");
  return parts.join(" ");
}

/** Opens the document exactly as signature `s` signed it, in a new tab. */
export async function openSignedVersion(tab: Tab, s: SigInfo) {
  const info = tab.info.value;
  if (!info) return;
  try {
    const path = await invoke<string>("signed_version", { doc: info.docId, i: s.i });
    const { openFiles } = await import("./store");
    await openFiles([path]);
  } catch (e) {
    showToast(String(e), 5000);
  }
}

// ---------------------------------------------------------------- signing

onDocReady((tab) => void loadSignatures(tab));

// A form's signature field: sign an empty one; show the details of a signed one.
onSignatureClick((tab, src, w) => {
  if (!w.signed) return startSigning(tab, { src, w });
  openSignPane("doc");
});

export async function listCerts(): Promise<CertSummary[]> {
  return invoke<CertSummary[]>("certs");
}

let pending: { tab: Tab; setup: SignSetup } | null = null;

/** Opens the Sign pane at the certificate section; with `field`, the next signature goes into that form field. */
export function startSigning(tab: Tab, field?: { src: number; w: Widget }) {
  signField.value = field ? { tab, ...field } : null;
  openSignPane("dsc");
}

/** The Sign button: saves unsaved changes first, then signs straight away (invisible, or a form's field) or lets you draw the box. */
export async function proceed(tab: Tab, setup: SignSetup) {
  const info = tab.info.value;
  if (!info?.isPdf) return showToast("Only PDFs can be signed. Save this file as a PDF first (Save as).", 4000);
  if (tab.dirty.value) {
    const r = await ask("Save before signing?", "A digital signature covers the document as it's saved, so your changes need saving first.", [
      { label: "Cancel", value: "cancel" },
      { label: "Save", value: "save", primary: true },
    ]);
    if (r !== "save" || !(await save(tab))) return;
  }
  const f = signField.value?.tab === tab ? signField.value : null;
  const field = f ? { src: f.src, w: f.w } : undefined;
  signField.value = null;
  rememberChoices(setup);
  if (field) return void finish(tab, setup, field.src, undefined, field.w.n);
  if (setup.place === "invisible") return void finish(tab, setup, 0);
  pending = { tab, setup };
  annotTool.value = "box:dsc";
  showToast("Drag a box on the page where the signature should go.", 4000);
}

registerBoxer("dsc", (tab, src, rect) => {
  const p = pending;
  pending = null;
  if (!p || p.tab !== tab) return;
  // A click instead of a drag: a box of the usual size there.
  let r = rect;
  if (r[2] - r[0] < 20 || r[3] - r[1] < 12) r = [rect[0] - 90, rect[1] - 30, rect[0] + 90, rect[1] + 30];
  void finish(tab, p.setup, src, r);
});

/** PDF_SIGNATURE_SHOW_* */
const SHOW_LABELS = 1, SHOW_DATE = 4, SHOW_TEXT_NAME = 8, SHOW_GRAPHIC_NAME = 16;

async function finish(tab: Tab, setup: SignSetup, src: number, rect?: number[], field?: number) {
  const info = tab.info.value;
  if (!info) return;
  let target: string | null = null;
  // A PDF made from an Office file lives in the app's cache: the signed one is always a new file.
  if (setup.newFile || info.converted) {
    const stem = baseName(tab.path).replace(/\.[^.]+$/, "");
    target = await saveDialog({
      title: "Save the signed document as",
      defaultPath: tab.path.replace(/[^\\/]+$/, `${stem} (signed).pdf`),
      filters: [{ name: "PDF", extensions: ["pdf"] }],
    });
    if (!target) return;
  }
  const flags = SHOW_LABELS | SHOW_TEXT_NAME | (setup.showDate ? SHOW_DATE : 0) | (setup.image ? 0 : SHOW_GRAPHIC_NAME);
  tab.saving.value = true;
  showToast(setup.cert.hardware ? "Signing… if your token asks for its PIN, type it in the token's window." : "Signing…", 60000);
  try {
    const saved = await invoke<DocInfo>("sign_document", {
      doc: info.docId,
      target,
      opts: {
        cert: setup.cert.thumb,
        page: src,
        rect: rect ?? null,
        field: field ?? null,
        reason: setup.reason,
        location: setup.location,
        image: setup.image,
        flags,
        lock: setup.lock,
      },
    });
    await adoptSaved(tab, saved);
    showToast(`Signed and saved ${baseName(saved.path)}`);
    await loadSignatures(tab);
  } catch (e) {
    showToast(String(e), 7000);
  } finally {
    tab.saving.value = false;
  }
}

// Reason and location are remembered for next time.
function rememberChoices(s: SignSetup) {
  updateSettings({ dsc: { reason: s.reason, location: s.location, showDate: s.showDate, lock: s.lock, newFile: s.newFile, place: s.place, cert: s.cert.thumb } });
}
export const dscDefaults = () => settings.value.dsc;
