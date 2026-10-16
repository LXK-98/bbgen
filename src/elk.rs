//! ELK's layered layout, for the circuit view: symbols placed and wires routed by the Eclipse Layout Kernel.
//!
//! ELK is Java; elkjs is ELK compiled to JavaScript. It's vendored in vendor/elkjs and run here in QuickJS (the
//! rquickjs crate), so bbgen needs neither Node nor Java. build.rs compiles it to QuickJS bytecode, which loads in a
//! few milliseconds; a thread loads it the first time it lays something out, and keeps it.
//!
//! A graph goes in and comes back in ELK's JSON format, with positions and wire routes filled in:
//! https://eclipse.dev/elk/documentation/tooldevelopers/graphdatastructure/jsonformat.html. Its options are ELK's
//! own (https://eclipse.dev/elk/reference.html). The same graph always gets the same layout.
//!
//! elkjs runs ELK in a worker: here, the fake one it ships, which passes its messages with `setTimeout`. `layout`
//! runs the promise jobs and the timeouts until the result comes back.

use rquickjs::{Context, Ctx, Module, Runtime};
use serde_json::Value;
use std::cell::RefCell;

static BYTECODE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/elk.qbc"));

/// What elkjs expects around it: a CommonJS `module`, a `global`, a quiet `console`, and a `setTimeout` whose
/// callbacks `layout` runs.
const PRELUDE: &str = "var module = {exports: {}}; var exports = module.exports; var global = globalThis;
var console = {log: function () {}, warn: function () {}, error: function () {}};
var timeouts = [];
globalThis.setTimeout = function (f) { timeouts.push(f); return timeouts.length; };";

/// Lays out `input`, leaving the result in `done` (without `$H`, the object ids ELK's compiled Java adds) or why
/// not in `failed`.
const RUN: &str = "var done = null, failed = null;
new module.exports().layout(JSON.parse(input)).then(
  function (g) { done = JSON.stringify(g, function (k, v) { return k === '$H' ? undefined : v; }); },
  function (e) { failed = String(e && e.message || e); });";

struct Engine {
    rt: Runtime,
    ctx: Context,
}

thread_local! {
    static ENGINE: RefCell<Option<Result<Engine, String>>> = const { RefCell::new(None) };
}

/// What went wrong, with the JavaScript exception's message when there is one.
fn why(ctx: &Ctx, e: rquickjs::Error) -> String {
    if !e.is_exception() {
        return e.to_string();
    }
    let thrown = ctx.catch();
    match thrown.as_exception() {
        Some(ex) => ex.message().unwrap_or_else(|| format!("{:?}", ex)),
        None => format!("{:?}", thrown),
    }
}

fn load() -> Result<Engine, String> {
    let rt = Runtime::new().map_err(|e| e.to_string())?;
    let ctx = Context::full(&rt).map_err(|e| e.to_string())?;
    ctx.with(|ctx| {
        let run = || -> rquickjs::Result<()> {
            ctx.eval::<(), _>(PRELUDE)?;
            // Safety: the bytecode is build.rs's, made by this same QuickJS
            let module = unsafe { Module::load(ctx.clone(), BYTECODE) }?;
            let (_, evaluated) = module.eval()?;
            evaluated.finish::<()>()
        };
        run().map_err(|e| format!("can't load ELK: {}", why(&ctx, e)))
    })?;
    Ok(Engine { rt, ctx })
}

impl Engine {
    fn run(&self, input: String) -> Result<Value, String> {
        let fail = |what: &str, e: String| format!("ELK {}: {}", what, e);
        self.ctx.with(|ctx| {
            ctx.globals().set("input", input).and_then(|_| ctx.eval::<(), _>(RUN)).map_err(|e| fail("didn't start", why(&ctx, e)))
        })?;
        // the worker's messages and the promises' jobs, until there are none left
        loop {
            while self.rt.is_job_pending() {
                self.rt.execute_pending_job().map_err(|e| fail("failed", e.to_string()))?;
            }
            let more = self.ctx.with(|ctx| {
                ctx.eval::<bool, _>("timeouts.length > 0 && (timeouts.shift()(), true)").map_err(|e| fail("failed", why(&ctx, e)))
            })?;
            if !more && !self.rt.is_job_pending() {
                break;
            }
        }
        let (done, failed) = self.ctx.with(|ctx| {
            let g = ctx.globals();
            Ok::<_, String>((g.get::<_, Option<String>>("done").map_err(|e| why(&ctx, e))?,
                             g.get::<_, Option<String>>("failed").map_err(|e| why(&ctx, e))?))
        })?;
        match (done, failed) {
            (Some(json), _) => serde_json::from_str(&json).map_err(|e| fail("returned something odd", e.to_string())),
            (None, Some(e)) => Err(format!("ELK: {}", e)),
            (None, None) => Err("ELK never finished".into()),
        }
    }
}

/// Load ELK on this thread now, rather than with the first layout (to time the two apart).
pub fn ready() -> Result<(), String> {
    ENGINE.with(|cell| cell.borrow_mut().get_or_insert_with(load).as_ref().map(|_| ()).map_err(Clone::clone))
}

/// The graph laid out by ELK: the same graph, with each node's `x` and `y`, each port's, and each edge's `sections`
/// (its route: start, bend points, end) and `junctionPoints` filled in.
pub fn layout(graph: &Value) -> Result<Value, String> {
    ENGINE.with(|cell| {
        let mut slot = cell.borrow_mut();
        let engine = slot.get_or_insert_with(load).as_ref().map_err(Clone::clone)?;
        engine.run(graph.to_string())
    })
}
