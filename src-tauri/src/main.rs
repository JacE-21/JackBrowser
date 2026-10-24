#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Mutex,
    },
};
use tauri::{
    webview::{NewWindowResponse, WebviewBuilder},
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, State, WebviewUrl, WindowEvent,
};

/// Height of the toolbar (tab strip + address bar + bookmarks bar) in logical pixels.
const TOOLBAR_H: f64 = 112.0;
const HOME: &str = "https://duckduckgo.com/";

static NEXT_TAB: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Serialize)]
struct UrlEvent {
    id: String,
    url: String,
}

#[derive(Clone, Serialize)]
struct TitleEvent {
    id: String,
    title: String,
}

/// Portable mode: keep all browser data (cookies, cache, storage) in a `data`
/// folder next to the executable instead of the user profile.
fn data_dir() -> PathBuf {
    let base = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    let dir = base.join("data");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Turn whatever the user typed into a URL: real URLs pass through,
/// host-like text gets https://, everything else becomes a search.
fn resolve_input(input: &str) -> String {
    let s = input.trim();
    if s.is_empty() {
        return HOME.to_string();
    }
    if let Ok(u) = url::Url::parse(s) {
        if matches!(u.scheme(), "http" | "https" | "file" | "about") {
            return s.to_string();
        }
    }
    let looks_like_host = !s.contains(' ')
        && (s.contains('.') || s.starts_with("localhost"))
        && !s.ends_with('.');
    if looks_like_host {
        return format!("https://{s}");
    }
    let q: String = url::form_urlencoded::byte_serialize(s.as_bytes()).collect();
    format!("https://duckduckgo.com/?q={q}")
}


#[derive(Clone, Serialize, Deserialize)]
struct Bookmark {
    title: String,
    url: String,
}

struct Store {
    bookmarks: Mutex<Vec<Bookmark>>,
    adblock: AtomicBool,
    domains: HashSet<String>,
    adblock_script: String,
}

#[derive(Serialize)]
struct AppState {
    bookmarks: Vec<Bookmark>,
    adblock: bool,
    start_url: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Settings {
    adblock: bool,
}

fn read_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(|l| l.trim().to_lowercase())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect()
}

impl Store {
    fn load() -> Store {
        let dir = data_dir();
        let bookmarks: Vec<Bookmark> = std::fs::read_to_string(dir.join("bookmarks.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        let adblock = std::fs::read_to_string(dir.join("settings.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<Settings>(&t).ok())
            .map(|s| s.adblock)
            .unwrap_or(true);

        // Bundled lists (converted from uBlock Origin filter lists) plus optional user lists.
        let custom = dir.join("adblock");
        let _ = std::fs::create_dir_all(&custom);
        let mut domains: HashSet<String> =
            read_lines(include_str!("../filters/domains.txt")).into_iter().collect();
        if let Ok(t) = std::fs::read_to_string(custom.join("domains.txt")) {
            domains.extend(read_lines(&t));
        }
        let mut css: Vec<String> = include_str!("../filters/cosmetic.css")
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
        if let Ok(t) = std::fs::read_to_string(custom.join("cosmetic.css")) {
            css.extend(t.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()));
        }
        let mut sorted: Vec<&String> = domains.iter().collect();
        sorted.sort();
        let adblock_script = include_str!("../filters/adblock.js")
            .replace("__DOMAINS__", &serde_json::to_string(&sorted).unwrap_or("[]".into()))
            .replace("__CSS__", &serde_json::to_string(&css).unwrap_or("[]".into()));

        Store {
            bookmarks: Mutex::new(bookmarks),
            adblock: AtomicBool::new(adblock),
            domains,
            adblock_script,
        }
    }

    fn host_blocked(&self, host: &str) -> bool {
        let mut h = host.to_lowercase();
        loop {
            if self.domains.contains(&h) {
                return true;
            }
            match h.find('.') {
                Some(i) => h = h[i + 1..].to_string(),
                None => return false,
            }
        }
    }

    fn save_bookmarks(&self) {
        if let Ok(b) = self.bookmarks.lock() {
            if let Ok(t) = serde_json::to_string_pretty(&*b) {
                let _ = std::fs::write(data_dir().join("bookmarks.json"), t);
            }
        }
    }
}

/// Turn a `// @match https://example.com/*` glob into a JS regex source.
fn glob_to_regex(glob: &str) -> String {
    let mut out = String::from("^");
    for c in glob.chars() {
        match c {
            '*' => out.push_str(".*"),
            '.' | '+' | '?' | '^' | '$' | '{' | '}' | '(' | ')' | '|' | '[' | ']' | '\\' | '/' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out.push('$');
    out
}

/// "Extensions": every `data/extensions/*.js` file is injected into every page.
/// An optional `// @match <glob>` header limits it to matching URLs.
fn load_userscripts() -> Vec<String> {
    let dir = data_dir().join("extensions");
    let _ = std::fs::create_dir_all(&dir);
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().map(|x| x == "js").unwrap_or(false))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
        .into_iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .map(|code| {
            let matches: Vec<String> = code
                .lines()
                .filter_map(|l| l.trim().strip_prefix("// @match"))
                .map(|g| glob_to_regex(g.trim()))
                .collect();
            format!(
                "(function(){{try{{const M={};if(M.length&&!M.some(r=>new RegExp(r).test(location.href)))return;\n{}\n}}catch(e){{console.error('[extension]',e)}}}})();",
                serde_json::to_string(&matches).unwrap_or("[]".into()),
                code
            )
        })
        .collect()
}

fn content_size(app: &AppHandle) -> Option<(f64, f64)> {
    let win = app.get_window("main")?;
    let size = win.inner_size().ok()?.to_logical::<f64>(win.scale_factor().ok()?);
    Some((size.width, (size.height - TOOLBAR_H).max(1.0)))
}

fn create_tab(app: &AppHandle, id: &str, input: &str, private: bool) -> Result<(), String> {
    let window = app.get_window("main").ok_or("main window missing")?;
    let (w, h) = content_size(app).ok_or("cannot read window size")?;
    let target = resolve_input(input);
    let parsed: url::Url = target.parse().map_err(|e: url::ParseError| e.to_string())?;

    let store = app.state::<Store>();
    let adblock_on = store.adblock.load(Ordering::SeqCst);

    let nav_app = app.clone();
    let nav_id = id.to_string();
    let load_app = app.clone();
    let load_id = id.to_string();
    let title_app = app.clone();
    let title_id = id.to_string();
    let win_app = app.clone();

    let mut builder = WebviewBuilder::new(format!("tab-{id}"), WebviewUrl::External(parsed));
    builder = if private {
        // Private tab: nothing is written to disk.
        builder.incognito(true)
    } else {
        builder.data_directory(data_dir())
    };
    if adblock_on {
        builder = builder.initialization_script_for_all_frames(store.adblock_script.clone());
    }
    for script in load_userscripts() {
        builder = builder.initialization_script_for_all_frames(script);
    }
    let builder = builder
        .on_navigation(move |u| {
            if let Some(host) = u.host_str() {
                let st = nav_app.state::<Store>();
                if st.adblock.load(Ordering::SeqCst) && st.host_blocked(host) {
                    return false;
                }
            }
            let _ = nav_app.emit_to(
                "toolbar",
                "tab-url",
                UrlEvent { id: nav_id.clone(), url: u.to_string() },
            );
            true
        })
        .on_page_load(move |_wv, payload| {
            let _ = load_app.emit_to(
                "toolbar",
                "tab-url",
                UrlEvent { id: load_id.clone(), url: payload.url().to_string() },
            );
        })
        .on_document_title_changed(move |_wv, title| {
            let _ = title_app.emit_to(
                "toolbar",
                "tab-title",
                TitleEvent { id: title_id.clone(), title },
            );
        })
        // Links that want a new window (target=_blank, window.open) become new tabs.
        .on_new_window(move |u, _features| {
            let _ = win_app.emit_to("toolbar", "open-tab", u.to_string());
            NewWindowResponse::Deny
        });

    window
        .add_child(builder, LogicalPosition::new(0.0, TOOLBAR_H), LogicalSize::new(w, h))
        .map_err(|e| e.to_string())?;
    layout(app);
    layout_soon(app);
    Ok(())
}

#[tauri::command]
fn new_tab(app: AppHandle, url: String, private: bool) -> Result<String, String> {
    let id = NEXT_TAB.fetch_add(1, Ordering::SeqCst).to_string();
    create_tab(&app, &id, &url, private)?;
    Ok(id)
}

#[tauri::command]
fn get_state(store: State<Store>) -> AppState {
    AppState {
        bookmarks: store.bookmarks.lock().map(|b| b.clone()).unwrap_or_default(),
        adblock: store.adblock.load(Ordering::SeqCst),
        // `lite-browser <url>` opens that address in an extra tab.
        start_url: std::env::args().nth(1).filter(|a| !a.starts_with('-')),
    }
}

/// Adds the bookmark if it is new, removes it otherwise. Returns the updated list.
#[tauri::command]
fn toggle_bookmark(store: State<Store>, url: String, title: String) -> Vec<Bookmark> {
    let list = {
        let mut b = store.bookmarks.lock().unwrap();
        if let Some(i) = b.iter().position(|x| x.url == url) {
            b.remove(i);
        } else if !url.is_empty() {
            let title = if title.trim().is_empty() { url.clone() } else { title };
            b.push(Bookmark { title, url });
        }
        b.clone()
    };
    store.save_bookmarks();
    list
}

#[tauri::command]
fn set_adblock(store: State<Store>, enabled: bool) {
    store.adblock.store(enabled, Ordering::SeqCst);
    let _ = std::fs::write(
        data_dir().join("settings.json"),
        serde_json::to_string(&Settings { adblock: enabled }).unwrap_or_default(),
    );
}

#[tauri::command]
fn open_extensions_dir() {
    let dir = data_dir().join("extensions");
    let _ = std::fs::create_dir_all(&dir);
    #[cfg(target_os = "windows")]
    let cmd = "explorer";
    #[cfg(target_os = "macos")]
    let cmd = "open";
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let cmd = "xdg-open";
    let _ = std::process::Command::new(cmd).arg(dir).spawn();
}

#[tauri::command]
fn switch_tab(app: AppHandle, id: String) {
    for (label, wv) in app.webviews() {
        if let Some(tab) = label.strip_prefix("tab-") {
            if tab == id {
                let _ = wv.show();
                let _ = wv.set_focus();
            } else {
                let _ = wv.hide();
            }
        }
    }
}

#[tauri::command]
fn close_tab(app: AppHandle, id: String) {
    if let Some(wv) = app.get_webview(&format!("tab-{id}")) {
        let _ = wv.close();
    }
}

#[tauri::command]
fn navigate(app: AppHandle, id: String, input: String) -> Result<(), String> {
    let target = resolve_input(&input);
    let parsed: url::Url = target.parse().map_err(|e: url::ParseError| e.to_string())?;
    let wv = app.get_webview(&format!("tab-{id}")).ok_or("tab not found")?;
    wv.navigate(parsed).map_err(|e| e.to_string())
}

#[tauri::command]
fn tab_action(app: AppHandle, id: String, action: String) -> Result<(), String> {
    let wv = app.get_webview(&format!("tab-{id}")).ok_or("tab not found")?;
    let js = match action.as_str() {
        "back" => "history.back()",
        "forward" => "history.forward()",
        "reload" => "location.reload()",
        "stop" => "window.stop()",
        _ => return Err("unknown action".into()),
    };
    wv.eval(js).map_err(|e| e.to_string())
}

/// GTK only applies sizes once the widgets are realized, so re-run layout a few times shortly after
/// a webview is created.
fn layout_soon(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        for ms in [60u64, 200, 600] {
            std::thread::sleep(std::time::Duration::from_millis(ms));
            let a = app.clone();
            let _ = app.run_on_main_thread(move || layout(&a));
        }
    });
}

