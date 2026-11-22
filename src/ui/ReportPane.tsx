// Report a problem (Settings › About, the command palette, and offered after a crash): the error log and this
// computer's details, shown exactly as they'd be handed over, each part included only if the user leaves it ticked,
// to copy, save, or put in a new GitHub issue. Paginaut sends nothing itself: the issue opens in the browser, where
// it can be read and changed before anyone posts it.
import { signal } from "@preact/signals";
import { invoke } from "@tauri-apps/api/core";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useState } from "preact/hooks";
import { detailsHold, issueUrl, pageDetails, REPORT_HOLDS, reportText, type Rows } from "../errors";
import { ask, onLinux, showToast } from "../store";
import { toolPane } from "../toolpanes";
import { ToolFrame } from "./ToolFrame";

export const reportOpen = signal(false);
toolPane("report", reportOpen, { close: () => (reportOpen.value = false) });

export function openReport() {
  reportOpen.value = true;
}

/** At start: the last session crashed or ended without closing. Asked once; nothing is sent either way. */
export async function offerReportAfterCrash() {
  const crashed = await invoke<boolean>("crash_notice").catch(() => false);
  if (!crashed) return;
  const v = await ask(
    "Paginaut closed unexpectedly last time",
    "What went wrong is in the error log on this computer. You can look at it, and choose whether to share it to help fix the problem. Nothing has been sent.",
    [
      { label: "Not now", value: "no" },
      { label: "Report a problem…", value: "yes", primary: true },
    ],
  );
  if (v === "yes") openReport();
}

export function ReportPane() {
  return reportOpen.value ? <ReportInner /> : null;
}

function ReportInner() {
  const [details, setDetails] = useState<Rows | null>(null);
  const [log, setLog] = useState("");
  const [withDetails, setWithDetails] = useState(true);
  const [withLog, setWithLog] = useState(true);
  const [error, setError] = useState("");

  const load = async () => {
    try {
      const [sys, text] = await Promise.all([invoke<Rows>("system_details"), invoke<string>("error_log")]);
      setDetails([...sys, ...pageDetails()]);
      setLog(text);
    } catch (e) {
      setError(String(e));
    }
  };
  useEffect(() => void load(), []);

  const parts = { details: withDetails ? details : null, log: withLog ? log : null };
  const text = details ? reportText(parts) : "";
  const nothing = !withDetails && !withLog;
  const lines = log.trim() ? log.trim().split("\n").filter((l) => !l.startsWith(" ")).length : 0;

  const copy = () =>
    navigator.clipboard.writeText(text).then(
      () => showToast("Copied the report"),
      (e) => setError(`Couldn't copy: ${e}`),
    );
  const save = async () => {
    const target = await saveDialog({ title: "Save the report as", defaultPath: "paginaut-report.txt", filters: [{ name: "Text", extensions: ["txt"] }] });
    if (!target) return;
    try {
      await invoke("report_save", { path: target, text });
      showToast("Saved the report");
    } catch (e) {
      setError(String(e));
    }
  };
  const issue = () => {
    if (!details) return;
    const { url, cut } = issueUrl(parts, onLinux ? "Linux" : "Windows");
    openUrl(url).then(
      () => cut && showToast("The issue has the log's last lines. Save the report and attach it for the whole log.", 6000),
      (e) => setError(`Couldn't open the browser: ${e}`),
    );
  };
  const clear = async () => {
    await invoke("error_log_clear").catch((e) => setError(String(e)));
    await load();
    showToast("Cleared the error log");
  };

  return (
    <ToolFrame id="report" title="Report a problem" class="report-pane">
      <span class="muted">
        Paginaut keeps a log of errors on this computer. Sharing it is up to you: nothing is sent unless you choose to send it below, and Paginaut itself
        never sends it anywhere.
      </span>

      <div class="tool-field">
        <label>What to include</label>
        <label class="check-row">
          <input type="checkbox" checked={withDetails} onChange={(e) => setWithDetails((e.target as HTMLInputElement).checked)} />
          <span>
            System details
            <small>{detailsHold(onLinux ? "Linux" : "Windows")}</small>
          </span>
        </label>
        <label class="check-row">
          <input type="checkbox" checked={withLog} onChange={(e) => setWithLog((e.target as HTMLInputElement).checked)} />
          <span>
            Error log {details ? `(${lines ? `${lines} ${lines === 1 ? "entry" : "entries"}` : "empty"})` : ""}
            <small>{REPORT_HOLDS.log}</small>
          </span>
        </label>
        <span class="hint">{REPORT_HOLDS.never}</span>
      </div>

      <div class="tool-field">
        <label>Exactly what would be shared</label>
        <textarea class="report-text" readOnly value={details ? text : "Gathering details…"} spellcheck={false} />
      </div>

      {error && <span class="error-text">{error}</span>}

      <div class="report-actions">
        <button class="btn pri" disabled={!details || nothing} onClick={issue}>
          Open a GitHub issue with this report
        </button>
        <span class="hint">
          Opens github.com/adityapahurkar9/paginaut in your browser with the form filled in. You add what happened, can change anything, and post it yourself
          (a free GitHub account is needed). Issues there are public.
        </span>
        <button class="btn" disabled={!details || nothing} onClick={() => void copy()}>
          Copy this report
        </button>
        <button class="btn" disabled={!details || nothing} onClick={() => void save()}>
          Save this report as a text file…
        </button>
        <button class="btn ghost" disabled={!log.trim()} onClick={() => void clear()}>
          Clear the error log
        </button>
      </div>
    </ToolFrame>
  );
}
