// Report a problem (Settings › About, the command palette): the error log and this computer's details, shown exactly
// as they'd be handed over, to copy, save, or put in a new GitHub issue. Nothing is sent by Paginaut itself: the issue
// opens in the browser, where it can be read and changed before anyone posts it.
import { signal } from "@preact/signals";
import { invoke } from "@tauri-apps/api/core";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useState } from "preact/hooks";
import { issueUrl, pageDetails, reportText, type Rows } from "../errors";
import { showToast } from "../store";
import { toolPane } from "../toolpanes";
import { ToolFrame } from "./ToolFrame";

export const reportOpen = signal(false);
toolPane("report", reportOpen, { close: () => (reportOpen.value = false) });

export function openReport() {
  reportOpen.value = true;
}

export function ReportPane() {
  return reportOpen.value ? <ReportInner /> : null;
}

function ReportInner() {
  const [details, setDetails] = useState<Rows | null>(null);
  const [log, setLog] = useState("");
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

  const text = details ? reportText(details, log) : "";
  const lines = log.trim() ? log.trim().split("\n").filter((l) => !l.startsWith(" ")).length : 0;

  const copy = () =>
    navigator.clipboard.writeText(text).then(
      () => showToast("Copied the error log and system details"),
      (e) => setError(`Couldn't copy: ${e}`),
    );
  const save = async () => {
    const target = await saveDialog({ title: "Save the error log and system details as", defaultPath: "paginaut-report.txt", filters: [{ name: "Text", extensions: ["txt"] }] });
    if (!target) return;
    try {
      await invoke("report_save", { path: target, text });
      showToast("Saved the error log and system details");
    } catch (e) {
      setError(String(e));
    }
  };
  const issue = () => {
    if (!details) return;
    const { url, cut } = issueUrl(details, log);
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
        Paginaut keeps a log of errors on this computer: crashes, files that failed to open, save or convert, and errors in the app itself. File paths and document
        names are taken out before anything is written. Nothing is sent unless you send it.
      </span>

      <div class="tool-field">
        <label>What would be shared</label>
        <span class="hint">
          System details (version, system, processor, memory, graphics, web view, screen, language) and the error log
          {details ? ` (${lines ? `${lines} ${lines === 1 ? "entry" : "entries"}` : "empty"})` : ""}. Exactly this text:
        </span>
        <textarea class="report-text" readOnly value={details ? text : "Gathering details…"} spellcheck={false} />
      </div>

      {error && <span class="error-text">{error}</span>}

      <div class="report-actions">
        <button class="btn pri" disabled={!details} onClick={issue}>
          Open a GitHub issue with these details
        </button>
        <span class="hint">Opens github.com/adityapahurkar9/paginaut in your browser, filled in. You read it, add what happened, and post it yourself (a GitHub account is needed).</span>
        <button class="btn" disabled={!details} onClick={() => void copy()}>
          Copy error log and system details
        </button>
        <button class="btn" disabled={!details} onClick={() => void save()}>
          Save error log and system details…
        </button>
        <button class="btn ghost" disabled={!log.trim()} onClick={() => void clear()}>
          Clear the error log
        </button>
      </div>
    </ToolFrame>
  );
}
