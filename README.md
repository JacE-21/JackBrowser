# Lite Browser

A tiny, portable browser built with [Tauri](https://tauri.app). It uses the web engine already on your
system (WebView2 on Windows, WebKitGTK on Linux) instead of bundling Chromium, so the whole app is a
single ~3-4 MB executable.

## Features
- Tabs (Ctrl+T new, Ctrl+W close, middle-click closes), address bar with search fallback (DuckDuckGo)
- Back / forward / reload / home, Ctrl+L focuses the address bar
- Links that open a new window become new tabs; `lite-browser <url>` opens a URL in a new tab
- Restores your open tabs on next start (private tabs are never saved)
- **Bookmarks:** star button or Ctrl+D; bookmarks bar under the address bar. Click to open, Ctrl/middle-click for a new tab, right-click to remove. Stored in `data/bookmarks.json`.
- **Private tabs:** Ctrl+Shift+N or the sunglasses button. Cookies and history are not written to disk.
- **Ad blocking:** built-in blocker using domain and cosmetic rules converted from the uBlock Origin filter lists
  (uBlock filters, badware, privacy). Toggle with the shield button (applies to new tabs).
  Add your own rules in `data/adblock/domains.txt` (one domain per line) and `data/adblock/cosmetic.css` (one CSS selector per line).
- **Extensions (userscripts):** drop `.js` files in `data/extensions/` (puzzle button opens the folder). They run on every page;
  add `// @match https://example.com/*` lines at the top to limit them to matching URLs.
- **Portable:** cookies, cache, bookmarks and settings live in a `data/` folder next to the executable.
  Copy the folder to a USB stick and your sessions go with it. Delete `data/` to wipe everything.

## Run
- Windows: needs the WebView2 runtime (already included in Windows 11 and updated Windows 10). Run `lite-browser.exe`.
- Linux: needs `libwebkit2gtk-4.1` (`sudo apt install libwebkit2gtk-4.1-0`). Run `./lite-browser`.

## Build
```
cd src-tauri
cargo build --release
```
Linux build deps: `libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev libxdo-dev libssl-dev`.
Windows build: push to GitHub and run the "Build portable binaries" workflow (Actions tab);
it produces `lite-browser.exe` for download.

## Limits
- This is **not** the uBlock Origin extension itself. Tauri's webviews (WebView2, WebKitGTK) cannot load Chrome/Firefox
  extensions, so the blocker reimplements the basics in JavaScript. It blocks requests made by page scripts, elements
  inserted by scripts or present in the HTML, top-level navigations, and hides common ad elements. It cannot intercept every
  network request the way the real extension does, and it does not support the full filter syntax.
- Private tabs rely on the webview's incognito mode. Verified on Linux only; test on Windows before relying on it.
- Windows build is untested here (built from the same source via the GitHub workflow).

## Notes
- Multi-webview support in Tauri is still behind the `unstable` feature flag, so pin versions if you need stability.
- Next candidates: downloads, find-in-page, a bookmarks manager, auto-updating filter lists.
