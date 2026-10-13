// Minimal standalone renderer for the wireframe .dc.html files, so they open in
// any browser without the design canvas. Supports {{ path }} holes, <sc-for>,
// <sc-if> and the DCLogic renderVals() class. Wireframes only, not app code.
(function () {
  class DCLogic {
    constructor(props) { this.props = props || {}; this.state = {}; }
    setState(s) { Object.assign(this.state, s); }
  }
  window.DCLogic = DCLogic;

  const HOLE = /\{\{\s*([^}]+?)\s*\}\}/g;
  function lookup(path, scope) {
    path = path.trim();
    if (path === 'true') return true;
    if (path === 'false') return false;
    if (/^-?\d+(\.\d+)?$/.test(path)) return Number(path);
    return path.split('.').reduce((o, k) => (o == null ? undefined : o[k]), scope);
  }
  function interp(str, scope) {
    return str.replace(HOLE, (_, p) => { const v = lookup(p, scope); return v == null ? '' : String(v); });
  }
  function holeOnly(str) { const m = str.trim().match(/^\{\{\s*([^}]+?)\s*\}\}$/); return m ? m[1] : null; }

  function process(node, scope) {
    for (const child of Array.from(node.childNodes)) {
      if (child.nodeType === 3) {
        if (HOLE.test(child.textContent)) child.textContent = interp(child.textContent, scope);
        HOLE.lastIndex = 0;
        continue;
      }
      if (child.nodeType !== 1) continue;
      const tag = child.localName.toLowerCase();
      if (tag === 'sc-for') {
        const list = lookup(holeOnly(child.getAttribute('list')) || '', scope) || [];
        const as = child.getAttribute('as') || 'item';
        list.forEach((item, i) => {
          const frag = document.createDocumentFragment();
          for (const c of Array.from(child.childNodes)) frag.appendChild(c.cloneNode(true));
          const holder = document.createElement('div');
          holder.appendChild(frag);
          process(holder, Object.assign({}, scope, { [as]: item, $index: i }));
          while (holder.firstChild) node.insertBefore(holder.firstChild, child);
        });
        node.removeChild(child);
        continue;
      }
      if (tag === 'sc-if') {
        const v = lookup(holeOnly(child.getAttribute('value')) || 'false', scope);
        if (v) {
          process(child, scope);
          while (child.firstChild) node.insertBefore(child.firstChild, child);
        }
        node.removeChild(child);
        continue;
      }
      for (const attr of Array.from(child.attributes)) {
        if (attr.value.indexOf('{{') !== -1) child.setAttribute(attr.name, interp(attr.value, scope));
      }
      process(child, scope);
    }
  }

  document.addEventListener('DOMContentLoaded', () => {
    const script = document.querySelector('script[data-dc-script]');
    let vals = {};
    if (script) {
      let props = {};
      try {
        const decl = JSON.parse(script.getAttribute('data-props') || '{}');
        for (const [k, v] of Object.entries(decl)) if (v && 'default' in v) props[k] = v.default;
      } catch (e) {}
      const Component = new Function('DCLogic', script.textContent + '\nreturn Component;')(DCLogic);
      vals = new Component(props).renderVals() || {};
    }
    const root = document.querySelector('x-dc');
    if (!root) return;
    const helmet = root.querySelector('helmet');
    if (helmet) { while (helmet.firstChild) document.head.appendChild(helmet.firstChild); helmet.remove(); }
    process(root, vals);
    root.style.display = 'contents';
  });
})();
