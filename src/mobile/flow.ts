// A document's text for Read, Book and read aloud: every page's content, read once per document.
import { api, newId } from "../api";
import { Analysis, type Flow, type RawPage } from "../reflow/model";

const analyses = new Map<number, Promise<Analysis>>();

/** Every page's content, read once per document (starting at the page you're on). */
export function analysisOf(docId: number, count: number, start: number, onProgress: (f: number) => void): Promise<Analysis> {
  const had = analyses.get(docId);
  if (had) return had;
  const pages: (RawPage | undefined)[] = new Array(count);
  let received = 0;
  const p = new Promise<Analysis>((resolve, reject) => {
    api
      .reflow(docId, start, newId(), (ev) => {
        if (ev.kind === "page") {
          try {
            pages[ev.page] = JSON.parse(ev.data) as RawPage;
          } catch {
            /* a page that can't be read is left out */
          }
          onProgress(++received / count);
        } else if (ev.error) reject(new Error(ev.error));
        else if (!ev.complete) reject(new Error("cancelled"));
        else resolve(new Analysis(pages));
      })
      .catch(reject);
  });
  p.catch(() => analyses.delete(docId));
  analyses.set(docId, p);
  return p;
}

/** Forgets a closed document's reflowed text. */
export function releaseAnalysis(docId: number) {
  analyses.delete(docId);
}

/** The document's text in page order. */
export async function flowOf(docId: number, count: number, start: number): Promise<Flow> {
  const a = await analysisOf(docId, count, start, () => {});
  return a.flow(Array.from({ length: count }, (_, i) => i));
}
