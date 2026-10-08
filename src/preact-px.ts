// Preact 11 no longer adds "px" to plain numbers in inline styles (Preact 10 and React do: { left: 12 } is 12px).
// The interface gives positions and sizes as plain numbers throughout, so they get their px here, as Preact's own
// compat layer does it: on each element's style object, copied first, for every property that has a unit
// (not opacity, z-index, flex, line-height and the like). Imported before anything renders.
import { options, type VNode } from "preact";

/** Properties whose numbers have no unit (Preact's compat list). */
const UNITLESS = /^(-|f[lo].*[^se]$|g.{5,}[^ps]$|z|o[pr]|(W.{5})?[lL]i.*(t|mp)$|an|(bo|s).{4}Im|sca|m.{6}[ds]|ta|c.*[st]$|wido|ini)/;
/** The answer per property name: this runs for every element made, and the interface uses a few dozen names. */
const unitless = new Map<string, boolean>();
const isUnitless = (key: string) => {
  let u = unitless.get(key);
  if (u === undefined) unitless.set(key, (u = UNITLESS.test(key)));
  return u;
};

const next = options.vnode;
options.vnode = (vnode: VNode) => {
  const props = vnode.props as { style?: unknown };
  if (typeof vnode.type === "string" && props.style && typeof props.style === "object") {
    let style = props.style as Record<string, unknown>;
    let copied = false;
    for (const key in style) {
      if (typeof style[key] !== "number" || isUnitless(key)) continue;
      if (!copied) {
        style = props.style = { ...style };
        copied = true;
      }
      style[key] += "px";
    }
  }
  next?.(vnode);
};
