//! A low-overhead way to capture timings of lots of spans within your application, then at a later
//! point, gather the traces from each thread and write them as a Perfetto trace file for viewing in
//! the Perfetto UI.

use crate::schema::DebugAnnotation;
use crate::schema::ThreadDescriptor;
use crate::schema::TracePacket;
use crate::schema::TrackDescriptor;
use nix::unistd::Pid;
use prost::Message;
use rand::RngCore;
use rand::rngs::ThreadRng;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

#[cfg(feature = "fastant")]
type Instant = fastant::Instant;

#[cfg(not(feature = "fastant"))]
type Instant = std::time::SystemTime;

mod schema;

/// Begins a time span that ends when the current scope ends.
///
/// Example usage:
///
/// ```
/// use perfetto_recorder::scope;
///
/// scope!("Parsing");
/// ```
///
/// If you need the span to outlive the current scope or you'd like to drop it before the end of the
/// current scope, then use [start_span] instead.
#[macro_export]
macro_rules! scope {
    ($($args:tt)*) => {
        let _guard = $crate::start_span!($($args)*);
    };
}

/// Begins a timing span, returning a guard, that when dropped will end the span.
///
/// Example usage:
///
/// ```
/// use perfetto_recorder::start_span;
///
/// let span_guard = start_span!("Parsing");
/// // Do some work.
/// drop(span_guard);
/// ```
///
/// If you don't need the span to outlive the scope in which it's created.
#[macro_export]
macro_rules! start_span {
    ($name:expr $(, $($arg_name:ident $( = $arg_value:expr)?),*)?) => {{
        const SOURCE_INFO: $crate::SourceInfo = $crate::SourceInfo {
            name: $name,
            file: file!(),
            line: line!(),
            arg_names: &[$($(stringify!($arg_name)),*)?],
        };
        if $crate::is_enabled() {
            $crate::record_event($crate::Event::StartSpan(&SOURCE_INFO));
                $crate::record_event($crate::Event::Timestamp($crate::time()));
            $($($crate::RecordArg::record_arg(
                $crate::start_span!(@arg_value $arg_name $($arg_value)?)
            );)*)?
        }

        $crate::SpanGuard::new(&SOURCE_INFO)
    }};

    (@arg_value $name:ident) => {
        $name
    };

    (@arg_value $name:ident $value:expr) => {
        $value
    };
}

/// A guard that when dropped will end a span.
///
/// Created by the [start_span] macro.
pub struct SpanGuard {
    #[cfg(feature = "enable")]
    pub source: &'static SourceInfo,
}

/// Trace events that occurred on a single thread.
pub struct ThreadTraceData {
    events: Vec<Event>,
    pid: Pid,
    tid: Pid,
    thread_name: Option<String>,
}

impl ThreadTraceData {
    pub fn take_current_thread() -> Self {
        let thread = std::thread::current();
        Self {
            events: EVENTS.take(),
            pid: nix::unistd::getpid(),
            tid: nix::unistd::gettid(),
            thread_name: thread.name().map(str::to_owned),
        }
    }
}

/// The number of events consumed by each span.
pub const EVENTS_PER_SPAN: usize = 4;

/// The number of events consumed by each argument.
pub const EVENTS_PER_ARG: usize = 1;

/// Reserve capacity on the current thread for additional spans and their arguments.
///
/// See constants [EVENTS_PER_SPAN] and [EVENTS_PER_ARG] to aid in working out what a reasonable
/// value might be. Note that string slices will consume additional capacity for each multiple of 15
/// in size. Calling this is entirely optional, but might make recording spans more consistent by
/// reducing the need to reallocate the recording for the current thread.
pub fn current_thread_reserve(additional: usize) {
    EVENTS.with_borrow_mut(|events| events.reserve(additional))
}

/// Types that implement this trait can be used as arguments to the [span] macro.
pub trait RecordArg {
    fn record_arg(self);
}

