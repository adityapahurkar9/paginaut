// Updates: checking Paginaut's releases (Help › Check for updates, Settings › About, and once a day when that's
// on), and installing a newer version: downloaded, its signature checked, then its installer runs and Paginaut
// restarts. Open documents with unsaved changes are saved or kept first.
import { signal } from "@preact/signals";
import { api, type UpdateChecked } from "./api";
import { ask, openSettings, persistPositions, settings, showToast, tabs, updateSettings } from "./store";

export type UpdateState =
  | { phase: "idle" }
  | { phase: "checking" }
  | { phase: "checked"; result: UpdateChecked; at: number }
  | { phase: "downloading"; done: number; total: number }
  | { phase: "error"; message: string };

export const updateState = signal<UpdateState>({ phase: "idle" });

const DAY = 24 * 60 * 60 * 1000;

/** Checks now. Quiet: the daily check, which only speaks up when there's a newer version. */
export async function checkForUpdates(quiet = false) {
  if (updateState.value.phase === "checking" || updateState.value.phase === "downloading") return;
  updateState.value = { phase: "checking" };
  try {
    const result = await api.updateCheck();
    updateState.value = { phase: "checked", result, at: Date.now() };
    updateSettings({ updates: { ...updatesSetting(), last: Date.now() } });
    if (quiet && result.available) showToast(`Paginaut ${result.available.version} is available: Help › Check for updates.`, 8000);
  } catch (e) {
    updateState.value = quiet ? { phase: "idle" } : { phase: "error", message: String(e) };
  }
}

export const updatesSetting = () => settings.value.updates ?? { auto: true };

export function setAutoUpdates(auto: boolean) {
  updateSettings({ updates: { ...updatesSetting(), auto } });
}

/** Help › Check for updates: the answer is shown in Settings › About. */
export function checkFromMenu() {
  openSettings("about");
  void checkForUpdates();
}

/** The tabs here with unsaved changes; says so and answers true when there are any. */
async function unsavedFirst(): Promise<boolean> {
  const unsaved = tabs.value.filter((t) => t.dirty.value);
  if (!unsaved.length) return false;
  const names = unsaved.map((t) => t.name).join(", ");
  await ask("Save your changes first", `Updating restarts Paginaut. Save or close these, then choose Install again: ${names}.`, [
    { label: "OK", value: "ok", primary: true },
  ]);
  return true;
}

export async function installUpdate() {
  const s = updateState.value;
  if (s.phase !== "checked" || !s.result.available) return;
  if (await unsavedFirst()) return;
  updateState.value = { phase: "downloading", done: 0, total: 0 };
  try {
    // (Nothing is downloaded twice: an installer already here and checked is used.)
    await api.updateInstall((e) => (updateState.value = { phase: "downloading", done: e.done, total: e.total }));
    // The download took a while, and documents may have been changed since: look again before anything quits.
    updateState.value = s;
    if (await unsavedFirst()) return;
    persistPositions();
    const quitting = await api.updateRun();
    if (!quitting) showToast("The new version opened in your software installer: install it there, then start Paginaut again.", 9000);
  } catch (e) {
    updateState.value = String(e) === "stopped" ? s : { phase: "error", message: String(e) };
  }
}

export function stopUpdate() {
  void api.updateStop();
}

/** Once a day, a little after starting, when automatic checks are on. */
window.setTimeout(() => {
  const u = updatesSetting();
  if (u.auto && Date.now() - (u.last ?? 0) > DAY) void checkForUpdates(true);
}, 20_000);
