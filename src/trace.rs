//! A video of bbgen at work (`--video x.mp4`): one frame per step of placing and wiring.
//!
//! Placement and wiring call step() as they go; it does nothing unless a recording is running. A frame is the
//! full drawing of the circuit as it stands, captioned with what the step did, the parts and wires it changed
//! ringed in orange. By default only the steps bbgen keeps are recorded; with every, the tries it threw away
//! too, briefly. The frames become a video with ImageMagick (SVG to PNG) and ffmpeg.
//!
//! With `--events` (start_events) the steps go to stdout instead, one JSON line each as they happen, for a
//! program that shows the run live (bbgen studio): every step, tried or kept, with its score, and a drawing
//! of the board with the kept ones only. A drawing means checking and drawing the whole circuit, which is
//! most of what a step would cost.

use crate::circuit::Circuit;
use std::cell::RefCell;
use std::path::Path;

pub const KEPT_SECONDS: f64 = 0.8;
pub const TRIED_SECONDS: f64 = 0.25;
pub const WIRE_SECONDS: f64 = 0.3;
pub const LAST_SECONDS: f64 = 3.0;
const DENSITY: f64 = 120.0;

pub struct Recorder {
    pub every: bool,
    pub frames: Vec<(String, f64)>,
    placing: u32,
    /// printing events, not keeping frames: how many so far
    events: Option<u64>,
}

/// What a step of placement did to the score: its phase (`start`, `turn`, `repair`), what it tried, and the
/// score before (none for a starting layout) and after.
pub struct Scored<'a> {
    pub phase: &'a str,
    pub what: String,
    pub from: Option<f64>,
    pub score: f64,
}

thread_local! {
    static RECORDER: RefCell<Option<Recorder>> = const { RefCell::new(None) };
}

pub fn active() -> bool {
    RECORDER.with(|r| r.borrow().is_some())
}

pub fn start(every: bool) {
    RECORDER.with(|r| *r.borrow_mut() = Some(Recorder { every, frames: vec![], placing: 0, events: None }));
}

/// Print each step as a JSON line from now on (`--events`), until finish().
pub fn start_events() {
    RECORDER.with(|r| *r.borrow_mut() = Some(Recorder { every: true, frames: vec![], placing: 0, events: Some(0) }));
}

/// Whether the steps are printed as events, not recorded as frames. Then placement may build its trials side
/// by side: their order is the same, and the events all come from the thread that keeps the score.
pub fn printing() -> bool {
    RECORDER.with(|r| r.borrow().as_ref().map(|r| r.events.is_some()).unwrap_or(false))
}

pub fn finish() -> Vec<(String, f64)> {
    RECORDER.with(|r| r.borrow_mut().take().map(|r| r.frames).unwrap_or_default())
}

/// Placement's trial builds wire themselves too; only the final build's wiring makes frames.
pub fn placing<T>(f: impl FnOnce() -> T) -> T {
    RECORDER.with(|r| if let Some(r) = r.borrow_mut().as_mut() { r.placing += 1 });
    let out = f();
    RECORDER.with(|r| if let Some(r) = r.borrow_mut().as_mut() { r.placing -= 1 });
    out
}

/// Record a frame, if a recording is running.
pub fn step(c: &Circuit, caption: &str, kept: bool, focus: &[String], seconds: Option<f64>, wiring: bool) {
    record(c, caption, None, kept, focus, seconds, wiring);
}

/// A step of placement, with the score it came to.
pub fn scored(c: &Circuit, caption: &str, scored: Scored, kept: bool, focus: &[String]) {
    record(c, caption, Some(scored), kept, focus, None, false);
}

