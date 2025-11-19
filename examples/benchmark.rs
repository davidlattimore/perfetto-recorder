use perfetto_recorder::ThreadTraceData;
use perfetto_recorder::TraceBuilder;
use perfetto_recorder::scope;
use std::time::Instant;

const N: u32 = 100_000;

fn main() -> anyhow::Result<()> {
    perfetto_recorder::start()?;

    perfetto_recorder::current_thread_reserve(N as usize * perfetto_recorder::EVENTS_PER_SPAN);

    // Measure capture time
    let start = Instant::now();

    for _ in 0..N {
        scope!("foo");
    }

    let elapsed = start.elapsed();

    println!("Average span overhead: {} ns", (elapsed / N).as_nanos());

    // Measure encoding time
    let start = Instant::now();

    let mut builder = TraceBuilder::new()?;

    let encoded = builder
        .process_thread_data(&ThreadTraceData::take_current_thread())
        .encode_to_vec();

    let elapsed = start.elapsed();

    println!(
        "Encode time: {} ms for {:0.1} MiB or {} ns per span",
        elapsed.as_millis(),
        encoded.len() as f64 / 1024_f64 / 1024_f64,
        (elapsed / N).as_nanos()
    );

    Ok(())
}