impl RecordArg for bool {
    fn record_arg(self) {
        record_event(Event::Bool(self));
    }
}

impl RecordArg for f64 {
    fn record_arg(self) {
        record_event(Event::F64(self));
    }
}

impl RecordArg for u64 {
    fn record_arg(self) {
        record_event(Event::U64(self));
    }
}

impl RecordArg for u32 {
    fn record_arg(self) {
        record_event(Event::U64(self.into()));
    }
}

impl RecordArg for u16 {
    fn record_arg(self) {
        record_event(Event::U64(self.into()));
    }
}

impl RecordArg for u8 {
    fn record_arg(self) {
        record_event(Event::U64(self.into()));
    }
}

impl RecordArg for usize {
    fn record_arg(self) {
        record_event(Event::U64(self as u64));
    }
}

impl RecordArg for i64 {
    fn record_arg(self) {
        record_event(Event::I64(self));
    }
}

impl RecordArg for i32 {
    fn record_arg(self) {
        record_event(Event::I64(self.into()));
    }
}

impl RecordArg for i16 {
    fn record_arg(self) {
        record_event(Event::I64(self.into()));
    }
}

impl RecordArg for i8 {
    fn record_arg(self) {
        record_event(Event::I64(self.into()));
    }
}

impl RecordArg for String {
    fn record_arg(self) {
        record_event(Event::String(self));
    }
}

impl RecordArg for isize {
    fn record_arg(self) {
        record_event(Event::I64(self as i64));
    }
}

impl RecordArg for &str {
    fn record_arg(self) {
        let mut pending: &[u8] = &[];
        for chunk in self.as_bytes().chunks(STR_PART_LEN) {
            if let Some(part_bytes) = pending.first_chunk::<STR_PART_LEN>() {
                record_event(Event::StrPart(*part_bytes));
            }
            pending = chunk;
        }
        let mut padded_bytes = [0; STR_PART_LEN];
        padded_bytes[..pending.len()].copy_from_slice(pending);
        record_event(Event::StrEnd {
            len: pending.len() as u8,
            bytes: padded_bytes,
        });
    }
}

