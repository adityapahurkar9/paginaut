// A signed document: a bar at the top with the overall verdict, and the Sign
// pane's "Signatures in this document" section with each signature's details.
import { useState } from "preact/hooks";
import { ShieldAlert, ShieldCheck, ShieldX, X } from "lucide-preact";
import * as A from "../actions";
import { checkRevocation, describe, docSigs, openSignedVersion, verdictOf, type Revocation, type SigInfo, type Verdict } from "../dsc";
import { openSignPane } from "../sign";
import type { Tab } from "../store";

const ICON: Record<Verdict, typeof ShieldCheck> = { valid: ShieldCheck, warn: ShieldAlert, invalid: ShieldX };

function overall(list: SigInfo[]): Verdict {
  const v = list.map(verdictOf);
  return v.includes("invalid") ? "invalid" : v.includes("warn") ? "warn" : "valid";
}

const dismissed = new WeakSet<Tab>();

export function SigBar({ tab }: { tab: Tab }) {
  const [, redraw] = useState(0);
  const all = docSigs(tab).value;
  const signed = all?.filter((s) => s.signed) ?? [];
  if (!signed.length || dismissed.has(tab)) return null;
  const v = overall(signed);
  const Icon = ICON[v];
  const names = [...new Set(signed.map((s) => s.check?.signer || "unknown"))];
  const who = names.length === 1 ? names[0] : `${names.length} people`;
  const text =
    v === "valid" ? `Signed by ${who}. ${signed.length > 1 ? "All signatures are" : "The signature is"} valid.`
    : v === "warn" ? `Signed by ${who}. Unchanged since signing, but the signer's certificate couldn't be confirmed.`
    : `Signed by ${who}. At least one signature is invalid.`;
  return (
    <div class={`formbar sigbar ${v}`}>
      <Icon size={16} class="sig-ic" />
      <span>{text}</span>
      <button class="btn ghost" onClick={() => openSignPane("doc")}>
        Details
      </button>
      <button
        class="ib sm"
        aria-label="Dismiss"
        onClick={() => {
          dismissed.add(tab);
          redraw((n) => n + 1);
        }}
      >
        <X size={14} />
      </button>
    </div>
  );
}

const when = (s: SigInfo) => (s.time ? new Date(s.time * 1000).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" }) : "");

// Answers from "Check whether it was revoked", per signature (by tab), kept while Paginaut runs.
const revoked = new WeakMap<Tab, Map<number, Revocation | "checking">>();

export function DocSigs({ tab }: { tab: Tab }) {
  const [, redraw] = useState(0);
  const checks = revoked.get(tab) ?? new Map<number, Revocation | "checking">();
  revoked.set(tab, checks);
  const revocation = async (s: SigInfo) => {
    checks.set(s.i, "checking");
    redraw((n) => n + 1);
    const r = await checkRevocation(tab, s);
    if (r) checks.set(s.i, r);
    else checks.delete(s.i);
    redraw((n) => n + 1);
  };
  const list = docSigs(tab).value;
  if (list === null) return <div class="muted">Checking signatures…</div>;
  const signed = list.filter((s) => s.signed);
  const empty = list.filter((s) => !s.signed);
  return (
    <div class="sig-cards">
      {!signed.length && <div class="muted">No digital signatures.</div>}
      {signed.map((s) => {
        const v = verdictOf(s);
        const Icon = ICON[v];
        return (
          <div key={s.i} class={`sig-card ${v}`}>
            <div class="sig-card-head">
              <Icon size={18} class="sig-ic" />
              <div class="grow">
                <strong>{s.check?.signer || "Unknown signer"}</strong>
                <div class="muted">{when(s)}</div>
              </div>
            </div>
            <p>{describe(s)}</p>
            <dl>
              {s.reason && (<><dt>Reason</dt><dd>{s.reason}</dd></>)}
              {s.location && (<><dt>Location</dt><dd>{s.location}</dd></>)}
              {s.check?.issuer && (<><dt>Issued by</dt><dd>{s.check.issuer}</dd></>)}
              {s.check?.email && (<><dt>Email</dt><dd>{s.check.email}</dd></>)}
              {!!s.check?.timestamp && (
                <>
                  <dt>Timestamp</dt>
                  <dd>
                    {new Date(s.check.timestamp * 1000).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" })}
                    {s.check.tsa ? `, by ${s.check.tsa}` : ""}
                    {s.check.timestampTrusted ? "" : " (not trusted)"}
                  </dd>
                </>
              )}
              <dt>Field</dt>
              <dd>{s.name}{s.page >= 0 ? `, page ${s.page + 1}` : " (invisible)"}</dd>
              {(() => {
                const r = checks.get(s.i);
                if (!r || r === "checking") return null;
                return (
                  <>
                    <dt>Revoked?</dt>
                    <dd class={r.status === "revoked" ? "bad" : ""}>{r.status === "good" ? "No. " : r.status === "revoked" ? "Yes. " : "Couldn't tell. "}{r.detail}</dd>
                  </>
                );
              })()}
            </dl>
            <div class="sig-card-btns">
              {s.page >= 0 && (
                <button class="btn ghost sm" onClick={() => A.goToSource(s.page)}>
                  Go to it
                </button>
              )}
              {s.changed && (
                <button class="btn ghost sm" onClick={() => void openSignedVersion(tab, s)} title="The document exactly as this signature signed it, in a new tab">
                  Open signed version
                </button>
              )}
              {s.check?.intact && (
                <button
                  class="btn ghost sm"
                  disabled={checks.get(s.i) === "checking"}
                  onClick={() => void revocation(s)}
                  title="Asks the certificate's authority, online, whether it has cancelled the certificate"
                >
                  {checks.get(s.i) === "checking" ? "Checking…" : "Check whether it was revoked"}
                </button>
              )}
            </div>
          </div>
        );
      })}
      {empty.length > 0 && (
        <div class="sig-empty-fields">
          <div class="muted">Empty signature fields</div>
          {empty.map((s) => (
            <button key={s.i} class="btn ghost sm" onClick={() => (s.page >= 0 ? A.goToSource(s.page) : undefined)}>
              {s.name}{s.page >= 0 ? `, page ${s.page + 1}` : ""}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