fn record(c: &Circuit, caption: &str, scored: Option<Scored>, kept: bool, focus: &[String], seconds: Option<f64>, wiring: bool) {
    let wanted = RECORDER.with(|r| match r.borrow().as_ref() {
        None => false,
        Some(r) => !((wiring && r.placing > 0) || (!kept && !r.every)),
    });
    if !wanted {
        return;
    }
    if let Some(n) = RECORDER.with(|r| r.borrow_mut().as_mut().and_then(|r| r.events.as_mut().map(|n| { *n += 1; *n }))) {
        let phase = scored.as_ref().map(|s| s.phase).unwrap_or(if wiring { "wiring" } else { "done" });
        let mut ev = serde_json::json!({"event": "step", "n": n, "phase": phase,
            "caption": scored.as_ref().map(|s| s.what.as_str()).unwrap_or(caption), "kept": kept, "focus": focus,
            "score": scored.as_ref().map(|s| round1(s.score)), "from": scored.as_ref().and_then(|s| s.from).map(round1)});
        if kept {
            let result = crate::check::check(c);
            let svg = crate::render::draw_annotated_focus(c, &result, focus).0;
            let (width, height) = size(&svg);
            ev["svg"] = svg.into();
            ev["width"] = width.into();
            ev["height"] = height.into();
        }
        use std::io::Write;
        let mut out = std::io::stdout().lock();
        writeln!(out, "{}", ev).and_then(|_| out.flush()).ok();
        return;
    }
    let seconds = seconds.unwrap_or(if wiring { WIRE_SECONDS } else if kept { KEPT_SECONDS } else { TRIED_SECONDS });
    let result = crate::check::check(c);
    let svg = crate::render::draw_full(c, &result, None, Some(caption), focus).0;
    RECORDER.with(|r| if let Some(r) = r.borrow_mut().as_mut() { r.frames.push((svg, seconds)) });
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

fn size(svg: &str) -> (f64, f64) {
    let grab = |key: &str| -> f64 {
        let at = svg.find(&format!(" {}=\"", key)).unwrap() + key.len() + 3;
        svg[at..].split('"').next().unwrap().parse().unwrap()
    };
    (grab("width"), grab("height"))
}

fn on_path(tool: &str) -> bool {
    std::env::var_os("PATH").map(|p| std::env::split_paths(&p).any(|d| d.join(tool).is_file())).unwrap_or(false)
}

/// Frames to an .mp4 (or .gif, by out's suffix). Needs ImageMagick and ffmpeg.
pub fn write_video(frames: &[(String, f64)], out: &Path) -> Result<(), String> {
    let missing: Vec<&str> = ["magick", "ffmpeg"].into_iter().filter(|t| !on_path(t)).collect();
    if !missing.is_empty() {
        return Err(format!("a video needs {} on PATH", missing.join(" and ")));
    }
    if frames.is_empty() {
        return Err("nothing to record: no step was taken".into());
    }
    let width = frames.iter().map(|(s, _)| size(s).0).fold(f64::NEG_INFINITY, f64::max) * DENSITY / 96.0;
    let height = frames.iter().map(|(s, _)| size(s).1).fold(f64::NEG_INFINITY, f64::max) * DENSITY / 96.0;
    let (width, height) = (2 * (width / 2.0).ceil() as i64, 2 * (height / 2.0).ceil() as i64);
    let tmp = std::env::temp_dir().join(format!("bbgen-video-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let result = (|| -> Result<(), String> {
        let pngs: Vec<Result<std::path::PathBuf, String>> = std::thread::scope(|s| {
            let jobs: Vec<_> = frames.iter().enumerate().map(|(i, (svg, _))| {
                let tmp = &tmp;
                s.spawn(move || -> Result<std::path::PathBuf, String> {
                    let (sp, pp) = (tmp.join(format!("{:05}.svg", i)), tmp.join(format!("{:05}.png", i)));
                    std::fs::write(&sp, svg).map_err(|e| e.to_string())?;
                    let ok = std::process::Command::new("magick")
                        .args(["-density", &DENSITY.to_string(), "-background", "white"]).arg(&sp)
                        .args(["-gravity", "northwest", "-extent", &format!("{}x{}", width, height)]).arg(&pp)
                        .output().map_err(|e| e.to_string())?.status.success();
                    if ok { Ok(pp) } else { Err("magick failed".into()) }
                })
            }).collect();
            jobs.into_iter().map(|j| j.join().unwrap()).collect()
        });
        let pngs: Vec<std::path::PathBuf> = pngs.into_iter().collect::<Result<_, _>>()?;
        let mut lines = Vec::new();
        for (p, (_, seconds)) in pngs.iter().zip(frames) {
            lines.push(format!("file '{}'", p.file_name().unwrap().to_string_lossy()));
            lines.push(format!("duration {}", crate::pyf::repr(*seconds)));
        }
        lines.push(format!("file '{}'", pngs.last().unwrap().file_name().unwrap().to_string_lossy()));
        std::fs::write(tmp.join("frames.txt"), lines.join("\n") + "\n").map_err(|e| e.to_string())?;
        let gif = out.extension().map(|e| e.to_string_lossy().to_lowercase() == "gif").unwrap_or(false);
        let codec: Vec<&str> = if gif {
            vec!["-vf", "fps=10,split[a][b];[a]palettegen[p];[b][p]paletteuse", "-loop", "0"]
        } else {
            vec!["-vf", "fps=25", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "20"]
        };
        let target = std::fs::canonicalize(out.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new(".")))
            .map_err(|e| e.to_string())?.join(out.file_name().unwrap());
        let ok = std::process::Command::new("ffmpeg").args(["-y", "-loglevel", "error", "-f", "concat", "-safe", "0", "-i"])
            .arg(tmp.join("frames.txt")).args(&codec).arg(&target).current_dir(&tmp).status().map_err(|e| e.to_string())?.success();
        if ok { Ok(()) } else { Err("ffmpeg failed".into()) }
    })();
    std::fs::remove_dir_all(&tmp).ok();
    result
}