#[doc(hidden)]
#[derive(Debug)]
pub enum Event {
    /// The start of a span. Must be followed by a timestamp.
    StartSpan(&'static SourceInfo),

    /// The end of a span. Must be followed by a timestamp.
    EndSpan(&'static SourceInfo),

    /// The time at which the preceeding start/end span occurred.
    Timestamp(Instant),

    Bool(bool),
    U64(u64),
    I64(i64),
    F64(f64),
    String(String),

    /// Part of a str slice. Must be followed by either another [Event::StrPart] or a
    /// [Event::StrEnd].
    StrPart([u8; STR_PART_LEN]),

    /// The end of a str slice.
    StrEnd {
        len: u8,
        bytes: [u8; STR_PART_LEN],
    },
}

/// The maximum number of bytes we can fit in an [Event::StrPart].
const STR_PART_LEN: usize = 15;

#[doc(hidden)]
#[derive(Debug)]
pub struct SourceInfo {
    pub name: &'static str,
    pub file: &'static str,
    pub line: u32,
    pub arg_names: &'static [&'static str],
}

#[doc(hidden)]
#[inline(always)]
pub fn record_event(event: Event) {
    EVENTS.with_borrow_mut(|events| events.push(event));
}

thread_local! {
    static EVENTS: RefCell<Vec<Event>> = const { RefCell::new(Vec::new()) };
}

thread_local! {
    static RNG: RefCell<ThreadRng> = RefCell::new(ThreadRng::default());
}

#[doc(hidden)]
#[inline(always)]
pub fn time() -> Instant {
    Instant::now()
}

impl Drop for SpanGuard {
    fn drop(&mut self) {
        #[cfg(feature = "enable")]
        if is_enabled() {
            record_event(Event::EndSpan(self.source));
            record_event(Event::Timestamp(time()));
        }
    }
}

impl SpanGuard {
    #[doc(hidden)]
    #[allow(unused_variables)]
    pub fn new(source: &'static SourceInfo) -> Self {
        #[cfg(feature = "enable")]
        {
            Self { source }
        }
        #[cfg(not(feature = "enable"))]
        {
            Self {}
        }
    }
}

const CLOCK_ID: u32 = 6;

static RUNTIME_ENABLED: AtomicBool = AtomicBool::new(false);

/// Enable recording. Can be called multiple times. Any spans emitted prior to the first call will
/// be discarded.
pub fn start() -> Result<(), TracingDisabledAtBuildTime> {
    if !cfg!(feature = "enable") {
        return Err(TracingDisabledAtBuildTime);
    }

    RUNTIME_ENABLED.store(true, Ordering::Relaxed);
    Ok(())
}

/// Returns whether recording is enabled.
pub fn is_enabled() -> bool {
    cfg!(feature = "enable") && RUNTIME_ENABLED.load(Ordering::Relaxed)
}

/// An error that is produced if [enable] is called when the "enable" feature of this crate is not
/// active.
#[derive(Debug)]
pub struct TracingDisabledAtBuildTime;

/// An error that is produced if [enable] has not been called, but we're trying to build a trace.
#[derive(Debug)]
pub struct TracingDisabled;

/// Used to build a trace file.
///
/// Example usage:
/// ```
/// # use perfetto_recorder::*;
///
/// # if perfetto_recorder::is_enabled() {
///
/// TraceBuilder::new()?
///     .process_thread_data(&ThreadTraceData::take_current_thread())
///     .write_to_file("a.pftrace")?;
///
/// # }
///
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub struct TraceBuilder {
    trace: schema::Trace,
    pending_interned: Option<schema::InternedData>,
    name_ids: HashMap<&'static str, u64>,
    debug_annotation_name_ids: HashMap<&'static str, u64>,
    source_location_ids: HashMap<(&'static str, u32), u64>,
    thread_uuids: HashMap<Pid, Uuid>,
    sequence_id: u32,
    #[cfg(feature = "fastant")]
    time_anchor: fastant::Anchor,
}

impl TraceBuilder {
    pub fn new() -> Result<TraceBuilder, TracingDisabled> {
        if !is_enabled() {
            return Err(TracingDisabled);
        }

        let sequence_id = RNG.with_borrow_mut(|rng| rng.next_u32());

        let mut builder = TraceBuilder {
            sequence_id,
            trace: Default::default(),
            pending_interned: Default::default(),
            name_ids: Default::default(),
            source_location_ids: Default::default(),
            debug_annotation_name_ids: Default::default(),
            thread_uuids: Default::default(),
            #[cfg(feature = "fastant")]
            time_anchor: fastant::Anchor::new(),
        };

        builder.add_packet(TracePacket {
            sequence_flags: Some(
                schema::trace_packet::SequenceFlags::SeqIncrementalStateCleared as u32,
            ),
            ..Default::default()
        });

        Ok(builder)
    }

    /// Merges trace data captured from a thread into the trace.
    pub fn process_thread_data(&mut self, thread: &ThreadTraceData) -> &mut Self {
        let thread_uuid = self.thread_uuid(thread);

        let mut events = thread.events.iter();

        while let Some(event) = events.next() {
            match event {
                Event::StartSpan(source_info) => {
                    self.emit_track_event(
                        source_info,
                        schema::track_event::Type::SliceBegin,
                        &mut events,
                        thread_uuid,
                    );
                }
                Event::EndSpan(source_info) => {
                    self.emit_track_event(
                        source_info,
                        schema::track_event::Type::SliceEnd,
                        &mut events,
                        thread_uuid,
                    );
                }
                other => panic!("Internal error: Unexpected event {other:?}"),
            }
        }

        self
    }

    // Encode the Perfetto trace as bytes.
    pub fn encode_to_vec(&self) -> Vec<u8> {
        self.trace.encode_to_vec()
    }

    pub fn write_to_file(&self, path: impl AsRef<Path>) -> Result<(), std::io::Error> {
        std::fs::write(path, self.encode_to_vec())
    }

    fn name_id(&mut self, name: &'static str) -> u64 {
        let next_id = self.name_ids.len() as u64 + 1;
        *self.name_ids.entry(name).or_insert_with(|| {
            self.pending_interned
                .get_or_insert_default()
                .event_names
                .push(schema::EventName {
                    iid: Some(next_id),
                    name: Some(name.to_owned()),
                });
            next_id
        })
    }

    fn debug_annotation_name_id(&mut self, name: &'static str) -> u64 {
        let next_id = self.debug_annotation_name_ids.len() as u64 + 1;
        *self
            .debug_annotation_name_ids
            .entry(name)
            .or_insert_with(|| {
                self.pending_interned
                    .get_or_insert_default()
                    .debug_annotation_names
                    .push(schema::DebugAnnotationName {
                        iid: Some(next_id),
                        name: Some(name.to_owned()),
                    });
                next_id
            })
    }

    fn source_location_id(&mut self, source_location: &'static SourceInfo) -> u64 {
        let next_id = self.source_location_ids.len() as u64 + 1;
        *self
            .source_location_ids
            .entry((source_location.file, source_location.line))
            .or_insert_with(|| {
                self.pending_interned
                    .get_or_insert_default()
                    .source_locations
                    .push(schema::SourceLocation {
                        iid: Some(next_id),
                        file_name: Some(source_location.file.to_owned()),
                        function_name: None,
                        line_number: Some(source_location.line),
                    });
                next_id
            })
    }

    fn emit_track_event(
        &mut self,
        source_info: &'static SourceInfo,
        kind: schema::track_event::Type,
        events: &mut std::slice::Iter<Event>,
        thread_uuid: Uuid,
    ) {
        let Some(Event::Timestamp(timestamp)) = events.next() else {
            panic!("Internal error: Timestamp must follow top-level events");
        };

        let name_id = self.name_id(source_info.name);
        let source_location_id = self.source_location_id(source_info);
        let mut track_event = schema::TrackEvent::default();
        track_event.set_type(kind);
        track_event.name_field = Some(schema::track_event::NameField::NameIid(name_id));
        track_event.source_location_field = Some(
            schema::track_event::SourceLocationField::SourceLocationIid(source_location_id),
        );
        track_event.track_uuid = Some(thread_uuid.0);

        if kind == schema::track_event::Type::SliceBegin && !source_info.arg_names.is_empty() {
            track_event.debug_annotations = source_info
                .arg_names
                .iter()
                .map(|arg_name| {
                    let value = convert_next_arg(events);
                    DebugAnnotation {
                        name_field: Some(schema::debug_annotation::NameField::NameIid(
                            self.debug_annotation_name_id(arg_name),
                        )),
                        value: Some(value),
                    }
                })
                .collect();
        }

        let packet = TracePacket {
            timestamp: Some(self.get_unix_nanos(*timestamp)),
            timestamp_clock_id: Some(CLOCK_ID),
            data: Some(schema::trace_packet::Data::TrackEvent(track_event)),
            interned_data: self.pending_interned.take(),
            ..Default::default()
        };

        self.add_packet(packet);
    }

    fn thread_uuid(&mut self, thread: &ThreadTraceData) -> Uuid {
        if let Some(uuid) = self.thread_uuids.get(&thread.tid) {
            return *uuid;
        }

        let uuid = Uuid::new();

        self.add_packet(TracePacket {
            data: Some(schema::trace_packet::Data::TrackDescriptor(
                TrackDescriptor {
                    uuid: Some(uuid.0),
                    thread: Some(ThreadDescriptor {
                        pid: Some(thread.pid.as_raw()),
                        tid: Some(thread.tid.as_raw()),
                        thread_name: thread.thread_name.clone(),
                    }),
                    ..Default::default()
                },
            )),
            ..Default::default()
        });

        self.thread_uuids.insert(thread.tid, uuid);

        uuid
    }

    fn add_packet(&mut self, mut packet: TracePacket) {
        packet.optional_trusted_packet_sequence_id = Some(
            schema::trace_packet::OptionalTrustedPacketSequenceId::TrustedPacketSequenceId(
                self.sequence_id,
            ),
        );
        self.trace.packet.push(packet);
    }

    #[cfg(feature = "fastant")]
    fn get_unix_nanos(&self, timestamp: Instant) -> u64 {
        timestamp.as_unix_nanos(&self.time_anchor)
    }

    #[cfg(not(feature = "fastant"))]
    fn get_unix_nanos(&self, timestamp: Instant) -> u64 {
        timestamp
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64
    }
}

/// Reads the next argument from `events`.
fn convert_next_arg(events: &mut std::slice::Iter<'_, Event>) -> schema::debug_annotation::Value {
    let event = events.next().expect("Internal error: missing arg value");

    use schema::debug_annotation::Value;
    match event {
        Event::StartSpan(_) => panic!("Internal error: Unexpected StartSpan"),
        Event::EndSpan(_) => panic!("Internal error: Unexpected EndSpan"),
        Event::Timestamp(_) => panic!("Internal error: Unexpected Timestamp"),
        Event::Bool(value) => Value::BoolValue(*value),
        Event::U64(value) => Value::UintValue(*value),
        Event::I64(value) => Value::IntValue(*value),
        Event::F64(value) => Value::DoubleValue(*value),
        Event::String(value) => Value::StringValue(value.clone()),
        Event::StrPart(bytes) => {
            let mut merged_bytes = Vec::new();
            merged_bytes.extend_from_slice(bytes);
            loop {
                match events.next() {
                    Some(Event::StrPart(bytes)) => {
                        merged_bytes.extend_from_slice(bytes);
                    }
                    Some(Event::StrEnd { len, bytes }) => {
                        merged_bytes.extend_from_slice(&bytes[..*len as usize]);
                        // The string started out as valid UTF-8 &str, so it should still be valid.
                        break Value::StringValue(String::from_utf8(merged_bytes).unwrap());
                    }
                    other => panic!(
                        "Internal error: Unexpected event {other:?} while looking for StrEnd"
                    ),
                }
            }
        }
        Event::StrEnd { len, bytes } => {
            Value::StringValue(str::from_utf8(&bytes[..*len as usize]).unwrap().to_owned())
        }
    }
}

impl Uuid {
    fn new() -> Uuid {
        Uuid(RNG.with_borrow_mut(|rng| rng.next_u64()))
    }
}

impl std::error::Error for TracingDisabledAtBuildTime {}

impl std::fmt::Display for TracingDisabledAtBuildTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "The \"enable\" feature of perfetto-recorder is not active"
        )
    }
}

