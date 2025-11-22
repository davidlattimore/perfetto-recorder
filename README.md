# Perfetto record

This crate can be used to record Perfetto traces showing internal spans of what your application is
doing. See [perfetto.dev](https://perfetto.dev/) for more information about Perfetto.

## Example usage

```rust
use perfetto_recorder::scope;
use perfetto_recorder::TraceBuilder;
use perfetto_recorder::ThreadTraceData;

perfetto_recorder::start()?;
{
    scope!("foo", value = 1_u64, foo = 2_i64, baz = "baz");
    // Do some work.
}
TraceBuilder::new()?
    .process_thread_data(&ThreadTraceData::take_current_thread())
    .write_to_file("out.pftrace");
```

The duration of the span `foo` will be recorded along with the supplied arguments.

If your application uses multiple threads, then you'll need to call
`ThreadTraceData::take_current_thread()` from each thread in order to gather the trace data from
those threads. See `examples/rayon.rs` for an example.

You should then be able to open `out.pftrace` in the [perfetto UI](https://ui.perfetto.dev/).

## Features

### enable

The `enable` feature enables recording of spans. This is off by default because it's assumed that
you don't want to captuure span information during normal running and only want to opt-in when
you're analysing performance.

### fastant

Turn this feature on in order to get faster span captures. Typically, this feature would be expected
to reduce span overhead from about 115 ns to aboout 50 ns. This feature does however incur about a
20 ms delay at program startup. If you use this feature, it's suggested that you enable it together
with the `enable` feature. e.g.  something like this.

```toml
[features]
perfetto = ["perfetto-recorder/enable", "perfetto-recorder/fastant"]
```

## Performance

The primary reason why this crate exists is in order to reduce the overhead of recording a span. If
the overhead is too high, then parts of the application with lots of spans will use more time,
making the trace less accurately reflect the way that the application behaves without tracing.

The other crates in this space all seem to build on top of tracing, which due to levels of
abstraction adds substantial overhead.

These are some benchmark results on an AMD Ryzen 9955HX:

* tracing-perfetto (file): 3038 ns per span
* tracing-perfetto (thread-local): 2723 ns per span
* tracing-perfetto-sdk-layer (file): 3716 ns per span
* tracing-perfetto-sdk-layer (file non-blocking): 3268 ns per span
* tracing-perfetto-sdk-layer (thread-local writer): 2970 ns per span
* perfetto-recorder: 61 ns per span

When actually converting the trace to Perfetto format, this crate then incurs a cost of
approximately 563 ns per span. By doing this work later, once the application has finished, we avoid
having too much effect on the actual runtime of the application. It's possible that our conversion
cost could be further reduced by writing the perfetto format directly rather than converting to an a
Prost in-memory representation first, but this hasn't been a priority.

## Unsupported features

This crate doesn't support async usage. Put another way, it assumes that a span opened on one thread
will be closed on that same thread. tracing-perfetto-sdk-layer has support for async.

tracing-perfetto-sdk-layer also has support for receiving perfetto tracing data from the system,
allowing the trace to also include things like scheduling events.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT)
at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in
Wild by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.
