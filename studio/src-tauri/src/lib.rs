//! bbgen studio's native side. It runs the `bbgen` binary for every build (never bbgen's library), lists the
//! circuits that come with bbgen, and says when a circuit file is saved. The UI is in ../src.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Emitter, Manager, State};

/// The bbgen checkout this was built in, if it's still there: its launcher, examples and benchmarks.
fn repo() -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    root.join("Cargo.toml").is_file().then(|| root.canonicalize().unwrap_or(root))
}

/// bbgen, and how it was found: the one picked in the studio, else $BBGEN, else one next to the studio, else the
/// checkout's launcher (which builds bbgen when its sources changed), else whatever `bbgen` is on PATH.
fn find_bbgen(chosen: Option<&Path>) -> (PathBuf, &'static str) {
    if let Some(p) = chosen {
        return (p.into(), "picked in the studio");
    }
    if let Some(p) = std::env::var_os("BBGEN") {
        return (PathBuf::from(p), "$BBGEN");
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        let beside = dir.join(if cfg!(windows) { "bbgen.exe" } else { "bbgen" });
        if beside.is_file() {
            return (beside, "beside the studio");
        }
    }
    if let Some(launcher) = repo().map(|r| r.join("bin/bbgen")).filter(|p| p.is_file()) {
        return (launcher, "this checkout's bin/bbgen");
    }
    (PathBuf::from("bbgen"), "on PATH")
}

impl Studio {
    fn bbgen(&self) -> PathBuf {
        find_bbgen(self.chosen.lock().unwrap().as_deref()).0
    }
}

/// `bbgen --version`, or why it couldn't be run.
fn version(exe: &Path) -> std::result::Result<String, String> {
    let out = Command::new(exe).arg("--version").stdin(Stdio::null()).output()
        .map_err(|e| format!("can't run {}: {}", exe.display(), e))?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if out.status.success() && text.starts_with("bbgen ") {
        Ok(text)
    } else {
        Err(format!("{} doesn't answer --version as bbgen does", exe.display()))
    }
}

/// Run `path` as bbgen from now on (null: find it as at the start), if it is one; which bbgen runs, how it was
/// found, and its version.
#[tauri::command]
async fn use_bbgen(path: Option<String>, studio: State<'_, Studio>) -> Result<Value, String> {
    let chosen = path.filter(|p| !p.trim().is_empty()).map(PathBuf::from);
    let (exe, how) = find_bbgen(chosen.as_deref());
    let probe = exe.clone();
    let version = tauri::async_runtime::spawn_blocking(move || version(&probe)).await.map_err(|e| e.to_string())?;
    if chosen.is_some() {
        version.as_ref()?;
    }
    *studio.chosen.lock().unwrap() = chosen;
    Ok(json!({"path": exe.to_string_lossy(), "how": how, "version": version.as_ref().ok(), "error": version.err()}))
}

#[derive(Default)]
struct Studio {
    /// the bbgen picked in the studio, if one was
    chosen: Mutex<Option<PathBuf>>,
    /// the build running now: a newer one stops it
    running: Mutex<Option<Arc<Mutex<Child>>>>,
    watchers: Mutex<HashMap<String, notify::RecommendedWatcher>>,
    /// the circuit named on the command line (`bbgen studio FILE`), until the UI takes it
    opened: Mutex<Option<String>>,
}

/// The circuit a command line names: its first argument that isn't a flag, made absolute against `cwd`.
fn circuit_arg(args: impl Iterator<Item = String>, cwd: &Path) -> Option<String> {
    let arg = args.into_iter().find(|a| !a.starts_with('-'))?;
    Some(cwd.join(arg).to_string_lossy().into_owned())
}

/// The circuit the studio was started with, once: the UI opens it instead of the last one it had open.
#[tauri::command]
fn opened(studio: State<'_, Studio>) -> Option<String> {
    studio.opened.lock().unwrap().take()
}