impl std::error::Error for TracingDisabled {}

impl std::fmt::Display for TracingDisabled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "The `perfetto_recorder::start()` was not called")
    }
}

#[derive(Clone, Copy, Debug)]
struct Uuid(u64);

/// Units for counter tracks.
#[derive(Debug, Clone)]
pub enum CounterUnit {
    /// Unspecified unit.
    Unspecified,
    /// Time in nanoseconds.
    TimeNs,
    /// Generic count.
    Count,
    /// Size in bytes.
    SizeBytes,
    /// Custom unit with a name (e.g., "%", "fps", etc.).
    Custom(String),
}

impl CounterUnit {
    fn to_proto_unit(&self) -> Option<i32> {
        Some(match self {
            CounterUnit::Unspecified => schema::counter_descriptor::Unit::Unspecified,
            CounterUnit::TimeNs => schema::counter_descriptor::Unit::TimeNs,
            CounterUnit::Count => schema::counter_descriptor::Unit::Count,
            CounterUnit::SizeBytes => schema::counter_descriptor::Unit::SizeBytes,
            CounterUnit::Custom(_) => schema::counter_descriptor::Unit::Unspecified,
        } as i32)
    }

    fn to_proto_unit_name(&self) -> Option<String> {
        if let Self::Custom(name) = self {
            Some(name.clone())
        } else {
            None
        }
    }
}

