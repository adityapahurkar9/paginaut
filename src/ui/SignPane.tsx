// The Sign pane (right side): your saved signatures and initials, Fill & Sign
// marks, signing with a certificate (DSC token), and the signatures a document has.
import { useEffect, useRef } from "preact/hooks";
import type { ComponentChild } from "preact";
import { CalendarDays, Check, FileLock2, Plus, Strikethrough, TypeOutline, UserRound, X } from "lucide-preact";
import { annotTool, selectedAnnot, type AnnotTool } from "../annots";
import { bookScreen } from "../modes";
import * as P from "../pages";
import { creating, deleteSig, loadSigs, paneFocus, pickSig, sigs, sigsOf, type SigEntry, type SigKind } from "../sign";
import { activeTab, ask, settings, showToast, updateSettings, type Tab } from "../store";
import { CertSection } from "./CertSection";
import { Maker } from "./SignMaker";
import { DocSigs } from "./SigPanel";

/** Inner width of the pane (its CSS width less padding), for the drawing pad. */
const INNER = 292;

export function SignPane() {
  const tab = activeTab.value;
  const ready = tab?.status.value === "ready" ? tab : null;
  const body = useRef<HTMLDivElement>(null);
  const focus = paneFocus.value;
  const open = settings.value.signPane && !bookScreen.value;
  useEffect(() => {
    if (!sigs.value) void loadSigs();
  }, []);
  // Bring the asked-for section into view.
  useEffect(() => {
    if (!open || !focus) return;
    requestAnimationFrame(() => body.current?.querySelector(`#pane-${focus.section}`)?.scrollIntoView({ block: "start", behavior: "smooth" }));
  }, [focus?.n, open]);
  if (!open) return null;
  return (
    <aside class="signpane" aria-label="Sign">
      <div class="signpane-head">
        <strong>Sign</strong>
        <button class="ib sm" aria-label="Close the Sign pane" title="Close" onClick={() => updateSettings({ signPane: false })}>
          <X size={15} />
        </button>
      </div>
      <div class="signpane-body" ref={body}>
        <Section id="mine" title="My signatures">
          <Library kind="signature" tab={ready} />
          <Library kind="initials" tab={ready} />
          <p class="muted small">Kept on this PC, encrypted for your Windows account.</p>
        </Section>
        <Section id="marks" title="Fill & Sign">
          <Marks tab={ready} />
        </Section>
        <Section id="dsc" title="Digital signature (DSC)">
          <CertSection key={ready?.key ?? "none"} tab={ready} />
        </Section>
        {ready?.info.value?.isPdf && (
          <Section id="doc" title="Signatures in this document">
            <DocSigs key={ready.key} tab={ready} />
          </Section>
        )}
        {ready?.info.value?.isPdf && (
          <button
            class="btn locked"
            title="A copy with signatures, comments and form entries made part of the page, so they can't be moved or edited"
            onClick={() => void P.saveFlattened(ready, true)}
          >
            <FileLock2 size={15} /> Save locked copy…
          </button>
        )}
      </div>
    </aside>
  );
}

function Section({ id, title, children }: { id: string; title: string; children: ComponentChild }) {
  return (
    <section class="pane-sec" id={`pane-${id}`}>
      <h3>{title}</h3>
      {children}
    </section>
  );
}

// ---------------------------------------------------------------- saved signatures

function Library({ kind, tab }: { kind: SigKind; tab: Tab | null }) {
  const items = sigsOf(kind);
  const tool = annotTool.value;
  const making = creating.value === kind;
  const label = kind === "initials" ? "Initials" : "Signatures";
  const remove = async (e: SigEntry) => {
    const r = await ask(`Delete ${e.name}?`, "It's removed from this PC. Signatures already placed in documents stay.", [
      { label: "Cancel", value: "cancel" },
      { label: "Delete", value: "delete", primary: true },
    ]);
    if (r === "delete") void deleteSig(e.id).catch((x) => showToast(String(x)));
  };
  return (
    <div class="lib">
      <div class="lib-head">
        <span>{label}</span>
        {!making && (
          <button class="btn ghost sm" onClick={() => (creating.value = kind)}>
            <Plus size={13} /> New
          </button>
        )}
      </div>
      {making && <Maker key={kind} kind={kind} width={INNER} />}
      {items.length > 0 && (
        <div class={`lib-grid ${kind}`}>
          {items.map((e) => (
            <div key={e.id} class={`lib-item ${tool === `sig:${e.id}` ? "on" : ""}`}>
              <button
                class="lib-pick"
                disabled={!tab}
                title={tab ? `Place ${e.name}: click on the page` : "Open a document to place it"}
                onClick={() => {
                  selectedAnnot.value = null;
                  pickSig(e.id);
                }}
              >
                <img src={`data:image/png;base64,${e.png}`} alt={e.name} />
              </button>
              <button class="lib-del" aria-label={`Delete ${e.name}`} title="Delete" onClick={() => void remove(e)}>
                <X size={12} />
              </button>
            </div>
          ))}
        </div>
      )}
      {!items.length && !making && <p class="muted small">None yet. Draw one, type it, or use a photo.</p>}
    </div>
  );
}

// ---------------------------------------------------------------- Fill & Sign marks

const MARKS: [AnnotTool, string, ComponentChild][] = [
  ["text", "Text", <TypeOutline size={16} />],
  ["stamp:check", "Tick", <Check size={16} />],
  ["stamp:cross", "Cross", <X size={16} />],
  ["stamp:dot", "Dot", <span class="dot-ic" />],
  ["fs-strike", "Strike", <Strikethrough size={16} />],
  ["stamp:date", "Date", <CalendarDays size={16} />],
  ["stamp:name", "Name", <UserRound size={16} />],
];

function Marks({ tab }: { tab: Tab | null }) {
  const tool = annotTool.value;
  return (
    <div class="marks">
      {MARKS.map(([t, label, icon]) => (
        <button
          key={t}
          class={`mark ${tool === t ? "on" : ""}`}
          disabled={!tab}
          onClick={() => {
            annotTool.value = tool === t ? null : t;
            selectedAnnot.value = null;
          }}
        >
          {icon}
          <span>{label}</span>
        </button>
      ))}
    </div>
  );
}