fn layout(app: &AppHandle) {
    let Some(window) = app.get_window("main") else { return };
    let Ok(scale) = window.scale_factor() else { return };
    let Ok(size) = window.inner_size() else { return };
    let size = size.to_logical::<f64>(scale);
    let ch = (size.height - TOOLBAR_H).max(1.0);
    if std::env::var("LITE_DEBUG").is_ok() {
        eprintln!("layout: window {}x{} scale {}", size.width, size.height, scale);
    }
    for (label, wv) in app.webviews() {
        if label == "toolbar" {
            let _ = wv.set_position(LogicalPosition::new(0.0, 0.0));
            let _ = wv.set_size(LogicalSize::new(size.width, TOOLBAR_H));
        } else {
            let _ = wv.set_position(LogicalPosition::new(0.0, TOOLBAR_H));
            let _ = wv.set_size(LogicalSize::new(size.width, ch));
        }
        if std::env::var("LITE_DEBUG").is_ok() {
            eprintln!("  {label}: pos {:?} size {:?}", wv.position().ok(), wv.size().ok());
        }
    }
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            new_tab, switch_tab, close_tab, navigate, tab_action, get_state, toggle_bookmark,
            set_adblock, open_extensions_dir
        ])
        .setup(|app| {
            app.manage(Store::load());
            let handle = app.handle().clone();
            let window = tauri::window::WindowBuilder::new(app, "main")
                .title("Lite Browser")
                .inner_size(1200.0, 800.0)
                .min_inner_size(480.0, 320.0)
                .build()?;

            let size = window.inner_size()?.to_logical::<f64>(window.scale_factor()?);
            let toolbar = window.add_child(
                // WebView2 requires every webview in one window to share a data dir.
                WebviewBuilder::new("toolbar", WebviewUrl::App("index.html".into()))
                    .data_directory(data_dir()),
                LogicalPosition::new(0.0, 0.0),
                LogicalSize::new(size.width, TOOLBAR_H),
            )?;

            // Linux: Tauri packs child webviews into a vertical GTK box and ignores positions,
            // so pin the toolbar to a fixed height and let the tab webviews take the rest.
            #[cfg(target_os = "linux")]
            toolbar.with_webview(|pw| {
                use gtk::prelude::*;
                let wv = pw.inner();
                wv.set_size_request(-1, TOOLBAR_H as i32);
                if let Some(parent) = wv.parent().and_then(|p| p.downcast::<gtk::Box>().ok()) {
                    parent.set_child_packing(&wv, false, false, 0, gtk::PackType::Start);
                }
            })?;
            #[cfg(not(target_os = "linux"))]
            let _ = toolbar;

            layout(&handle);
            layout_soon(&handle);
            window.on_window_event(move |e| {
                if let WindowEvent::Resized(_) = e {
                    layout(&handle);
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Lite Browser");
}