/// A handle to a counter track that can be used to record counter values.
#[derive(Debug, Clone, Copy)]
pub struct CounterTrack {
    uuid: u64,
}

impl TraceBuilder {
    /// Creates a new counter track.
    ///
    /// Counter tracks display time-series data like CPU usage, memory usage, etc.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the counter track
    /// * `unit` - The unit for the counter values
    /// * `unit_multiplier` - Multiplier for the values (e.g., 1024*1024 to convert bytes to MB)
    /// * `is_incremental` - Whether values are incremental (delta) or absolute
    ///
    /// # Example
    ///
    /// ```
    /// # use perfetto_recorder::*;
    /// # if perfetto_recorder::is_enabled() {
    /// let mut trace = TraceBuilder::new()?;
    /// let cpu_counter = trace.create_counter_track(
    ///     "CPU Usage",
    ///     CounterUnit::Custom("%".to_string()),
    ///     1,
    ///     false,
    /// );
    /// # }
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn create_counter_track(
        &mut self,
        name: impl Into<String>,
        unit: CounterUnit,
        unit_multiplier: i64,
        is_incremental: bool,
    ) -> CounterTrack {
        let uuid = Uuid::new();

        self.add_packet(TracePacket {
            data: Some(schema::trace_packet::Data::TrackDescriptor(
                TrackDescriptor {
                    uuid: Some(uuid.0),
                    parent_uuid: None,
                    process: None,
                    thread: None,
                    counter: Some(schema::CounterDescriptor {
                        unit: unit.to_proto_unit(),
                        unit_name: unit.to_proto_unit_name(),
                        unit_multiplier: Some(unit_multiplier),
                        is_incremental: Some(is_incremental),
                    }),
                    static_or_dynamic_name: Some(
                        schema::track_descriptor::StaticOrDynamicName::Name(name.into()),
                    ),
                },
            )),
            ..Default::default()
        });

        CounterTrack { uuid: uuid.0 }
    }