/// A path from the page, made absolute: bbgen and editors can't take an absolute path for an option (`-o`, vim's
/// `+cmd`), whatever the page sends.
fn absolute(path: &str) -> Result<String, String> {
    std::path::absolute(path).map(|p| p.to_string_lossy().into_owned()).map_err(|e| format!("{path}: {e}"))
}

/// A circuit file from the page: an existing .yaml or .yml, as an absolute path.
fn circuit_file(path: &str) -> Result<String, String> {
    let file = absolute(path)?;
    let p = Path::new(&file);
    if p.is_file() && p.extension().is_some_and(|x| x == "yaml" || x == "yml") {
        Ok(file)
    } else {
        Err(format!("not a circuit file: {path}"))
    }
}

/// `bbgen build <path> --json` (and `--schematic`, or `--wires`, when the circuit view wants it): what it printed (a scene or an
/// error), its stderr and exit status. With `events`, `--events`: each step's line goes to `on_step` as bbgen prints it.
#[tauri::command]
async fn build(path: String, schematic: bool, wires: bool, events: bool, on_step: Channel, studio: State<'_, Studio>)
               -> Result<Value, String> {
    let exe = studio.bbgen();
    let path = absolute(&path)?;
    let mut args = vec!["build", path.as_str(), if events { "--events" } else { "--json" }];
    if schematic {
        args.push(if wires { "--wires" } else { "--schematic" });
    }
    let command = format!("bbgen {}", args.join(" "));
    let started = Instant::now();
    let mut child = Command::new(&exe).args(&args).stdin(Stdio::null()).stdout(Stdio::piped())
        .stderr(Stdio::piped()).spawn().map_err(|e| format!("can't run {}: {}", exe.display(), e))?;
    let (out, mut err) = (child.stdout.take().unwrap(), child.stderr.take().unwrap());
    let child = Arc::new(Mutex::new(child));
    if let Some(old) = studio.running.lock().unwrap().replace(child.clone()) {
        old.lock().unwrap().kill().ok();
    }
    let result = tauri::async_runtime::spawn_blocking(move || {
        let errs = std::thread::spawn(move || {
            let mut s = String::new();
            err.read_to_string(&mut s).ok();
            s
        });
        // a step's line starts with its "event"; the scene (or the error) is the rest
        let mut stdout = String::new();
        for line in BufReader::new(out).lines().map_while(|l| l.ok()) {
            if line.starts_with("{\"event\"") {
                on_step.send(InvokeResponseBody::Json(line)).ok();
            } else {
                stdout.push_str(&line);
                stdout.push('\n');
            }
        }
        let stderr = errs.join().unwrap_or_default();
        let code = child.lock().unwrap().wait().ok().and_then(|s| s.code());
        (stdout, stderr, code)
    }).await.map_err(|e| e.to_string())?;
    let (stdout, mut stderr, code) = result;
    let json: Value = match stdout.trim() {
        "" => Value::Null,
        text => serde_json::from_str(text).unwrap_or_else(|_| {
            stderr.push_str("\n(bbgen printed something that is not JSON)");
            Value::Null
        }),
    };
    Ok(json!({"json": json, "stderr": stderr, "code": code, "command": command,
              "ms": started.elapsed().as_millis() as u64}))
}

/// Stop the build that's running, if one is: it ends with no exit status.
#[tauri::command]
fn stop(studio: State<'_, Studio>) {
    if let Some(child) = studio.running.lock().unwrap().as_ref() {
        child.lock().unwrap().kill().ok();
    }
}

/// What `bbgen build` writes, for the export menu: one file (`board`, `bare`, `checklist` or `schematic`) to the
/// file `to`, or `all` (the board, its checklist and the schematic) into the folder `to`. bbgen writes them into a
/// folder of its own first, since the board and the bare board have the same name. Returns the files written.
fn export_files(exe: &Path, path: &str, what: &str, wires: bool, to: &Path) -> Result<Vec<String>, String> {
    let stem = Path::new(path).file_stem().ok_or(format!("not a file: {path}"))?.to_string_lossy().into_owned();
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let tmp = std::env::temp_dir().join(format!("bbgen-studio-{}-{}", std::process::id(), nanos));
    let mut args = vec!["build".to_string(), path.into(), "-o".into(), tmp.to_string_lossy().into_owned(), "--no-png".into()];
    if what == "bare" {
        args.push("--bare".into());
    }
    if what == "schematic" || what == "all" {
        args.push(if wires { "--wires" } else { "--schematic" }.into());
    }
    let run = Command::new(exe).args(&args).stdin(Stdio::null()).output()
        .map_err(|e| format!("can't run {}: {}", exe.display(), e));
    let copied = run.and_then(|out| {
        // 1 is a board the check found errors on: still a board to export
        if !matches!(out.status.code(), Some(0 | 1)) {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(err.trim().trim_start_matches("bbgen: ").to_string());
        }
        let name = |ext: &str| format!("{stem}.{ext}");
        let files: Vec<(PathBuf, PathBuf)> = match what {
            "board" | "bare" => vec![(tmp.join(name("svg")), to.into())],
            "checklist" => vec![(tmp.join(name("md")), to.into())],
            "schematic" => vec![(tmp.join(name("schematic.svg")), to.into())],
            "all" => ["svg", "md", "schematic.svg"].iter().map(|e| (tmp.join(name(e)), to.join(name(e)))).collect(),
            _ => return Err(format!("nothing to export called {what}")),
        };
        let mut written = Vec::new();
        for (from, to) in files {
            if !from.is_file() {
                return Err(format!("bbgen wrote no {}", from.file_name().unwrap_or_default().to_string_lossy()));
            }
            std::fs::copy(&from, &to).map_err(|e| format!("{}: {}", to.display(), e))?;
            written.push(to.to_string_lossy().into_owned());
        }
        Ok(written)
    });
    std::fs::remove_dir_all(&tmp).ok();
    copied
}

/// Export one of bbgen's files to a file you choose (`all`: into a folder you choose), offered as `name` (a `kind`, for
/// the dialog's filter). The dialog is opened here, not by the page, so only you say where the files go. The files
/// written, or null if you cancelled.
#[tauri::command]
async fn export(path: String, what: String, wires: bool, name: Option<String>, kind: Option<String>, window: tauri::Window,
                studio: State<'_, Studio>) -> Result<Option<Vec<String>>, String> {
    use tauri_plugin_dialog::DialogExt;
    if !matches!(what.as_str(), "board" | "bare" | "checklist" | "schematic" | "all") {
        return Err(format!("nothing to export called {what}"));
    }
    let exe = studio.bbgen();
    let path = absolute(&path)?;
    let mut dialog = window.dialog().file().set_parent(&window);
    if let Some(folder) = Path::new(&path).parent() {
        dialog = dialog.set_directory(folder);
    }
    tauri::async_runtime::spawn_blocking(move || {
        let to = if what == "all" {
            dialog.set_title("Export into a folder").blocking_pick_folder()
        } else {
            let name = name.ok_or(format!("no name to export the {what} as"))?;
            let ext = name.rsplit('.').next().unwrap_or_default().to_string();
            dialog.set_file_name(&name).add_filter(kind.unwrap_or_default(), &[&ext]).blocking_save_file()
        };
        let Some(to) = to else { return Ok(None) };
        let to = to.into_path().map_err(|e| e.to_string())?;
        export_files(&exe, &path, &what, wires, &to).map(Some)
    }).await.map_err(|e| e.to_string())?
}

/// Where the build guide keeps its progress: one file for every circuit (path → the steps done), in the app's data
/// folder. A file beside the circuit would end up in git. `pnpm dev`'s server uses the same file (vite/bbgen-dev.ts).
fn progress_file(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_data_dir().map(|d| d.join("guide-progress.json")).map_err(|e| e.to_string())
}

fn read_progress(file: &Path) -> serde_json::Map<String, Value> {
    std::fs::read_to_string(file).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.as_object().cloned()).unwrap_or_default()
}

