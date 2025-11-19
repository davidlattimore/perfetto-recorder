use std::io::Result;
fn main() -> Result<()> {
    prost_build::compile_protos(&["proto/perfetto_trace.proto"], &["proto"])?;
    Ok(())
}