    /// Records an integer counter value at a specific timestamp.
    ///
    /// # Arguments
    ///
    /// * `counter` - The counter track to record to
    /// * `timestamp` - The timestamp for this value
    /// * `value` - The counter value
    ///
    /// # Example
    ///
    /// ```
    /// # use perfetto_recorder::*;
    /// # if perfetto_recorder::is_enabled() {
    /// let mut trace = TraceBuilder::new()?;
    /// let counter = trace.create_counter_track("Memory", CounterUnit::SizeBytes, 1, false);
    /// trace.record_counter_i64(counter, perfetto_recorder::time(), 1024);
    /// # }
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn record_counter_i64(&mut self, counter: CounterTrack, timestamp: Instant, value: i64) {
        let packet = TracePacket {
            timestamp: Some(self.get_unix_nanos(timestamp)),
            timestamp_clock_id: Some(CLOCK_ID),
            data: Some(schema::trace_packet::Data::TrackEvent(schema::TrackEvent {
                track_uuid: Some(counter.uuid),
                r#type: Some(schema::track_event::Type::Counter as i32),
                counter_value_field: Some(schema::track_event::CounterValueField::CounterValue(
                    value,
                )),
                ..Default::default()
            })),
            ..Default::default()
        };

        self.add_packet(packet);
    }