/// The build guide's steps done for a circuit, each as its checklist item and text (`part:U1` and its line).
#[tauri::command]
fn progress(path: String, app: AppHandle) -> Result<Value, String> {
    let all = read_progress(&progress_file(&app)?);
    Ok(all.get(&path).and_then(|p| p.get("done")).cloned().unwrap_or(json!([])))
}

/// Keep the build guide's steps done for a circuit (none: forget it).
#[tauri::command]
fn save_progress(path: String, done: Vec<String>, app: AppHandle) -> Result<(), String> {
    let file = progress_file(&app)?;
    let mut all = read_progress(&file);
    if done.is_empty() {
        all.remove(&path);
    } else {
        all.insert(path, json!({"done": done}));
    }
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&file, serde_json::to_string_pretty(&Value::Object(all)).unwrap()).map_err(|e| e.to_string())
}

/// The circuits that come with bbgen: an installed studio's examples (share/bbgen/examples, beside its bin/), else
/// the checkout's examples and benchmarks.
#[tauri::command]
fn examples() -> Vec<String> {
    let installed = std::env::current_exe().ok().and_then(|e| Some(e.parent()?.parent()?.join("share/bbgen")))
        .filter(|s| s.join("examples").is_dir());
    let (root, dirs): (PathBuf, &[&str]) = match (installed, repo()) {
        (Some(share), _) => (share, &["examples"]),
        (None, Some(repo)) => (repo, &["examples", "benchmarks"]),
        (None, None) => return vec![],
    };
    let mut out = Vec::new();
    for dir in dirs {
        let mut files: Vec<String> = std::fs::read_dir(root.join(dir)).into_iter().flatten().flatten()
            .map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "yaml" || x == "yml"))
            .map(|p| p.to_string_lossy().into_owned()).collect();
        files.sort();
        out.extend(files);
    }
    out
}

