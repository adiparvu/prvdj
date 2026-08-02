//! Proof that the whole signal path obeys the audio-thread contract.
//!
//! `prv-rt` proves that its primitives allocate nothing. This proves the same
//! for the processors built on them — which is the claim that actually matters,
//! because a performer's audio runs through these, not through the primitives
//! directly.
//!
//! Two measurements, because they cover different risks:
//!
//! 1. A realistic channel strip with every control being moved while it renders.
//!    A chain whose parameters never change would not exercise the paths a
//!    parameter change takes, which is exactly where an allocation would hide.
//! 2. The same processors behind [`Chain`], to show that dynamic dispatch and
//!    bypass switching add nothing.

// A failure while setting the test up is a test failure and should be loud.
#![allow(
    clippy::expect_used,
    reason = "test fixture construction; a failure here must fail the test"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "block indices and frame counts are far below any precision limit"
)]
#![allow(
    clippy::integer_division,
    reason = "block counters; truncation is the intended cadence arithmetic"
)]

use prv_rt::alloc_guard::{AllocationScope, CountingAllocator};
use prv_rt::AudioBuffer;
use prv_time::{SampleRate, Tempo, TimeSignature};
use prv_transport::{Transport, TransportEvent};

use prv_dsp::{
    Chain, DjFilter, Gain, Limiter, LoudnessMeter, PitchShift, PrepareConfig, ProcessContext,
    Processor, Resampler, ThreeBandEq, TimeStretch,
};

#[global_allocator]
static ALLOCATOR: CountingAllocator<std::alloc::System> =
    CountingAllocator::new(std::alloc::System);

/// Frames per block. 128 at 48 kHz is 2.7 milliseconds — a low-latency setting a
/// performer would actually choose, and the least forgiving of the common sizes.
const BLOCK_FRAMES: usize = 128;

/// Blocks to render. About a minute of audio at this block size.
const BLOCKS: usize = 25_000;

const RATE: SampleRate = SampleRate::HZ_48000;

#[test]
fn a_channel_strip_under_constant_control_movement_allocates_nothing() {
    let config = PrepareConfig::new(RATE, BLOCK_FRAMES as u32, 2);

    let mut gain = Gain::new();
    let mut eq = ThreeBandEq::new();
    let mut filter = DjFilter::new();
    gain.prepare(&config);
    eq.prepare(&config);
    filter.prepare(&config);

    let mut buffer = AudioBuffer::new(2, BLOCK_FRAMES).expect("valid buffer shape");
    let ctx = ProcessContext::new(BLOCK_FRAMES, RATE);

    let mut transport = Transport::new(RATE, Tempo::BPM_128, TimeSignature::FOUR_FOUR);
    transport.apply(TransportEvent::Load).expect("load");
    transport
        .apply(TransportEvent::LoadSucceeded)
        .expect("load succeeded");
    transport.apply(TransportEvent::Play).expect("play");
    transport
        .set_loop(prv_time::Frames::new(0), prv_time::Frames::new(96_000))
        .expect("valid loop");
    transport.enable_loop();

    // One warm-up block, excluded so that no first-call initialisation inside
    // the standard library is attributed to the signal path.
    fill(&mut buffer, 0);
    gain.process(&ctx, &mut buffer);
    eq.process(&ctx, &mut buffer);
    filter.process(&ctx, &mut buffer);
    transport.advance(BLOCK_FRAMES as u32);

    let scope = AllocationScope::begin();

    for block in 0..BLOCKS {
        // Move every control. These are the calls a performer's hands produce,
        // and each one touches a smoother, a coefficient design or both.
        let phase = block % 256;
        let sweep = (phase as f32 / 255.0) * 2.0 - 1.0;
        gain.set_level(0.5 + 0.5 * sweep.abs());
        eq.set_gains(1.0 - sweep.abs(), 1.0, 0.5 + 0.5 * sweep.abs());
        filter.set_position(sweep);

        fill(&mut buffer, block);
        let frames = transport.frames_until_wrap(BLOCK_FRAMES as u32);
        gain.process(&ctx, &mut buffer);
        eq.process(&ctx, &mut buffer);
        filter.process(&ctx, &mut buffer);
        transport.advance(frames);

        // A seek clears filter state, which is the other path a parameter
        // change can take.
        if block % 1_000 == 0 {
            eq.reset();
            filter.reset();
        }
    }

    assert_eq!(
        scope.allocations(),
        0,
        "the signal path allocated {} times across {BLOCKS} blocks; \
         a single allocation in a 2.7 ms callback is an audible dropout",
        scope.allocations()
    );
    assert_eq!(
        scope.deallocations(),
        0,
        "the signal path ran {} destructors across {BLOCKS} blocks; \
         no destructor may run on the audio thread",
        scope.deallocations()
    );

    assert!(
        buffer.as_slice().iter().all(|sample| sample.is_finite()),
        "the chain produced a non-finite sample"
    );
    assert!(
        buffer.peak() > 0.0,
        "the measurement must have processed real audio"
    );
}

