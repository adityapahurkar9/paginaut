// The Sign pane's "Digital signature" section: pick a certificate (DSC token or
// other), reason, location, what the box shows, where the signed file goes.
import { useEffect, useState } from "preact/hooks";
import { KeyRound, RefreshCw, Usb, X } from "lucide-preact";
import { dscDefaults, listCerts, proceed, signField, type CertSummary, type SignSetup } from "../dsc";
import { sigs } from "../sign";
import type { Tab } from "../store";

const REASONS = [
  "I approve this document",
  "I am the author of this document",
  "I have reviewed this document",
  "I agree to the terms defined by this document",
];

const day = (t: number) => new Date(t * 1000).toLocaleDateString();

// The list is read once per session (and on Refresh): plugging in a token needs a Refresh.
let cached: CertSummary[] | null = null;

export function CertSection({ tab }: { tab: Tab | null }) {
  const def = dscDefaults();
  const [certs, setCerts] = useState<CertSummary[] | null>(cached);
  const [err, setErr] = useState("");
  const [cert, setCert] = useState<string>(def.cert);
  const [reason, setReason] = useState(def.reason);
  const [other, setOther] = useState(!REASONS.includes(def.reason) && def.reason !== "");
  const [location, setLocation] = useState(def.location);
  const [image, setImage] = useState<string>("");
  const [showDate, setShowDate] = useState(def.showDate);
  const [lock, setLock] = useState(def.lock);
  const [place, setPlace] = useState(def.place);
  const [newFile, setNewFile] = useState(def.newFile);
  const [showAll, setShowAll] = useState(false);

  const pickDefault = (list: CertSummary[]) => {
    // Keep the last one used if it's still here, else the first usable one.
    const usable = list.filter((c) => c.current && c.likely);
    if (!usable.some((c) => c.thumb === cert)) setCert(usable[0]?.thumb ?? "");
  };
  const refresh = () => {
    setCerts(null);
    setErr("");
    listCerts()
      .then((list) => {
        cached = list;
        setCerts(list);
        pickDefault(list);
      })
      .catch((e) => {
        setCerts([]);
        setErr(String(e));
      });
  };
  useEffect(() => {
    if (cached) pickDefault(cached);
    else refresh();
  }, []);

  const shown = (certs ?? []).filter((c) => showAll || c.likely);
  const chosen = shown.find((c) => c.thumb === cert && c.current) ?? null;
  const field = signField.value?.tab === tab ? signField.value : null;
  const fields = tab?.forms.value?.fields ?? 0;
  const isPdf = !!tab?.info.value?.isPdf;
  const go = () => {
    if (!chosen || !tab) return;
    const setup: SignSetup = { cert: chosen, reason: reason.trim(), location: location.trim(), image: image || null, showDate, lock, place, newFile };
    void proceed(tab, setup);
  };
  const saved = (sigs.value ?? []).filter((s) => s.kind === "signature");

  return (
    <div class="cert-sec">
      <div class="cert-head">
        <span class="muted">Your certificates</span>
        <button class="btn ghost sm" onClick={refresh} title="Plugged in a token? Look again">
          <RefreshCw size={13} /> Refresh
        </button>
      </div>
      <div class="cert-list" role="radiogroup" aria-label="Certificate">
        {certs === null ? (
          <div class="muted cert-empty">Looking for certificates…</div>
        ) : !shown.length ? (
          <div class="cert-empty">
            <strong>No signing certificate found.</strong>
            <span class="muted">Plug in your DSC token (its driver puts the certificate into Windows), then press Refresh. {err}</span>
          </div>
        ) : (
          shown.map((c) => (
            <button
              key={c.thumb}
              role="radio"
              aria-checked={cert === c.thumb}
              disabled={!c.current}
              class={`cert ${cert === c.thumb && c.current ? "on" : ""}`}
              onClick={() => setCert(c.thumb)}
              title={`${c.provider}\nThumbprint ${c.thumb}`}
            >
              <span class="cert-ic">{c.hardware ? <Usb size={16} /> : <KeyRound size={16} />}</span>
              <span class="cert-txt">
                <strong>{c.name}</strong>
                <span class="muted">{c.issuer}</span>
                <span class={c.current ? "muted" : "bad"}>
                  {c.current ? `Until ${day(c.notAfter)}` : c.notAfter * 1000 < Date.now() ? `Expired ${day(c.notAfter)}` : `From ${day(c.notBefore)}`}
                  {c.hardware ? " · token" : " · this PC"}
                </span>
              </span>
            </button>
          ))
        )}
      </div>
      {certs && certs.some((c) => !c.likely) && (
        <button class="btn ghost sm cert-more" onClick={() => setShowAll(!showAll)}>
          {showAll ? "Hide other certificates" : `Show ${certs.filter((c) => !c.likely).length} other certificate(s)`}
        </button>
      )}

      <label class="fld">
        <span>Reason</span>
        <select
          class="inp"
          value={other ? "__other" : reason}
          onChange={(e) => {
            const v = e.currentTarget.value;
            setOther(v === "__other");
            setReason(v === "__other" ? "" : v);
          }}
        >
          {REASONS.map((r) => <option key={r} value={r}>{r}</option>)}
          <option value="">(no reason)</option>
          <option value="__other">Other…</option>
        </select>
      </label>
      {other && <input class="inp" value={reason} placeholder="Your reason" aria-label="Your reason" onInput={(e) => setReason(e.currentTarget.value)} />}
      <label class="fld">
        <span>Location</span>
        <input class="inp" value={location} placeholder="e.g. Pune" onInput={(e) => setLocation(e.currentTarget.value)} />
      </label>
      <label class="fld">
        <span>The box shows</span>
        <select class="inp" value={image} onChange={(e) => setImage(e.currentTarget.value)}>
          <option value="">Your name, in large type</option>
          {saved.map((s) => <option key={s.id} value={s.png}>{s.name} (handwritten)</option>)}
        </select>
      </label>
      <label class="chk">
        <input type="checkbox" checked={showDate} onChange={(e) => setShowDate(e.currentTarget.checked)} /> Show the date
      </label>
      {field ? (
        <div class="cert-field">
          <span>Goes in the form's field “{field.w.name}” on page {field.src + 1}.</span>
          <button class="ib sm" aria-label="Don't use that field" title="Don't use that field" onClick={() => (signField.value = null)}>
            <X size={14} />
          </button>
        </div>
      ) : (
        <div class="seg">
          <button class={`segb ${place === "box" ? "on" : ""}`} onClick={() => setPlace("box")}>Box on the page</button>
          <button class={`segb ${place === "invisible" ? "on" : ""}`} onClick={() => setPlace("invisible")}>Invisible</button>
        </div>
      )}
      <div class="seg">
        <button class={`segb ${newFile ? "on" : ""}`} onClick={() => setNewFile(true)}>Save as new file</button>
        <button class={`segb ${!newFile ? "on" : ""}`} onClick={() => setNewFile(false)}>Into this file</button>
      </div>
      {fields > 0 && !field && (
        <label class="chk wrap">
          <input type="checkbox" checked={lock} onChange={(e) => setLock(e.currentTarget.checked)} /> Lock the form after signing
        </label>
      )}
      <button class="btn pri" disabled={!chosen || !isPdf} onClick={go}>
        {field || place === "invisible" ? "Sign" : "Sign: draw the box next"}
      </button>
      {!isPdf && <p class="muted">Open a PDF to sign it.</p>}
      {chosen?.hardware && <p class="muted">Your token asks for its PIN in its own window. Paginaut never sees it.</p>}
    </div>
  );
}
