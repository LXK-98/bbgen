//! Compiles the vendored elkjs (vendor/elkjs) to QuickJS bytecode, once, so bbgen loads ELK in a few milliseconds
//! rather than parsing 1.5 MB of JavaScript every run (src/elk.rs). The bytecode comes from the same QuickJS that
//! runs it, and is written in the target's byte order.

use rquickjs::{Context, Module, Runtime, WriteOptions, WriteOptionsEndianness};
use std::path::PathBuf;

fn main() {
    let src = "vendor/elkjs/elk.bundled.js";
    println!("cargo:rerun-if-changed={}", src);
    println!("cargo:rerun-if-changed=build.rs");
    let elk = std::fs::read_to_string(src).unwrap_or_else(|e| panic!("{}: {}", src, e));
    let endianness = match std::env::var("CARGO_CFG_TARGET_ENDIAN").as_deref() {
        Ok("big") => WriteOptionsEndianness::Big,
        _ => WriteOptionsEndianness::Little,
    };
    let rt = Runtime::new().unwrap();
    let ctx = Context::full(&rt).unwrap();
    // compiled as a module: its top level runs when bbgen loads it, with the globals src/elk.rs sets up first
    let bytes = ctx.with(|ctx| {
        let module = Module::declare(ctx.clone(), "elk", elk).unwrap_or_else(|e| panic!("{} doesn't compile: {}", src, e));
        module.write(WriteOptions { endianness, strip_source: true, strip_debug: true, ..Default::default() }).unwrap()
    });
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("elk.qbc");
    std::fs::write(out, bytes).unwrap();
}
