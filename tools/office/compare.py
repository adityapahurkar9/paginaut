# The Office quality test: compares each engine's PDF of the sample files against Microsoft Office's own PDF
# (testfiles/office/ref, made by make-samples.py).
#   visual  ink overlap per page: the share of each page's ink (anything darker than light grey) that has ink within
#           3 pt on the other page, both ways (F1); 1 = same drawing, a missing or extra page scores 0. Plain
#           SSIM rated mostly blank pages highly whatever was on them.
#   text    words missing from the engine's PDF, words out of order, words it added
#   layout  page count and page size
# Writes testfiles/office/report.html (side by side) and scores.json. With --check, fails when an engine scores
# lower than tools/office/baseline.json allows (the regression test); --save-baseline records the current scores.
#
# Engine PDFs live in testfiles/office/<engine>/<name>.pdf (mupdf: office_convert convert; lo, word: the app's
# converters). Pages are rendered by the office_convert example (MuPDF), so all engines are seen the same way.
#   python tools/office/compare.py [--check | --save-baseline] [engine...]
import difflib
import html
import json
import os
import re
import subprocess
import sys
from collections import Counter

import numpy as np
from PIL import Image
from scipy.ndimage import binary_dilation

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
OFFICE = os.path.join(ROOT, "testfiles", "office")
DUMP = os.path.join(OFFICE, "dump")
TOOL = os.path.join(ROOT, "src-tauri", "target", "release", "examples", "office_convert.exe")
BASELINE = os.path.join(os.path.dirname(__file__), "baseline.json")
NAMES = ["report", "letter", "budget", "invoice", "deck"]
ENGINE_NAMES = {"mupdf": "MuPDF (built in)", "lo": "LibreOffice", "word": "Microsoft Office"}


def dump(engine, name):
    """Renders a PDF's pages (cached by modification time); returns [(image path, text)]."""
    pdf = os.path.join(OFFICE, "ref" if engine == "ref" else engine, name + ".pdf")
    if not os.path.exists(pdf):
        return None
    out = os.path.join(DUMP, engine)
    stamp = os.path.join(out, name + ".stamp")
    mtime = str(os.path.getmtime(pdf))
    if not (os.path.exists(stamp) and open(stamp).read() == mtime):
        for f in os.listdir(out) if os.path.isdir(out) else []:
            if re.fullmatch(re.escape(name) + r"-\d+\.(ppm|txt|png)", f):
                os.remove(os.path.join(out, f))
        subprocess.run([TOOL, "dump", out, pdf], check=True, capture_output=True)
        i = 0
        while os.path.exists(os.path.join(out, f"{name}-{i}.ppm")):
            Image.open(os.path.join(out, f"{name}-{i}.ppm")).save(os.path.join(out, f"{name}-{i}.png"))
            i += 1
        open(stamp, "w").write(mtime)
    pages = []
    i = 0
    while os.path.exists(os.path.join(out, f"{name}-{i}.png")):
        pages.append((os.path.join(out, f"{name}-{i}.png"), open(os.path.join(out, f"{name}-{i}.txt"), encoding="utf-8").read()))
        i += 1
    return pages


def ink(path, size):
    return np.asarray(Image.open(path).convert("L").resize(size, Image.BILINEAR)) < 200


def overlap(a, b, reach=3):
    if not a.any() and not b.any():
        return 1.0
    grow = np.ones((2 * reach + 1, 2 * reach + 1), bool)
    recall = (a & binary_dilation(b, grow)).sum() / max(a.sum(), 1)
    precision = (b & binary_dilation(a, grow)).sum() / max(b.sum(), 1)
    return float(2 * recall * precision / max(recall + precision, 1e-9))


def visual(ref, got):
    scores = []
    for i in range(max(len(ref), len(got))):
        if i >= len(ref) or i >= len(got):
            scores.append(0.0)
            continue
        size = Image.open(ref[i][0]).size  # 1 px per point
        scores.append(overlap(ink(ref[i][0], size), ink(got[i][0], size)))
    return scores


def words(pages):
    # Numbers keep their separators (1,234.50) so a reformatted number counts as a different word. Indic scripts
    # are left out: Word's own PDFs give their text back garbled (vowel signs split off, spaces lost), so they
    # can only be compared by the picture.
    t = re.sub(r"[\u0900-\u0DFF\u200c\u200d]+", " ", " ".join(t for _, t in pages))
    return re.findall(r"\d[\d,.]*\d|\w+", t)


def text(ref, got):
    a, b = words(ref), words(got)
    m = difflib.SequenceMatcher(None, a, b, autojunk=False)
    in_order = sum(bl.size for bl in m.get_matching_blocks())
    common = sum((Counter(a) & Counter(b)).values())
    n = max(len(a), 1)
    return {
        "words": len(a),
        "missing": round((len(a) - common) / n, 4),
        "reordered": round((common - in_order) / n, 4),
        "added": round(max(len(b) - common, 0) / n, 4),
        "missing_sample": list((Counter(a) - Counter(b)).keys())[:12],
    }