/// Emit `file-changed` with the path whenever the file is saved. Watches its folder: editors often save by
/// writing a new file and renaming it over the old one.
#[tauri::command]
fn watch(path: String, app: AppHandle, studio: State<'_, Studio>) -> Result<(), String> {
    use notify::Watcher;
    let file = PathBuf::from(&path);
    let (dir, name) = match (file.parent(), file.file_name()) {
        (Some(d), Some(n)) => (d.to_path_buf(), n.to_os_string()),
        _ => return Err(format!("not a file: {}", path)),
    };
    let tell = path.clone();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if let Ok(e) = event {
            if e.kind.is_modify() || e.kind.is_create() {
                if e.paths.iter().any(|p| p.file_name() == Some(&name)) {
                    app.emit("file-changed", &tell).ok();
                }
            }
        }
    }).map_err(|e| e.to_string())?;
    watcher.watch(&dir, notify::RecursiveMode::NonRecursive).map_err(|e| e.to_string())?;
    studio.watchers.lock().unwrap().insert(path, watcher);
    Ok(())
}

#[tauri::command]
fn unwatch(path: String, studio: State<'_, Studio>) {
    studio.watchers.lock().unwrap().remove(&path);
}

/// Editors that need a terminal: started from the studio, they get a terminal window of their own.
const IN_TERMINAL: &[&str] = &["vi", "vim", "nvim", "nano", "micro", "hx", "helix", "kak", "ne", "joe", "mg"];

/// Terminals to run such an editor in, if `$TERMINAL` isn't set, and how each is given a command.
const TERMINALS: &[(&str, &[&str])] = &[
    ("ptyxis", &["--"]), ("gnome-terminal", &["--"]), ("konsole", &["-e"]), ("kitty", &[]), ("alacritty", &["-e"]),
    ("foot", &[]), ("wezterm", &["start", "--"]), ("xterm", &["-e"]),
];

