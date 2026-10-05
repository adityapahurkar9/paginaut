// "Sort by Name ↑": tapping the name moves to the next way of sorting (each in its usual order:
// names A to Z, newest first, biggest first); the arrow turns the order around.
import { ArrowDown, ArrowUp } from "lucide-preact";

export interface SortState<K extends string> {
  key: K;
  desc: boolean;
}

export function SortToggle<K extends string>({ order, value, onChange }: { order: [K, string, boolean][]; value: SortState<K>; onChange: (s: SortState<K>) => void }) {
  const i = Math.max(0, order.findIndex(([k]) => k === value.key));
  const [, label] = order[i];
  const [nextKey, nextLabel, nextDesc] = order[(i + 1) % order.length];
  return (
    <div class="sort">
      <span class="sort-l">Sort by</span>
      <button class="sort-key" aria-label={`Sorted by ${label.toLowerCase()}; sort by ${nextLabel.toLowerCase()} instead`} onClick={() => onChange({ key: nextKey, desc: nextDesc })}>
        {label}
      </button>
      <button
        class="ib sm sort-dir"
        aria-label={value.desc ? "Descending; make it ascending" : "Ascending; make it descending"}
        onClick={() => onChange({ key: value.key, desc: !value.desc })}
      >
        {value.desc ? <ArrowDown size={17} /> : <ArrowUp size={17} />}
      </button>
    </div>
  );
}
