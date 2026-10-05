// What older Android WebViews (before Chrome 86) lack and the shared viewer code uses. Loaded first.

if (!Element.prototype.replaceChildren) {
  const replace = function (this: Element | DocumentFragment, ...nodes: (Node | string)[]) {
    while (this.lastChild) this.removeChild(this.lastChild);
    this.append(...nodes);
  };
  Element.prototype.replaceChildren = replace;
  DocumentFragment.prototype.replaceChildren = replace;
}
