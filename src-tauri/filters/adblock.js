(() => {
  if (window.__liteAdblock) return;
  window.__liteAdblock = true;
  const D = new Set(__DOMAINS__);
  const RULES = __CSS__;

  const hostOf = (u) => { try { return new URL(u, location.href).hostname.toLowerCase(); } catch (_) { return ""; } };
  const blockedHost = (h) => {
    while (h) {
      if (D.has(h)) return true;
      const i = h.indexOf(".");
      if (i < 0) return false;
      h = h.slice(i + 1);
    }
    return false;
  };
  const isBlocked = (u) => !!u && blockedHost(hostOf(String(u)));

  // Network calls from scripts
  const of = window.fetch;
  if (of) window.fetch = function (i, n) {
    const u = typeof i === "string" ? i : i && i.url;
    if (isBlocked(u)) return Promise.reject(new TypeError("Blocked by Lite Browser"));
    return of.apply(this, arguments);
  };
  const oo = XMLHttpRequest.prototype.open;
  XMLHttpRequest.prototype.open = function (m, u) {
    if (isBlocked(u)) arguments[1] = "data:,";
    return oo.apply(this, arguments);
  };
  if (navigator.sendBeacon) {
    const sb = navigator.sendBeacon.bind(navigator);
    navigator.sendBeacon = (u, d) => (isBlocked(u) ? true : sb(u, d));
  }

  // Elements created from script
  for (const [C, p] of [[HTMLScriptElement, "src"], [HTMLImageElement, "src"], [HTMLIFrameElement, "src"]]) {
    const d = Object.getOwnPropertyDescriptor(C.prototype, p);
    if (d && d.set) {
      Object.defineProperty(C.prototype, p, { ...d, set(v) { if (!isBlocked(v)) d.set.call(this, v); } });
    }
  }
  const sa = Element.prototype.setAttribute;
  Element.prototype.setAttribute = function (n, v) {
    if ((n === "src" || n === "href") && /^(SCRIPT|IMG|IFRAME|LINK)$/.test(this.tagName) && isBlocked(v)) return;
    return sa.call(this, n, v);
  };

  // Elements present in the HTML
  const check = (n) => {
    if (n.nodeType !== 1) return;
    if (/^(SCRIPT|IFRAME|IMG|LINK)$/.test(n.tagName) && isBlocked(n.src || n.href)) n.remove();
  };
  new MutationObserver((ms) => {
    for (const m of ms) for (const n of m.addedNodes) {
      check(n);
      if (n.querySelectorAll) n.querySelectorAll("script[src],iframe[src],img[src],link[href]").forEach(check);
    }
  }).observe(document, { childList: true, subtree: true });

  // Cosmetic filtering: one rule per selector so one bad selector can't break the rest
  const addCss = () => {
    const st = document.createElement("style");
    st.textContent = RULES.map((s) => s + "{display:none!important}").join("\n");
    (document.head || document.documentElement).appendChild(st);
  };
  if (document.documentElement) addCss();
  else document.addEventListener("DOMContentLoaded", addCss, { once: true });
})();
