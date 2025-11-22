#!/usr/bin/env -S cargo +nightly -Zscript

---
[package]
edition = "2024"
[dependencies]
prost-build = "0.14.1"
---

use std::io::Result;
use std::path::Path;

fn main() -> Result<()> {
    let out_dir = Path::new(file!()).parent().unwrap();
    prost_build::Config::new()
        .out_dir(out_dir)
        .compile_protos(&[out_dir.join("perfetto_trace.proto")], &["proto"])?;
    Ok(())
}
