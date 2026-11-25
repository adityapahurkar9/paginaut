// Where the panes on the right sit (panes.ts, toolpanes.ts). A pane slides in when it opens (CSS, .signpane), and
// slides out when it closes: the pane just closed is kept a moment over the page's edge, the page already taking
// the room back. A pane that gives way to another one goes at once.
import type { ComponentChildren } from "preact";
import { useEffect, useRef } from "preact/hooks";

const LEAVE_MS = 160;

export function PaneSlot({ children }: { children: ComponentChildren }) {
  const slot = useRef<HTMLDivElement>(null);
  const ghost = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const s = slot.current, g = ghost.current;
    if (!s || !g) return;
    const mo = new MutationObserver((records) => {
      if (s.querySelector(":scope > aside")) return;
      let gone: Element | null = null;
      for (const r of records) for (const n of r.removedNodes) if (n instanceof HTMLElement && n.tagName === "ASIDE") gone = n;
      if (!gone || matchMedia("(prefers-reduced-motion: reduce)").matches) return;
      // The pane's own element, no longer Preact's: shown leaving, then dropped.
      g.replaceChildren(gone);
      gone.classList.add("pane-leaving");
      gone.setAttribute("aria-hidden", "true");
      (gone as HTMLElement).inert = true;
      setTimeout(() => gone!.remove(), LEAVE_MS);
    });
    mo.observe(s, { childList: true });
    return () => mo.disconnect();
  }, []);
  return (
    <>
      <div class="pane-slot" ref={slot}>
        {children}
      </div>
      <div class="pane-ghost" ref={ghost} />
    </>
  );
}
