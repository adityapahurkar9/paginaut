// Making a signature or initials in the Sign pane: draw, type, or a photo of
// one on paper.
import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import { invoke } from "@tauri-apps/api/core";
import { ImageUp } from "lucide-preact";
import { authorName } from "../annots";
import { addSig, creating, cropToInk, droppedPicture, INK, inkFromPhoto, pickSig, SCRIPT_FONTS, sigs, typedCanvas, type SigKind } from "../sign";
import { showToast } from "../store";

// ---------------------------------------------------------------- create

type How = "draw" | "type" | "photo";

function initialsOf(name: string) {
  return name
    .split(/\s+/)
    .filter(Boolean)
    .map((w) => w[0]!.toUpperCase())
    .join("")
    .slice(0, 3);
}

const close = () => {
  creating.value = null;
  droppedPicture.value = null;
};

/** `width`: the space it has, in px. */
export function Maker({ kind, width }: { kind: SigKind; width: number }) {
  const [how, setHow] = useState<How>("draw");
  const [ink, setInk] = useState<string>(INK[0][0]);
  const [busy, setBusy] = useState(false);
  // Each way of making one hands back its picture on demand.
  const make = useRef<() => HTMLCanvasElement | null>(() => null);
  const [ready, setReady] = useState(false);
  const label = kind === "initials" ? "initials" : "signature";
  // A picture dropped from Explorer switches to Photo, which then reads it.
  const dropped = droppedPicture.value;
  useEffect(() => {
    if (dropped && how !== "photo") {
      setReady(false);
      setHow("photo");
    }
  }, [dropped]);

  const save = async () => {
    const c = make.current();
    const png = c && cropToInk(c);
    if (!png) return showToast(how === "photo" ? "No ink found in the picture." : `Make your ${label} first.`);
    setBusy(true);
    try {
      const n = (sigs.value ?? []).filter((e) => e.kind === kind).length + 1;
      const e = await addSig(kind, `${kind === "initials" ? "Initials" : "Signature"} ${n}`, png);
      close();
      pickSig(e.id);
      showToast(`Click on the page to place your ${label}.`);
    } catch (x) {
      showToast(String(x));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div class="maker">
      <div class="sig-top">
        <div class="seg" role="tablist">
          {(["draw", "type", "photo"] as How[]).map((h) => (
            <button key={h} role="tab" aria-selected={how === h} class={`segb ${how === h ? "on" : ""}`} onClick={() => { setReady(false); setHow(h); }}>
              {h === "draw" ? "Draw" : h === "type" ? "Type" : "Photo"}
            </button>
          ))}
        </div>
        <div class="grow" />
        <div class="sig-inks" role="radiogroup" aria-label="Ink colour">
          {INK.map(([c, name]) => (
            <button key={c} role="radio" aria-checked={ink === c} class={`sw ${ink === c ? "on" : ""}`} style={{ background: c }} title={name} aria-label={name} onClick={() => setInk(c)} />
          ))}
          {how === "photo" && (
            <button role="radio" aria-checked={ink === ""} class={`sw orig ${ink === "" ? "on" : ""}`} title="Keep the photo's own colour" aria-label="Original colour" onClick={() => setInk("")} />
          )}
        </div>
      </div>
      {how === "draw" && <Draw ink={ink || INK[0][0]} kind={kind} width={width} make={make} onChange={setReady} />}
      {how === "type" && <Type ink={ink || INK[0][0]} kind={kind} width={width} make={make} onChange={setReady} />}
      {how === "photo" && <Photo ink={ink || null} width={width} make={make} onChange={setReady} />}
      <div class="row">
        <button class="btn" onClick={close}>
          Cancel
        </button>
        <button class="btn pri" disabled={!ready || busy} onClick={() => void save()}>
          Save and place
        </button>
      </div>
    </div>
  );
}

type Part = { ink: string; kind: SigKind; width: number; make: { current: () => HTMLCanvasElement | null }; onChange: (ready: boolean) => void };

// ---------------------------------------------------------------- draw

type Pt = { x: number; y: number; w: number };

function Draw({ ink, kind, width, make, onChange }: Part) {
  const ref = useRef<HTMLCanvasElement>(null);
  const strokes = useRef<Pt[][]>([]);
  const W = width, H = Math.round(width * (kind === "initials" ? 0.42 : 0.4));
  const base = kind === "initials" ? 3.4 : 2.8;

  const paint = (c: HTMLCanvasElement, scale: number) => {
    const ctx = c.getContext("2d")!;
    ctx.setTransform(scale, 0, 0, scale, 0, 0);
    ctx.clearRect(0, 0, W, H);
    ctx.fillStyle = ink;
    ctx.strokeStyle = ink;
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    for (const s of strokes.current) {
      if (s.length === 1) {
        ctx.beginPath();
        ctx.arc(s[0].x, s[0].y, s[0].w / 2, 0, Math.PI * 2);
        ctx.fill();
        continue;
      }
      // Quadratic curves through the midpoints: smooth, and each piece gets its own width.
      let px = s[0].x, py = s[0].y;
      for (let i = 1; i < s.length; i++) {
        const a = s[i - 1], b = s[i];
        const mx = (a.x + b.x) / 2, my = (a.y + b.y) / 2;
        ctx.beginPath();
        ctx.moveTo(px, py);
        ctx.quadraticCurveTo(a.x, a.y, mx, my);
        ctx.lineWidth = (a.w + b.w) / 2;
        ctx.stroke();
        px = mx;
        py = my;
      }
      const last = s[s.length - 1];
      ctx.beginPath();
      ctx.moveTo(px, py);
      ctx.lineTo(last.x, last.y);
      ctx.lineWidth = last.w;
      ctx.stroke();
    }
  };

  useEffect(() => {
    const c = ref.current!;
    const dpr = window.devicePixelRatio || 1;
    c.width = W * dpr;
    c.height = H * dpr;
    paint(c, dpr);
    make.current = () => {
      if (!strokes.current.length) return null;
      // Three times the box size: sharp in print.
      const out = document.createElement("canvas");
      out.width = W * 3;
      out.height = H * 3;
      paint(out, 3);
      return out;
    };
  }, [ink]);

  const down = (e: PointerEvent) => {
    const c = ref.current!;
    c.setPointerCapture(e.pointerId);
    const r = c.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    const pen = e.pointerType === "pen";
    let last = { x: e.clientX - r.left, y: e.clientY - r.top, t: e.timeStamp };
    let w = pen ? base * (0.4 + e.pressure * 1.2) : base;
    const s: Pt[] = [{ x: last.x, y: last.y, w }];
    strokes.current.push(s);
    paint(c, dpr);
    const move = (ev: PointerEvent) => {
      for (const p of ev.getCoalescedEvents?.() ?? [ev]) {
        const x = p.clientX - r.left, y = p.clientY - r.top;
        const dist = Math.hypot(x - last.x, y - last.y);
        if (dist < 1.2) continue;
        // Pen: pressure. Mouse and finger: faster strokes are thinner, like ink.
        const v = dist / Math.max(1, p.timeStamp - last.t);
        const target = pen ? base * (0.4 + (p.pressure || 0.5) * 1.2) : base * Math.max(0.45, Math.min(1.35, 1.4 - v * 0.4));
        w = w * 0.6 + target * 0.4;
        s.push({ x, y, w });
        last = { x, y, t: p.timeStamp };
      }
      paint(c, dpr);
    };
    const up = () => {
      c.removeEventListener("pointermove", move);
      c.removeEventListener("pointerup", up);
      c.removeEventListener("pointercancel", up);
      onChange(true);
    };
    c.addEventListener("pointermove", move);
    c.addEventListener("pointerup", up);
    c.addEventListener("pointercancel", up);
  };

  const clear = () => {
    strokes.current = [];
    paint(ref.current!, window.devicePixelRatio || 1);
    onChange(false);
  };
  const undo = () => {
    strokes.current.pop();
    paint(ref.current!, window.devicePixelRatio || 1);
    onChange(strokes.current.length > 0);
  };

  return (
    <div class="sig-pad">
      <canvas ref={ref} style={{ width: `${W}px`, height: `${H}px` }} onPointerDown={down} aria-label="Draw here" />
      <div class="sig-line" />
      <div class="sig-hint">Draw with mouse, pen or finger</div>
      <div class="sig-pad-btns">
        <button class="btn ghost sm" onClick={undo}>
          Undo
        </button>
        <button class="btn ghost sm" onClick={clear}>
          Clear
        </button>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------- type

function Type({ ink, kind, make, onChange }: Part) {
  const [text, setText] = useState(() => (kind === "initials" ? initialsOf(authorName()) : authorName()));
  const [font, setFont] = useState(SCRIPT_FONTS[0]);
  // Fonts that aren't on this PC are left out.
  const fonts = useMemo(() => SCRIPT_FONTS.filter((f) => document.fonts.check(`20px "${f}"`)), []);
  useEffect(() => {
    make.current = () => (text.trim() ? typedCanvas(text.trim(), font, ink) : null);
    onChange(!!text.trim());
  }, [text, font, ink]);
  return (
    <div class="sig-type">
      <input class="inp" value={text} maxLength={60} placeholder={kind === "initials" ? "Your initials" : "Your name"} onInput={(e) => setText(e.currentTarget.value)} autoFocus />
      <div class="sig-fonts" role="radiogroup" aria-label="Handwriting">
        {(fonts.length ? fonts : SCRIPT_FONTS).map((f) => (
          <button key={f} role="radio" aria-checked={font === f} class={`sig-font ${font === f ? "on" : ""}`} style={{ fontFamily: `"${f}"`, color: ink }} onClick={() => setFont(f)} title={f}>
            {text || (kind === "initials" ? "AB" : "Your name")}
          </button>
        ))}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------- photo

function Photo({ ink, width, make, onChange }: Omit<Part, "kind" | "ink"> & { ink: string | null }) {
  const [img, setImg] = useState<HTMLImageElement | null>(null);
  const [bias, setBias] = useState(0);
  const [drag, setDrag] = useState(false);
  const preview = useRef<HTMLCanvasElement>(null);
  const file = useRef<HTMLInputElement>(null);

  const load = (blob: Blob) => {
    if (!blob.type.startsWith("image/")) return showToast("That isn't a picture.");
    const url = URL.createObjectURL(blob);
    const im = new Image();
    // The picture is read once loaded (it's drawn from the element, not the address): the address goes then, or
    // the photo stayed in memory until the window closed.
    im.onload = () => {
      URL.revokeObjectURL(url);
      setImg(im);
    };
    im.onerror = () => {
      URL.revokeObjectURL(url);
      showToast("That picture couldn't be read.");
    };
    im.src = url;
  };

  // Dropped from Explorer (the window hands us its path).
  const dropped = droppedPicture.value;
  useEffect(() => {
    if (!dropped) return;
    droppedPicture.value = null;
    const ext = dropped.split(".").pop()!.toLowerCase();
    invoke<ArrayBuffer>("read_picture", { path: dropped })
      .then((buf) => load(new Blob([buf], { type: `image/${ext === "jpg" || ext === "jfif" ? "jpeg" : ext}` })))
      .catch((x) => showToast(String(x)));
  }, [dropped]);

  // Paste a picture with Ctrl+V.
  useEffect(() => {
    const paste = (e: ClipboardEvent) => {
      const f = [...(e.clipboardData?.files ?? [])].find((f) => f.type.startsWith("image/"));
      if (f) load(f);
    };
    window.addEventListener("paste", paste);
    return () => window.removeEventListener("paste", paste);
  }, []);

  useEffect(() => {
    if (!img) {
      make.current = () => null;
      onChange(false);
      return;
    }
    const out = inkFromPhoto(img, ink, bias);
    make.current = () => out;
    onChange(true);
    const p = preview.current!;
    const s = Math.min((width - 16) / out.width, 120 / out.height);
    const dpr = window.devicePixelRatio || 1;
    p.width = Math.round(out.width * s * dpr);
    p.height = Math.round(out.height * s * dpr);
    p.style.width = `${Math.round(out.width * s)}px`;
    p.style.height = `${Math.round(out.height * s)}px`;
    const ctx = p.getContext("2d")!;
    ctx.imageSmoothingQuality = "high";
    ctx.drawImage(out, 0, 0, p.width, p.height);
  }, [img, ink, bias]);

  return (
    <div class="sig-photo">
      <div
        class={`sig-drop ${drag ? "over" : ""} ${img ? "has" : ""}`}
        onDragOver={(e) => { e.preventDefault(); e.stopPropagation(); setDrag(true); }}
        onDragLeave={() => setDrag(false)}
        onDrop={(e) => {
          e.preventDefault();
          e.stopPropagation();
          setDrag(false);
          const f = e.dataTransfer?.files[0];
          if (f) load(f);
        }}
      >
        {img ? (
          <canvas ref={preview} />
        ) : (
          <button class="btn" onClick={() => file.current?.click()}>
            <ImageUp size={16} /> Choose a picture…
          </button>
        )}
        {!img && <div class="muted">Sign on white paper and take a photo, then choose it, drop it here or paste it (Ctrl+V).</div>}
        <input ref={file} type="file" accept="image/*" hidden onChange={(e) => { const f = e.currentTarget.files?.[0]; if (f) load(f); }} />
      </div>
      {img && (
        <div class="sig-photo-row">
          <label class="muted" for="sig-bias">Clean</label>
          <input id="sig-bias" type="range" min={-1} max={1} step={0.05} value={bias} onInput={(e) => setBias(Number(e.currentTarget.value))} title="Left removes more paper shading; right keeps fainter strokes" />
          <label class="muted" for="sig-bias">Faint</label>
          <div class="grow" />
          <button class="btn ghost sm" onClick={() => { setImg(null); setBias(0); }}>
            Another
          </button>
        </div>
      )}
    </div>
  );
}
