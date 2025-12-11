use perfetto_recorder::CounterUnit;
use perfetto_recorder::ThreadTraceData;
use perfetto_recorder::TraceBuilder;
use perfetto_recorder::scope;
use std::time::Instant;

const N: u32 = 100_000;
const N_COUNTERS: u32 = 100_000;

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

    // Benchmark counter recording
    perfetto_recorder::current_thread_reserve(N_COUNTERS as usize * 2); // 2 events per counter

    let mut builder = TraceBuilder::new()?;
    let mut counter_i64 =
        builder.create_counter_track("test_counter_i64", CounterUnit::Count, 1, false);
    let mut counter_f64 = builder.create_counter_track(
        "test_counter_f64",
        CounterUnit::Custom("%".to_string()),
        1,
        false,
    );

    // Measure record_counter_i64 time
    let start = Instant::now();

    for i in 0..N_COUNTERS {
        counter_i64.record_i64(perfetto_recorder::time(), i as i64);
    }

    let elapsed = start.elapsed();

    println!(
        "Average record_counter_i64 overhead: {} ns",
        (elapsed / N_COUNTERS).as_nanos()
    );

    // Measure record_counter_f64 time
    let start = Instant::now();

    for i in 0..N_COUNTERS {
        counter_f64.record_f64(perfetto_recorder::time(), i as f64);
    }

    let elapsed = start.elapsed();

    println!(
        "Average record_counter_f64 overhead: {} ns",
        (elapsed / N_COUNTERS).as_nanos()
    );

    // Measure encoding time for counters
    let start = Instant::now();

    let encoded = builder
        .process_thread_data(&ThreadTraceData::take_current_thread())
        .encode_to_vec();

    let elapsed = start.elapsed();

    println!(
        "Counter encode time: {} ms for {:0.1} MiB or {} ns per counter event",
        elapsed.as_millis(),
        encoded.len() as f64 / 1024_f64 / 1024_f64,
        (elapsed / N_COUNTERS).as_nanos()
    );

    Ok(())
}
