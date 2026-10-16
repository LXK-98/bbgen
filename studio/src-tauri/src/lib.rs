//! bbgen studio's native side. It runs the `bbgen` binary for every build (never bbgen's library), lists the
//! circuits that come with bbgen, and says when a circuit file is saved. The UI is in ../src.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::{AppHandle, Emitter, State};

/// The bbgen checkout this was built in, if it's still there: its launcher, examples and benchmarks.
fn repo() -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    root.join("Cargo.toml").is_file().then(|| root.canonicalize().unwrap_or(root))
}

/// bbgen: $BBGEN, else one next to the studio, else the checkout's launcher (which builds bbgen when its sources
/// changed), else whatever `bbgen` is on PATH.
fn bbgen() -> PathBuf {
    if let Some(p) = std::env::var_os("BBGEN") {
        return PathBuf::from(p);
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        let beside = dir.join(if cfg!(windows) { "bbgen.exe" } else { "bbgen" });
        if beside.is_file() {
            return beside;
        }
    }
    if let Some(launcher) = repo().map(|r| r.join("bin/bbgen")).filter(|p| p.is_file()) {
        return launcher;
    }
    PathBuf::from("bbgen")
}

#[derive(Default)]
struct Studio {
    /// the build running now: a newer one stops it
    running: Mutex<Option<Arc<Mutex<Child>>>>,
    watchers: Mutex<HashMap<String, notify::RecommendedWatcher>>,
}

/// `bbgen build <path> --json` (and `--schematic` when the circuit view wants it): what it printed (a scene or an
/// error), its stderr and exit status.
#[tauri::command]
async fn build(path: String, schematic: bool, studio: State<'_, Studio>) -> Result<Value, String> {
    let exe = bbgen();
    let mut args = vec!["build", path.as_str(), "--json"];
    if schematic {
        args.push("--schematic");
    }
    let command = format!("bbgen {}", args.join(" "));
    let started = Instant::now();
    let mut child = Command::new(&exe).args(&args).stdin(Stdio::null()).stdout(Stdio::piped())
        .stderr(Stdio::piped()).spawn().map_err(|e| format!("can't run {}: {}", exe.display(), e))?;
    let (mut out, mut err) = (child.stdout.take().unwrap(), child.stderr.take().unwrap());
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
        let mut stdout = String::new();
        out.read_to_string(&mut stdout).ok();
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

/// The circuits that come with bbgen: its examples and benchmarks.
#[tauri::command]
fn examples() -> Vec<String> {
    let Some(root) = repo() else { return vec![] };
    let mut out = Vec::new();
    for dir in ["examples", "benchmarks"] {
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Studio::default())
        .invoke_handler(tauri::generate_handler![build, examples, watch, unwatch])
        .run(tauri::generate_context!())
        .expect("error while running bbgen studio");
}
