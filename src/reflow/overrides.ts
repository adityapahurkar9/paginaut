// Your corrections to the reading layout, remembered per file.
import { signal, type Signal } from "@preact/signals";
import type { ItemKind, LayoutOverrides, PageOverride } from "./model";
import type { Box } from "./model";
import type { Tab } from "../store";

const storageKey = (path: string) => `paginaut.layout.${path.toLowerCase()}`;
const bySignal = new WeakMap<Tab, { path: string; ov: Signal<LayoutOverrides> }>();

/** The tab's corrections (a signal: Read/Book rebuild when it changes). */
export function overridesOf(tab: Tab): Signal<LayoutOverrides> {
  const cur = bySignal.get(tab);
  if (cur && cur.path === tab.path) return cur.ov;
  let v: LayoutOverrides = {};
  try {
    v = JSON.parse(localStorage.getItem(storageKey(tab.path)) || "{}");
  } catch {
    /* none */
  }
  const ov = signal<LayoutOverrides>(v);
  bySignal.set(tab, { path: tab.path, ov });
  return ov;
}

function store(tab: Tab, next: LayoutOverrides) {
  overridesOf(tab).value = next;
  try {
    const empty = !next.columns && !Object.keys(next.pages ?? {}).length;
    if (empty) localStorage.removeItem(storageKey(tab.path));
    else localStorage.setItem(storageKey(tab.path), JSON.stringify(next));
  } catch {
    /* not persisted */
  }
}

/** Changes one page's corrections; an empty correction is removed. */
export function updatePage(tab: Tab, src: number, patch: (p: PageOverride) => PageOverride) {
  const ov = overridesOf(tab).value;
  const pages = { ...(ov.pages ?? {}) };
  const next = patch({ ...(pages[src] ?? {}) });
  const empty = !next.columns && !next.order?.length && !Object.keys(next.kinds ?? {}).length && !next.regions?.length;
  if (empty) delete pages[src];
  else pages[src] = next;
  store(tab, { ...ov, pages });
}

export function setDocColumns(tab: Tab, columns: number) {
  store(tab, { ...overridesOf(tab).value, columns: columns || undefined });
}

export function setKind(tab: Tab, src: number, key: string, kind: ItemKind | null) {
  updatePage(tab, src, (p) => {
    const kinds = { ...(p.kinds ?? {}) };
    if (kind) kinds[key] = kind;
    else delete kinds[key];
    return { ...p, kinds };
  });
}

export function addRegion(tab: Tab, src: number, box: Box, kind: ItemKind) {
  updatePage(tab, src, (p) => ({ ...p, regions: [...(p.regions ?? []), { box, kind }] }));
}

export function resetPage(tab: Tab, src: number) {
  updatePage(tab, src, () => ({}));
}
