// The panes on the right (Organize, Sign, Convert, Markdown, Reading): one open at a time, each with a button on the side
// bar at the window's right edge. Which buttons the side bar shows, and in what order, is up to the user.
import { signal } from "@preact/signals";
import { settings, updateSettings } from "./store";

export type PaneId = "organize" | "sign" | "convert" | "markdown" | "reading";

/** Every pane, in the side bar's default order. */
export const PANE_IDS: PaneId[] = ["organize", "sign", "convert", "markdown", "reading"];

/** The pane open on the right, if any. */
export const pane = signal<PaneId | null>(null);

export function openPane(id: PaneId) {
  pane.value = id;
}

export function closePane(id?: PaneId) {
  if (!id || pane.value === id) pane.value = null;
}

export function togglePane(id: PaneId) {
  pane.value = pane.value === id ? null : id;
}

/** The side bar's buttons, in order (a pane added in a later version joins at its default place). */
export function railItems(): PaneId[] {
  const saved = settings.value.rail;
  if (!saved) return PANE_IDS;
  const known = saved.order.filter((id): id is PaneId => PANE_IDS.includes(id as PaneId));
  for (const id of PANE_IDS) if (!known.includes(id)) known.splice(PANE_IDS.indexOf(id), 0, id);
  return known.filter((id) => !saved.hidden.includes(id));
}

export function railOrder(): PaneId[] {
  const saved = settings.value.rail;
  const known = (saved?.order ?? []).filter((id): id is PaneId => PANE_IDS.includes(id as PaneId));
  for (const id of PANE_IDS) if (!known.includes(id)) known.splice(PANE_IDS.indexOf(id), 0, id);
  return known;
}

export const railShown = () => settings.value.rail?.visible !== false;
export const onRail = (id: PaneId) => !(settings.value.rail?.hidden ?? []).includes(id);

function saveRail(patch: Partial<NonNullable<typeof settings.value.rail>>) {
  const cur = settings.value.rail ?? { order: PANE_IDS, hidden: [], visible: true };
  updateSettings({ rail: { ...cur, ...patch } });
}

export function setOnRail(id: PaneId, on: boolean) {
  const hidden = (settings.value.rail?.hidden ?? []).filter((h) => h !== id);
  saveRail({ hidden: on ? hidden : [...hidden, id] });
}

export function setRailShown(on: boolean) {
  saveRail({ visible: on });
}

export function moveOnRail(id: PaneId, by: -1 | 1) {
  const order = railOrder();
  const i = order.indexOf(id);
  const j = i + by;
  if (i < 0 || j < 0 || j >= order.length) return;
  [order[i], order[j]] = [order[j], order[i]];
  saveRail({ order });
}

export function resetRail() {
  updateSettings({ rail: undefined });
}
