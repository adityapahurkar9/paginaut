// Report a problem on Android (Settings › Report a problem, and offered after a crash): the error log and the phone's
// details, each part included only if left on, shown exactly as they'd be shared. Handed on through the share sheet
// or as a GitHub issue opened in the browser: Paginaut has no internet permission and sends nothing itself.
import { signal } from "@preact/signals";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useState } from "preact/hooks";
import { ArrowLeft } from "lucide-preact";
import { detailsHold, issueUrl, pageDetails, REPORT_HOLDS, reportText, type Rows } from "../errors";
import { native } from "./native";
import { back } from "./nav";
import { errorText, say } from "./state";

/** Set when the sheet was opened because the last session crashed. */
export const afterCrash = signal(false);

/** At start: true once when the last session crashed (Rust found the Kotlin crash or a panic in the log). */
export const crashedLastTime = () => invoke<boolean>("crash_notice").catch(() => false);

function Switch({ on, onChange, label, hint }: { on: boolean; onChange: (v: boolean) => void; label: string; hint: string }) {
  return (
    <button class="row toggle-row report-part" role="switch" aria-checked={on} onClick={() => onChange(!on)}>
      <span class="row-label">
        {label}
        <small>{hint}</small>
      </span>
      <span class={`switch ${on ? "on" : ""}`}>
        <span />
      </span>
    </button>
  );
}

export function ReportSheet() {
  const [details, setDetails] = useState<Rows | null>(null);
  const [log, setLog] = useState("");
  const [withDetails, setWithDetails] = useState(true);
  const [withLog, setWithLog] = useState(true);

  const load = async () => {
    try {
      const [rows, text] = await Promise.all([native.deviceDetails(), native.errorLog()]);
      setDetails([...rows, ...pageDetails()]);
      setLog(text);
    } catch (e) {
      say(errorText(e));
    }
  };
  useEffect(() => {
    void load();
    return () => (afterCrash.value = false);
  }, []);

  const parts = { details: withDetails ? details : null, log: withLog ? log : null };
  const text = details ? reportText(parts) : "Gathering details…";
  const nothing = !details || (!withDetails && !withLog);
  const lines = log.trim() ? log.trim().split("\n").filter((l) => !l.startsWith(" ")).length : 0;

  const share = () => native.shareReport(text).catch((e) => say(errorText(e)));
  const issue = () => {
    const { url, cut } = issueUrl(parts, "Android");
    openUrl(url).then(
      () => cut && say("The issue has the log's last lines. Share the report as a file to send the whole log."),
      (e) => say(errorText(e)),
    );
  };
  const clear = async () => {
    try {
      await native.clearErrorLog();
      await load();
      say("Cleared the error log");
    } catch (e) {
      say(errorText(e));
    }
  };

  return (
    <>
      <div class="sheet-title">
        <button class="ib" aria-label="Close Report a problem" onClick={() => back()}>
          <ArrowLeft size={22} />
        </button>
        <h3>Report a problem</h3>
      </div>
      <div class="sheet-scroll report-sheet">
        {afterCrash.value && <p class="report-crash">Paginaut closed unexpectedly last time. What went wrong is in the error log below. Nothing has been sent.</p>}
        <p class="hint">
          Paginaut keeps a log of errors on this phone. Sharing it is up to you: nothing leaves the phone unless you send it with one of the buttons below, to the app
          or page you pick. Paginaut itself has no internet access.
        </p>
        <Switch on={withDetails} onChange={setWithDetails} label="Include phone details" hint={detailsHold("Android")} />
        <Switch
          on={withLog}
          onChange={setWithLog}
          label={`Include the error log${details ? ` (${lines ? `${lines} ${lines === 1 ? "entry" : "entries"}` : "empty"})` : ""}`}
          hint={REPORT_HOLDS.log}
        />
        <p class="hint">{REPORT_HOLDS.never}</p>
        <pre class="report-text">{text}</pre>
        <button class="row link-row" disabled={nothing} onClick={() => void share()}>
          <span class="row-label">Share this report as a file…</span>
        </button>
        <button class="row link-row" disabled={nothing} onClick={issue}>
          <span class="row-label">Open a GitHub issue with this report</span>
        </button>
        <p class="hint">Opens github.com in your browser with the form filled in; you add what happened and post it yourself (a free GitHub account is needed). Issues there are public.</p>
        <button class="row link-row" disabled={!log.trim()} onClick={() => void clear()}>
          <span class="row-label">Clear the error log</span>
        </button>
      </div>
    </>
  );
}