def layout(ref, got):
    same = sum(1 for r, g in zip(ref, got) if abs(Image.open(r[0]).size[0] - Image.open(g[0]).size[0]) <= 2 and abs(Image.open(r[0]).size[1] - Image.open(g[0]).size[1]) <= 2)
    return {"pages": len(got), "ref_pages": len(ref), "same_size": same}


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    engines = args or [e for e in ENGINE_NAMES if os.path.isdir(os.path.join(OFFICE, e))]
    results = {}
    for name in NAMES:
        ref = dump("ref", name)
        if ref is None:
            continue
        results[name] = {"ref": ref, "engines": {}}
        for e in engines:
            got = dump(e, name)
            if got is None:
                continue
            v = visual(ref, got)
            results[name]["engines"][e] = {
                "pages": got,
                "visual": round(sum(v) / len(v), 4),
                "page_visual": [round(x, 3) for x in v],
                "text": text(ref, got),
                "layout": layout(ref, got),
            }
    scores = {n: {e: {"visual": r["visual"], "missing": r["text"]["missing"], "reordered": r["text"]["reordered"], "pages": r["layout"]["pages"]} for e, r in res["engines"].items()} for n, res in results.items()}
    with open(os.path.join(OFFICE, "scores.json"), "w") as f:
        json.dump(scores, f, indent=1)
    write_report(results)
    for n, per in scores.items():
        for e, s in per.items():
            print(f"{n:8} {e:6} visual {s['visual']:.3f}  missing {s['missing']:.1%}  reordered {s['reordered']:.1%}  pages {s['pages']}/{results[n]['engines'][e]['layout']['ref_pages']}")
    if "--save-baseline" in sys.argv:
        with open(BASELINE, "w") as f:
            json.dump(scores, f, indent=1)
        print("baseline saved")
    if "--check" in sys.argv:
        base = json.load(open(BASELINE))
        bad = []
        for n, per in base.items():
            for e, s in per.items():
                now = scores.get(n, {}).get(e)
                if now is None:
                    continue
                if now["visual"] < s["visual"] - 0.02 or now["missing"] > s["missing"] + 0.01 or now["pages"] != s["pages"]:
                    bad.append(f"{n}/{e}: {s} -> {now}")
        print("\n".join(bad) if bad else "ALL PASSED (no engine worse than the baseline)")
        sys.exit(1 if bad else 0)


def write_report(results):
    rel = lambda p: os.path.relpath(p, OFFICE).replace("\\", "/")
    out = ["<!doctype html><meta charset=utf-8><title>Office conversion quality</title><style>",
           "body{font:14px system-ui;margin:24px;background:#f4f4f5;color:#18181b}table{border-collapse:collapse;margin:8px 0 24px}",
           "td,th{border:1px solid #d4d4d8;padding:4px 10px;text-align:left;background:#fff}.row{display:flex;gap:12px;overflow-x:auto;padding-bottom:8px}",
           ".col{flex:none}.col img{width:300px;border:1px solid #a1a1aa;background:#fff;display:block;margin-bottom:6px}.col h4{margin:4px 0}",
           ".good{color:#15803d}.bad{color:#b91c1c}small{color:#71717a}</style><h1>Office conversion quality</h1>",
           "<p>Reference: Microsoft Office's own PDF. Visual = ink in the same places, within 3 pt (1 = identical). Missing / reordered = share of the reference's words (Indic text is compared by the picture only: Word's PDFs give it back garbled).</p>"]
    for name, res in results.items():
        out.append(f"<h2>{name}</h2><table><tr><th>Engine<th>Visual<th>Missing words<th>Reordered<th>Added<th>Pages<th>Same page size<th>Missing, for example</tr>")
        for e, r in res["engines"].items():
            t, l = r["text"], r["layout"]
            cls = "good" if r["visual"] >= 0.8 and t["missing"] <= 0.02 else "bad"
            out.append(f"<tr><td>{ENGINE_NAMES.get(e, e)}<td class={cls}>{r['visual']:.3f}<td>{t['missing']:.1%}<td>{t['reordered']:.1%}<td>{t['added']:.1%}"
                       f"<td>{l['pages']} of {l['ref_pages']}<td>{l['same_size']}<td><small>{html.escape(' '.join(t['missing_sample']))}</small></tr>")
        out.append("</table>")
        cols = [("Microsoft Office (reference)", res["ref"], None)] + [(ENGINE_NAMES.get(e, e), r["pages"], r["page_visual"]) for e, r in res["engines"].items()]
        out.append("<div class=row>")
        for title, pages, pv in cols:
            out.append(f"<div class=col><h4>{html.escape(title)}</h4>")
            for i, (img, _) in enumerate(pages[:8]):
                note = f"<small>page {i + 1}" + (f" · {pv[i]:.2f}" if pv and i < len(pv) else "") + "</small>"
                out.append(f"{note}<img src='{rel(img)}' loading=lazy>")
            if len(pages) > 8:
                out.append(f"<small>… {len(pages) - 8} more pages</small>")
            out.append("</div>")
        out.append("</div>")
    with open(os.path.join(OFFICE, "report.html"), "w", encoding="utf-8") as f:
        f.write("\n".join(out))


main()
