//! Proof that the render path obeys the audio-thread contract.
//!
//! ADR-0002 states the contract: no heap allocation, no locking, no waiting, no
//! system calls, no panics, bounded execution time. Master Prompt #18 states it
//! first and Module Specification #002 repeats it. This file is where the claim
//! stops being a promise and becomes a measurement.
//!
//! The loop below is not a toy. It is the shape of the real callback: drain
//! commands sent from control threads, advance the authoritative clock by the
//! exact frames rendered, advance parameter ramps, fill the output buffer, and
//! publish a transport snapshot for the interface to read. If any of those steps
//! allocated, this test fails.
//!
//! A positive control runs first, so that a harness which had stopped counting
//! could not let the real assertion pass silently.

// A failure while setting the test up is a test failure and should be loud and
// immediate. The panic lints exist to protect the engine at runtime, not to
// stop a test from reporting that its own fixtures could not be built.
#![allow(
    clippy::expect_used,
    reason = "test fixture construction; a failure here must fail the test"
)]

use prv_rt::alloc_guard::{AllocationScope, CountingAllocator};
use prv_rt::{spsc, triple_buffer, AudioBuffer, LinearSmoother};
use prv_time::{Frames, SampleRate, Tempo, TimeSignature, TransportClock, TransportSnapshot};

#[global_allocator]
static ALLOCATOR: CountingAllocator<std::alloc::System> =
    CountingAllocator::new(std::alloc::System);

/// Frames per block. 128 at 48 kHz is 2.7 milliseconds — a low-latency setting
/// a performer would actually choose, and the least forgiving of the common
/// block sizes.
const BLOCK_FRAMES: usize = 128;

/// Blocks to render. At 128 frames this is about seven minutes of audio: long
/// enough that anything allocating even rarely is caught.
const BLOCKS: usize = 150_000;

/// Commands sent into the audio thread.
///
/// Plain data, `Copy`, with no owned allocation. Anything needing allocation is
/// prepared off-thread and handed over as a ready-made object, as ADR-0002
/// requires; nothing here has a destructor that could run on the audio thread.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Command {
    SetGain { target: f32, ramp_frames: u32 },
    Seek { position: i64 },
    SetTempo { micros_per_beat: u64 },
}

#[test]
fn the_harness_detects_allocation() {
    // Positive control. If the counting allocator were not installed, or the
    // counters had stopped moving, the real test below would pass for the wrong
    // reason. This makes that impossible.
    let scope = AllocationScope::begin();
    let allocated: Vec<u8> = Vec::with_capacity(4_096);
    assert_eq!(allocated.capacity(), 4_096);
    assert!(
        scope.allocations() > 0,
        "the allocation counter must observe a deliberate allocation; \
         if this fails, every other assertion in this file is meaningless"
    );

    let drop_scope = AllocationScope::begin();
    drop(allocated);
    assert!(
        drop_scope.deallocations() > 0,
        "the deallocation counter must observe a deliberate free"
    );
}

