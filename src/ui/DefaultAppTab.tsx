// Settings › Default app: the file types Paginaut is offered for in Windows (Open with, Default apps), and where
// it's the default now. Windows 10 and 11 let only the user choose a default, so the button opens Windows' Default
// apps page at Paginaut. Office files and pictures stay with their own apps unless ticked. Only an installed copy
// offers itself (a development build or a copied folder shows what's set, read-only).
import { useEffect, useState } from "preact/hooks";
import { api, type DefaultApps, type DefaultKind } from "../api";
import { showToast } from "../store";

export function DefaultAppTab() {
  const [state, setState] = useState<DefaultApps | null>(null);
  const [busy, setBusy] = useState(false);
  // Read again when the window comes back (after choosing in Windows' Settings).
  useEffect(() => {
    const load = () => void api.defaultAppsStatus().then(setState).catch((e) => showToast(String(e), 5000));
    load();
    window.addEventListener("focus", load);
    return () => window.removeEventListener("focus", load);
  }, []);
  if (!state) return <h3>Default app</h3>;
  const { installed, kinds } = state;
  const offer = async (id: string, on: boolean) => {
    setBusy(true);
    try {
      const ids = kinds.filter((k) => (k.id === id ? on : k.offered)).map((k) => k.id);
      setState(await api.defaultAppsSet(ids));
    } catch (e) {
      showToast(String(e), 5000);
    } finally {
      setBusy(false);
    }
  };
  const offered = kinds.filter((k) => k.offered);
  // Withdrawing a type Paginaut opens now would make Windows reset that default: choose another app there first.
  const locked = (k: DefaultKind) => k.offered && k.defaultFor > 0;
  const row = (k: DefaultKind) => (
    <label
      key={k.id}
      class={`set-rail-row def-row ${k.offered ? "" : "off"}`}
      title={locked(k) ? "Paginaut opens these now. To stop offering it, choose another app for them in Windows' Default apps first." : undefined}
    >
      <input type="checkbox" checked={k.offered} disabled={!installed || busy || locked(k)} onChange={(e) => void offer(k.id, (e.target as HTMLInputElement).checked)} />
      <span>
        {k.name}
        <small> {k.exts.map((e) => `.${e}`).join(" ")}</small>
      </span>
      {k.defaultFor > 0 && <b class="def-badge">{k.defaultFor === k.exts.length ? "Default" : `Default for ${k.defaultFor} of ${k.exts.length}`}</b>}
    </label>
  );
  return (
    <>
      <h3>Default app</h3>
      <span class="hint">
        Tick the files Paginaut is offered for in Windows, in "Open with" and in Default apps. Windows lets only you choose the default: the button
        opens Windows' Default apps at Paginaut, where you pick it for each type.
      </span>
      {!installed && <span class="theme-warn">This copy of Paginaut isn't installed, so it can't offer itself in Windows. The installed Paginaut can.</span>}
      <div class="set-rail">{kinds.filter((k) => !k.others).map(row)}</div>
      <span class="hint">Office files and pictures stay with their own apps unless you tick them here.</span>
      <div class="set-rail">{kinds.filter((k) => k.others).map(row)}</div>
      <div class="theme-row">
        <button class="btn pri sm" disabled={!offered.length} onClick={() => void api.defaultAppsOpen().catch((e) => showToast(String(e), 5000))}>
          Make Paginaut the default…
        </button>
        {offered.length > 0 && offered.every((k) => k.defaultFor === k.exts.length) && <span class="hint">Paginaut opens every ticked type.</span>}
      </div>
    </>
  );
}
