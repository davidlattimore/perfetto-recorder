use anyhow::Context;
use anyhow::anyhow;
use perfetto_recorder::ThreadTraceData;
use perfetto_recorder::TraceBuilder;
use perfetto_recorder::scope;
use rayon::iter::IntoParallelRefIterator;
use rayon::iter::ParallelIterator;
use std::sync::Mutex;
use std::time::Duration;

const N: u64 = 100;

fn main() -> anyhow::Result<()> {
    let trace_file = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow!("Please specify trace output file as an argument"))?;

    perfetto_recorder::start()?;

    // Reserving capacity for spans is entirely optional, but might help to get slightly more
    // consistent measurements.
    {
        scope!("Reserve trace capacity");
        let capacity_per_thread = N as usize
            * (perfetto_recorder::EVENTS_PER_SPAN + 2 * perfetto_recorder::EVENTS_PER_ARG)
            / rayon::current_num_threads()
            * 2;
        rayon::in_place_scope(|scope| {
            scope.spawn_broadcast(|_, _| {
                perfetto_recorder::current_thread_reserve(capacity_per_thread);
            })
        });
    }

    // Do some work with our threads.
    {
        scope!("Double many numbers");
        let collection1: Vec<u64> = (0..N).collect();
        let collection2: Vec<u64> = collection1
            .par_iter()
            .map(|v| {
                let sleep_ms = rand::random_range(2..10);
                scope!("Double number", v = *v, sleep_ms);
                // Pretend to do some real work.
                std::thread::sleep(Duration::from_millis(sleep_ms));
                v * 2
            })
            .collect();

        assert_eq!(collection2.last(), Some(&198));
    }

    let mut trace = TraceBuilder::new()?;

    // Record data from the main thread.
    trace.process_thread_data(&ThreadTraceData::take_current_thread());

    let trace = Mutex::new(trace);

    rayon::in_place_scope(|scope| {
        scope.spawn_broadcast(|_, _| {
            let thread_trace = ThreadTraceData::take_current_thread();
            trace.lock().unwrap().process_thread_data(&thread_trace);
        });
    });

    trace
        .into_inner()
        .unwrap()
        .write_to_file(&trace_file)
        .with_context(|| format!("Failed to write {trace_file}"))?;

    Ok(())
}