fn on_path(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file()))
}

/// How an editor is told to open a file at a line: `+12 file` unless it's one known to want otherwise.
fn at_line(editor: &str, path: &str, line: Option<u32>) -> Vec<String> {
    let Some(line) = line else { return vec![path.into()] };
    match editor {
        "code" | "code-insiders" | "codium" | "cursor" | "windsurf" => vec!["--goto".into(), format!("{path}:{line}")],
        "zed" | "zeditor" | "subl" | "hx" | "helix" | "micro" => vec![format!("{path}:{line}")],
        "kate" => vec!["--line".into(), line.to_string(), path.into()],
        _ => vec![format!("+{line}"), path.into()],
    }
}

/// The command that opens `path` at `line`: `$VISUAL` or `$EDITOR` (in a terminal window when it's a terminal
/// editor), else the system's own opener, at the top of the file.
fn editor(path: &str, line: Option<u32>) -> Command {
    let opener = || {
        let mut c = Command::new(if cfg!(target_os = "macos") { "open" } else if cfg!(windows) { "explorer" } else { "xdg-open" });
        c.arg(path);
        c
    };
    let Some(set) = ["VISUAL", "EDITOR"].iter().filter_map(|v| std::env::var(v).ok()).find(|v| !v.trim().is_empty()) else {
        return opener();
    };
    let mut words: Vec<String> = set.split_whitespace().map(String::from).collect();
    let name = Path::new(&words[0]).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    words.extend(at_line(&name, path, line));
    if IN_TERMINAL.contains(&name.as_str()) {
        let terminal = std::env::var("TERMINAL").ok().filter(|t| !t.is_empty()).map(|t| (t, vec!["-e".to_string()]))
            .or_else(|| TERMINALS.iter().find(|(t, _)| on_path(t)).map(|(t, a)| (t.to_string(), a.iter().map(|s| s.to_string()).collect())));
        let Some((terminal, flags)) = terminal.filter(|_| cfg!(all(unix, not(target_os = "macos")))) else { return opener() };
        let mut c = Command::new(terminal);
        c.args(flags).args(words);
        return c;
    }
    let mut c = Command::new(&words[0]);
    c.args(&words[1..]);
    c
}