#[test]
fn dispatching_through_a_chain_adds_no_allocation() {
    let config = PrepareConfig::new(RATE, BLOCK_FRAMES as u32, 2);

    let mut chain = Chain::new();
    chain.push(Box::new(Gain::new())).expect("chain has room");
    chain
        .push(Box::new(ThreeBandEq::new()))
        .expect("chain has room");
    chain
        .push(Box::new(DjFilter::new()))
        .expect("chain has room");
    // The limiter last, where it sits on a master. It is the processor with the
    // most state — two rings per channel and a polyphase filter bank — so if
    // anything in this crate were going to allocate mid-render, it would.
    chain
        .push(Box::new(Limiter::new()))
        .expect("chain has room");
    // The meter after the limiter, where a master meter belongs: what it shows
    // is what leaves. It changes nothing and is bound by the same contract.
    chain
        .push(Box::new(LoudnessMeter::new()))
        .expect("chain has room");
    chain.prepare(&config);

    let mut buffer = AudioBuffer::new(2, BLOCK_FRAMES).expect("valid buffer shape");
    let ctx = ProcessContext::new(BLOCK_FRAMES, RATE);

    fill(&mut buffer, 0);
    chain.process(&ctx, &mut buffer);

    let scope = AllocationScope::begin();

    for block in 0..BLOCKS {
        // Bypass switching clears processor state, so it exercises a path that
        // steady-state rendering does not.
        if block % 250 == 0 {
            chain.set_bypassed(1, (block / 250) % 2 == 0);
        }
        fill(&mut buffer, block);
        chain.process(&ctx, &mut buffer);
        let _ = chain.latency_frames();
    }

    assert_eq!(
        scope.allocations(),
        0,
        "chain dispatch allocated {} times",
        scope.allocations()
    );
    assert_eq!(scope.deallocations(), 0);
}

/// Fills the buffer with a deterministic signal.
///
/// A sine rather than silence: silence would let the denormal flushing take a
/// path it does not take in production, and would make the measurement easier
/// than reality.
fn fill(buffer: &mut AudioBuffer, block: usize) {
    let frames = buffer.frames();
    for channel_index in 0..buffer.channels() {
        if let Some(channel) = buffer.channel_mut(channel_index) {
            for (offset, sample) in channel.iter_mut().enumerate() {
                let phase = (block * frames + offset) as f32 * 0.01;
                *sample = 0.5 * phase.sin();
            }
        }
    }
}

/// The streaming components are not `Processor`s — they exist precisely because
/// the number of samples in and the number out differ — so the two tests above
/// do not reach them. They run in the same callback and are bound by the same
/// contract, and their write/read paths copy between buffers, which is exactly
/// where a `to_vec` slips in.
///
/// It did, in the first version of both. This is the test that would have
/// caught it.
#[test]
fn the_streaming_components_allocate_nothing_while_running() {
    let config = PrepareConfig::new(RATE, BLOCK_FRAMES as u32, 2);

    let mut stretch = TimeStretch::new();
    stretch.prepare(&config);
    stretch.set_ratio(1.25);

    let mut resampler = Resampler::new();
    resampler.prepare(&config);
    resampler.set_rate(1.25);

    let mut shift = PitchShift::new();
    shift.prepare(&config).expect("a valid configuration");
    shift.set_semitones(3.0);

    let mut input = AudioBuffer::new(2, BLOCK_FRAMES).expect("valid buffer shape");
    let mut output = AudioBuffer::new(2, BLOCK_FRAMES).expect("valid buffer shape");

    // Prime outside the measurement: the first frames are where the buffers
    // would grow if they were going to, and measuring only the steady state
    // would miss it. Priming here and measuring afterwards is deliberate — the
    // loop below runs long enough to cover every path several hundred times.
    fill(&mut input, 0);
    let _ = stretch.write(&input, BLOCK_FRAMES);
    let _ = resampler.write(&input, BLOCK_FRAMES);
    let _ = shift.write(&input, BLOCK_FRAMES);

    let scope = AllocationScope::begin();

    for block in 0..BLOCKS {
        fill(&mut input, block);

        // A ratio that moves, because a fader moves. `set_rate` is documented as
        // a control-thread call and may rebuild a kernel; it must still not
        // allocate, because the kernel it writes into already exists.
        if block % 500 == 0 {
            let step = ((block / 500) % 5) as f64;
            let sweep = 0.05_f64.mul_add(step, 1.0);
            stretch.set_ratio(sweep);
            resampler.set_rate(sweep);
            shift.set_semitones(step - 2.0);
        }

        let _ = stretch.write(&input, BLOCK_FRAMES);
        while stretch.read(&mut output, BLOCK_FRAMES) > 0 {}

        let _ = resampler.write(&input, BLOCK_FRAMES);
        while resampler.read(&mut output, BLOCK_FRAMES) > 0 {}

        let _ = shift.write(&input, BLOCK_FRAMES);
        while shift.read(&mut output, BLOCK_FRAMES) > 0 {}
    }

    assert_eq!(
        scope.allocations(),
        0,
        "the streaming components allocated {} times",
        scope.allocations()
    );
    assert_eq!(scope.deallocations(), 0);
}
