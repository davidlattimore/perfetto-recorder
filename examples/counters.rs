use anyhow::Result;
use perfetto_recorder::{CounterUnit, TraceBuilder};
use std::thread;
use std::time::Duration;

fn main() -> Result<()> {
    // Enable tracing
    perfetto_recorder::start()?;

    let trace_file = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "counters.pftrace".to_string());

    // Create a trace builder
    let mut trace = TraceBuilder::new()?;

    // Create counter tracks for system metrics
    let cpu_counter = trace.create_counter_track(
        "CPU Usage",
        CounterUnit::Custom("%".to_string()),
        1,     // Unit multiplier
        false, // Not incremental (absolute values)
    );

    let memory_counter = trace.create_counter_track(
        "Memory Usage",
        CounterUnit::SizeBytes,
        1024 * 1024, // Convert to MB
        false,       // Not incremental
    );

    let fps_counter = trace.create_counter_track(
        "Frame Rate",
        CounterUnit::Custom("fps".to_string()),
        1,
        false,
    );

    println!("Recording counter values...");

    // Simulate collecting metrics over time with actual delays
    for i in 0..100 {
        // Get current timestamp, handle both std::time::Instant and fastant::Instant
        let timestamp = perfetto_recorder::time();

        // Simulate varying CPU usage (15-85%)
        let cpu_usage = 50.0 + 30.0 * (i as f64 * 0.1).sin() + 5.0 * (i as f64 * 0.05).cos();

        // Simulate memory usage growing over time (500-1500 MB)
        let memory_mb = 500 + (i * 10) + ((i as f64 * 0.2).sin() * 50.0) as i64;

        // Simulate frame rate varying (30-60 fps)
        let fps = 45.0 + 15.0 * (i as f64 * 0.15).cos();

        // Record counter values
        trace.record_counter_f64(cpu_counter, timestamp, cpu_usage);
        trace.record_counter_i64(memory_counter, timestamp, memory_mb);
        trace.record_counter_f64(fps_counter, timestamp, fps);

        // Add some spikes at interesting points
        if i == 30 {
            trace.record_counter_f64(cpu_counter, timestamp, 95.0); // CPU spike
        }
        if i == 60 {
            trace.record_counter_i64(memory_counter, timestamp, 1800); // Memory spike
        }

        // Small delay between samples (10ms)
        thread::sleep(Duration::from_millis(10));
    }

    // Write the trace to a file
    trace.write_to_file(&trace_file)?;
    Ok(())
}