/// Open the circuit file in an editor, at a line when it's given.
#[tauri::command]
fn edit(path: String, line: Option<u32>) -> Result<(), String> {
    let path = circuit_file(&path)?;
    let mut command = editor(&path, line);
    let mut child = command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()
        .map_err(|e| format!("can't run {}: {}", command.get_program().to_string_lossy(), e))?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// WebKitGTK zooms the whole page on a pinch, and the page can't stop it: there's no cancellable event, nor a
/// setting. So its zoom gesture (which WebKitGTK keeps on the view as `wk-view-zoom-gesture`) is made to do
/// nothing, and each step of a pinch goes to the page as what Chromium and Firefox send for one: a wheel event with
/// Ctrl held, where the fingers are. Over a drawing, that zooms the drawing; anywhere else, nothing.
#[cfg(target_os = "linux")]
fn pinch_zooms_the_drawing(view: &webkit2gtk::WebView) {
    use glib::translate::{FromGlibPtrNone, ToGlibPtr};
    use gtk::prelude::*;
    use std::cell::Cell;
    use std::rc::Rc;
    use webkit2gtk::WebViewExt;
    let gesture = unsafe {
        let view: *mut glib::gobject_ffi::GObject = view.upcast_ref::<glib::Object>().to_glib_none().0;
        let p = glib::gobject_ffi::g_object_get_data(view, c"wk-view-zoom-gesture".as_ptr());
        if p.is_null() {
            return;
        }
        // WebKitGTK's own handlers: zooming the page
        glib::gobject_ffi::g_signal_handlers_destroy(p as *mut _);
        gtk::GestureZoom::from_glib_none(p as *mut gtk::ffi::GtkGestureZoom)
    };
    // a pinch's scale is since it began; the page wants each step's
    let last = Rc::new(Cell::new(1.0f64));
    gesture.connect_begin({
        let last = last.clone();
        move |_, _| last.set(1.0)
    });
    let view = view.clone();
    gesture.connect_scale_changed(move |gesture, scale| {
        let Some((x, y)) = gesture.bounding_box_center() else { return };
        if scale <= 0.0 {
            return;
        }
        let step = scale / last.replace(scale);
        // Drawing zooms by exp(-deltaY * 0.0015)
        let js = format!(
            "document.elementFromPoint({x},{y})?.dispatchEvent(new WheelEvent('wheel',{{deltaY:{},clientX:{x},clientY:{y},ctrlKey:true,bubbles:true,cancelable:true}}))",
            -step.ln() / 0.0015
        );
        view.evaluate_javascript(&js, None, None, None::<&gio::Cancellable>, |_| {});
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let cwd = std::env::current_dir().unwrap_or_default();
    let started_with = circuit_arg(std::env::args().skip(1), &cwd);
    tauri::Builder::default()
        // a second start (`bbgen studio FILE` while the studio is open) hands its circuit to this one, and exits
        .plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
            if let Some(path) = circuit_arg(args.into_iter().skip(1), Path::new(&cwd)) {
                let _ = app.emit("open-file", path);
            }
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .manage(Studio { opened: Mutex::new(started_with), ..Default::default() })
        .invoke_handler(tauri::generate_handler![build, stop, examples, watch, unwatch, edit, export, use_bbgen, progress, save_progress,
                                                 opened])
        .setup(|app| {
            #[cfg(target_os = "linux")]
            if let Some(window) = app.get_webview_window("main") {
                window.with_webview(|w| pinch_zooms_the_drawing(&w.inner()))?;
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running bbgen studio");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The checkout's examples, for a circuit file that's there.
    fn example() -> String {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/eeprom.yaml").to_string_lossy().into_owned()
    }

    #[test]
    fn paths_from_the_page_cant_pass_for_options() {
        // bbgen and editors take these for options (vim runs `+cmd`); absolute, they're only paths
        for path in ["-o", "--video=x.mp4", "+!touch pwned", "circuit.yaml"] {
            let p = absolute(path).unwrap();
            assert!(Path::new(&p).is_absolute() && !p.starts_with('-') && !p.starts_with('+'), "{path} -> {p}");
        }
    }

    #[test]
    fn only_a_circuit_file_is_opened_in_an_editor() {
        let file = circuit_file(&example()).unwrap();
        assert!(Path::new(&file).is_absolute());
        for path in ["+!touch pwned.yaml", "-c.yaml", "/no/such/circuit.yaml", env!("CARGO_MANIFEST_DIR")] {
            assert!(circuit_file(path).is_err(), "{path}");
        }
        // there, but not a circuit
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        assert!(circuit_file(&manifest.to_string_lossy()).is_err());
    }

    #[test]
    fn the_page_runs_no_script_but_its_own() {
        // the drawings go in the page as bbgen writes them: if markup ever got into one, the policy keeps it from
        // running script, which could call the studio's commands
        let conf: Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let csp = &conf["app"]["security"]["csp"];
        assert!(csp.is_object(), "no Content Security Policy: {csp}");
        let sources = |directive: &str| csp[directive].as_str().unwrap_or_default().split_whitespace().collect::<Vec<_>>();
        let scripts = sources("script-src");
        assert!(!scripts.is_empty(), "script-src isn't set");
        for loose in ["'unsafe-inline'", "'unsafe-eval'", "'unsafe-hashes'", "*", "data:", "blob:", "http:", "https:"] {
            assert!(!scripts.contains(&loose), "script-src allows {loose}");
        }
        assert_eq!(sources("object-src"), ["'none'"]);
        assert_eq!(sources("base-uri"), ["'none'"]);
    }
}