#[test]
fn a_full_render_loop_performs_no_heap_activity() {
    // ---- Setup. Everything that allocates happens here, before the loop. ----

    let (mut command_producer, mut command_consumer) = spsc::channel::<Command>(256);

    let mut clock = TransportClock::new(
        SampleRate::HZ_48000,
        Tempo::BPM_128,
        TimeSignature::FOUR_FOUR,
    );

    let (mut snapshot_writer, mut snapshot_reader) = triple_buffer::channel(clock.snapshot());

    let mut output = AudioBuffer::new(2, BLOCK_FRAMES).expect("valid buffer shape");
    let mut gain = LinearSmoother::new(1.0);

    // Queue commands up front so the loop exercises the intake path without a
    // producer thread perturbing the measurement.
    for index in 0..200_i64 {
        let command = match index % 3 {
            0 => Command::SetGain {
                target: 0.5,
                ramp_frames: 64,
            },
            1 => Command::Seek {
                position: index * 1_024,
            },
            _ => Command::SetTempo {
                micros_per_beat: 468_750,
            },
        };
        assert_eq!(command_producer.push(command), Ok(()));
    }

    // One warm-up block. Nothing here is expected to allocate, but excluding it
    // means the measurement cannot be confused by any first-call initialisation
    // inside the standard library.
    render_block(
        &mut command_consumer,
        &mut clock,
        &mut gain,
        &mut output,
        &mut snapshot_writer,
    );

    // ---- Measurement. ----

    let scope = AllocationScope::begin();

    for _ in 0..BLOCKS {
        render_block(
            &mut command_consumer,
            &mut clock,
            &mut gain,
            &mut output,
            &mut snapshot_writer,
        );
    }

    assert_eq!(
        scope.allocations(),
        0,
        "the render path allocated {} times across {BLOCKS} blocks; \
         a single allocation in a 2.7 ms callback is an audible dropout",
        scope.allocations()
    );
    assert_eq!(
        scope.deallocations(),
        0,
        "the render path ran {} destructors across {BLOCKS} blocks; \
         no destructor may run on the audio thread",
        scope.deallocations()
    );

    // The loop did real work. The queued Seek commands moved the transport
    // during the warm-up block, so the final position is that seek plus every
    // frame rendered afterwards.
    let frames_per_block = i64::try_from(BLOCK_FRAMES).expect("block size fits i64");
    // The warm-up block drained every queued command, so the last Seek landed
    // during it; the frames counted afterwards are that block plus the
    // measured ones.
    let blocks_rendered = i64::try_from(BLOCKS).expect("block count fits i64") + 1;
    // Command index 199 is the highest with `index % 3 == 1`, so it is the last
    // Seek issued.
    let last_seek = 199 * 1_024;
    assert_eq!(
        clock.position(),
        Frames::new(last_seek + blocks_rendered * frames_per_block),
        "the clock must advance by exactly the frames rendered after the last seek"
    );

    // And the interface side observes a coherent snapshot.
    let observed: TransportSnapshot = snapshot_reader.read();
    assert_eq!(observed.position, clock.position());
    assert_eq!(observed.sample_rate, SampleRate::HZ_48000);
}

/// One audio callback.
///
/// Everything reachable from here must satisfy the contract in ADR-0002.
fn render_block(
    commands: &mut spsc::Consumer<Command>,
    clock: &mut TransportClock,
    gain: &mut LinearSmoother,
    output: &mut AudioBuffer,
    snapshots: &mut triple_buffer::Writer<TransportSnapshot>,
) {
    // 1. Drain pending commands. Bounded: at most the queue capacity.
    while let Some(command) = commands.pop() {
        match command {
            Command::SetGain {
                target,
                ramp_frames,
            } => gain.set_target(target, ramp_frames),
            Command::Seek { position } => clock.seek(Frames::new(position)),
            Command::SetTempo { micros_per_beat } => {
                if let Ok(tempo) = Tempo::from_micros_per_beat(micros_per_beat) {
                    clock.set_tempo(tempo);
                }
            }
        }
    }

    // 2. Start from silence, so a processor that writes nothing leaves silence
    //    rather than the previous block.
    output.clear();

    // 3. Apply the smoothed gain to each channel. A real graph would run its
    //    processors here; the shape of the work is the same.
    //
    //    `try_from` rather than a cast: block sizes come from the audio device
    //    and are validated at the boundary, so the conversion cannot fail in
    //    practice, but a silent truncation would produce a clock that advances
    //    by the wrong amount — the one error this whole crate exists to prevent.
    let frames = u32::try_from(output.frames()).unwrap_or(u32::MAX);
    for channel in output.channels_iter_mut() {
        let mut channel_gain = *gain;
        for sample in channel.iter_mut() {
            *sample = 0.25 * channel_gain.next_value();
        }
    }
    // Advance the shared smoother once for the block, matching the per-channel
    // advance above.
    gain.skip(frames);

    // 4. Advance the authoritative clock by exactly the frames rendered.
    clock.advance(frames);

    // 5. Publish state for the interface. Wait-free.
    snapshots.publish(clock.snapshot());
}