    /// Records a floating-point counter value at a specific timestamp.
    ///
    /// # Arguments
    ///
    /// * `counter` - The counter track to record to
    /// * `timestamp` - The timestamp for this value
    /// * `value` - The counter value
    ///
    /// # Example
    ///
    /// ```
    /// # use perfetto_recorder::*;
    /// # if perfetto_recorder::is_enabled() {
    /// let mut trace = TraceBuilder::new()?;
    /// let counter = trace.create_counter_track("CPU %", CounterUnit::Custom("%".to_string()), 1, false);
    /// trace.record_counter_f64(counter, perfetto_recorder::time(), 42.5);
    /// # }
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn record_counter_f64(&mut self, counter: CounterTrack, timestamp: Instant, value: f64) {
        let packet = TracePacket {
            timestamp: Some(self.get_unix_nanos(timestamp)),
            timestamp_clock_id: Some(CLOCK_ID),
            data: Some(schema::trace_packet::Data::TrackEvent(schema::TrackEvent {
                track_uuid: Some(counter.uuid),
                r#type: Some(schema::track_event::Type::Counter as i32),
                counter_value_field: Some(
                    schema::track_event::CounterValueField::DoubleCounterValue(value),
                ),
                ..Default::default()
            })),
            ..Default::default()
        };

        self.add_packet(packet);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "enable")]
    #[test]
    fn test_basic_usage() {
        start().unwrap();
        {
            scope!(
                "foo",
                value = 1_u64,
                foo = 2_i64,
                baz = "baz",
                baz_owned = "baz".to_owned()
            );
            scope!("bar");
        }

        let num_events = EVENTS.with_borrow(|events| events.len());
        assert_eq!(num_events, 12);

        TraceBuilder::new()
            .unwrap()
            .process_thread_data(&ThreadTraceData::take_current_thread())
            .encode_to_vec();
    }

    #[cfg(not(feature = "enable"))]
    #[test]
    fn test_no_execution_when_disabled() {
        fn do_not_run() -> u32 {
            panic!("This should not be called");
        }

        scope!("foo", value = do_not_run());
    }

    /// Try different lengths of string slices to make sure we're able to split them into parts and
    /// join them back together again.
    #[test]
    fn str_encoding() {
        for l in 0..100 {
            let string: String = (0..l)
                .map(|i| char::from_u32('A' as u32 + i).unwrap())
                .collect();
            let str_slice = string.as_str();
            RecordArg::record_arg(str_slice);
            let events = EVENTS.take();
            let mut events = events.iter();
            match convert_next_arg(&mut events) {
                schema::debug_annotation::Value::StringValue(actual) => {
                    assert_eq!(actual, string);
                }
                other => panic!("Unexpected event: {other:?}"),
            }
            assert!(events.next().is_none());
        }
    }

    #[cfg(feature = "enable")]
    #[test]
    fn test_counter_tracks() {
        start().unwrap();

        let mut trace = TraceBuilder::new().unwrap();

        // Create different types of counter tracks
        let cpu_counter =
            trace.create_counter_track("CPU Usage", CounterUnit::Custom("%".to_string()), 1, false);

        let memory_counter =
            trace.create_counter_track("Memory", CounterUnit::SizeBytes, 1024 * 1024, false);

        let count_counter = trace.create_counter_track(
            "Events",
            CounterUnit::Count,
            1,
            true, // incremental
        );

        // Record some values
        let t1 = time();
        trace.record_counter_f64(cpu_counter, t1, 42.5);
        trace.record_counter_i64(memory_counter, t1, 1024);
        trace.record_counter_i64(count_counter, t1, 100);

        let t2 = time();
        trace.record_counter_f64(cpu_counter, t2, 75.0);
        trace.record_counter_i64(memory_counter, t2, 2048);
        trace.record_counter_i64(count_counter, t2, 50);

        // Verify we can encode without errors
        let bytes = trace.encode_to_vec();
        assert!(!bytes.is_empty());
    }
}
