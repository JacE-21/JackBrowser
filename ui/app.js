const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const tabs = new Map(); // id -> { title, url, private }
let active = null;
let bookmarks = [];
let adblock = true;

const $ = (id) => document.getElementById(id);
const urlInput = $("url");
const tabList = $("tab-list");

function save() {
  try {
    localStorage.setItem("session", JSON.stringify([...tabs.values()].filter((t) => !t.private).map((t) => t.url).filter(Boolean)));
  } catch (_) {}
}

function render() {
  tabList.textContent = "";
  for (const [id, t] of tabs) {
    const el = document.createElement("div");
    el.className = "tab" + (id === active ? " active" : "") + (t.private ? " private" : "");
    const title = document.createElement("span");
    title.className = "title";
    title.textContent = t.title || t.url || "New tab";
    const close = document.createElement("button");
    close.className = "close";
    close.textContent = "×";
    close.addEventListener("click", (e) => { e.stopPropagation(); closeTab(id); });
    el.append(title, close);
    el.addEventListener("mousedown", (e) => { if (e.button === 1) closeTab(id); else select(id); });
    tabList.append(el);
  }
}

async function select(id) {
  active = id;
  await invoke("switch_tab", { id });
  const t = tabs.get(id);
  if (document.activeElement !== urlInput) urlInput.value = t?.url ?? "";
  render();
  updateStar();
}

async function openTab(input, priv = false) {
  const id = await invoke("new_tab", { url: input, private: priv });
  tabs.set(id, { title: "", url: input, private: priv });
  await select(id);
  save();
  return id;
}

function updateStar() {
  const t = tabs.get(active);
  $("star").classList.toggle("on", !!t && bookmarks.some((b) => b.url === t.url));
  $("star").textContent = $("star").classList.contains("on") ? "\u2605" : "\u2606";
}

function renderBookmarks() {
  const bar = $("bookmarks");
  bar.textContent = "";
  if (!bookmarks.length) {
    const e = document.createElement("span");
    e.className = "empty";
    e.textContent = "Press the star to bookmark a page";
    bar.append(e);
  }
  for (const b of bookmarks) {
    const el = document.createElement("button");
    el.className = "bm";
    el.textContent = b.title;
    el.title = b.url + "\n(right-click to remove)";
    el.addEventListener("mousedown", (e) => {
      if (e.button === 0 && !e.ctrlKey) active && invoke("navigate", { id: active, input: b.url });
      else if (e.button === 1 || (e.button === 0 && e.ctrlKey)) openTab(b.url);
    });
    el.addEventListener("contextmenu", async (e) => {
      e.preventDefault();
      bookmarks = await invoke("toggle_bookmark", { url: b.url, title: b.title });
      renderBookmarks();
      updateStar();
    });
    bar.append(el);
  }
  updateStar();
}

function renderShield() {
  $("shield").classList.toggle("on", adblock);
  $("shield").title = "Ad blocker: " + (adblock ? "ON" : "OFF") + " (applies to new tabs)";
}

async function closeTab(id) {
  const ids = [...tabs.keys()];
  const i = ids.indexOf(id);
  tabs.delete(id);
  await invoke("close_tab", { id });
  if (tabs.size === 0) { active = null; await openTab(""); return; }
  if (id === active) await select(ids[i + 1] ?? ids[i - 1]);
  else render();
  save();
}

$("bar").addEventListener("submit", async (e) => {
  e.preventDefault();
  if (active) await invoke("navigate", { id: active, input: urlInput.value });
  urlInput.blur();
});
$("back").onclick = () => active && invoke("tab_action", { id: active, action: "back" });
$("forward").onclick = () => active && invoke("tab_action", { id: active, action: "forward" });
$("reload").onclick = () => active && invoke("tab_action", { id: active, action: "reload" });
$("home").onclick = () => active && invoke("navigate", { id: active, input: "https://duckduckgo.com/" });
$("new-tab").onclick = () => openTab("");
$("new-private").onclick = () => openTab("", true);
$("star").onclick = async () => {
  const t = tabs.get(active);
  if (!t || !t.url || t.private) return;
  bookmarks = await invoke("toggle_bookmark", { url: t.url, title: t.title });
  renderBookmarks();
};
$("shield").onclick = async () => {
  adblock = !adblock;
  await invoke("set_adblock", { enabled: adblock });
  renderShield();
};
$("ext").onclick = () => invoke("open_extensions_dir");
urlInput.addEventListener("focus", () => urlInput.select());

window.addEventListener("keydown", (e) => {
  const mod = e.ctrlKey || e.metaKey;
  if (mod && e.shiftKey && e.key.toLowerCase() === "n") { e.preventDefault(); openTab("", true); }
  else if (mod && e.key === "d") { e.preventDefault(); $("star").click(); }
  else if (mod && e.key === "t") { e.preventDefault(); openTab(""); }
  else if (mod && e.key === "w") { e.preventDefault(); active && closeTab(active); }
  else if (mod && e.key === "l") { e.preventDefault(); urlInput.focus(); }
  else if (mod && e.key === "r") { e.preventDefault(); $("reload").click(); }
  else if (e.altKey && e.key === "ArrowLeft") $("back").click();
  else if (e.altKey && e.key === "ArrowRight") $("forward").click();
});

listen("tab-url", ({ payload: { id, url } }) => {
  const t = tabs.get(id);
  if (!t) return;
  t.url = url;
  if (id === active && document.activeElement !== urlInput) urlInput.value = url;
  save();
  if (id === active) updateStar();
});
listen("tab-title", ({ payload: { id, title } }) => {
  const t = tabs.get(id);
  if (!t) return;
  t.title = title;
  render();
});
listen("open-tab", ({ payload }) => openTab(payload));

(async function restore() {
  const st = await invoke("get_state");
  bookmarks = st.bookmarks;
  adblock = st.adblock;
  renderBookmarks();
  renderShield();
  let urls = [];
  try { urls = JSON.parse(localStorage.getItem("session") || "[]"); } catch (_) {}
  if (!urls.length) urls = [""];
  for (const u of urls.slice(0, 8)) await openTab(u);
  if (st.start_url) await openTab(st.start_url);
  await select(st.start_url ? [...tabs.keys()].pop() : [...tabs.keys()][0]);
})();
