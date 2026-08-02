//! probe
#![allow(missing_docs, clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic, clippy::print_stdout)]
use prv_dsp::PrepareConfig;
use prv_dsp::Resampler;
use prv_rt::AudioBuffer;
use prv_time::SampleRate;

fn run(rate: f64) {
    let cfg = PrepareConfig::new(SampleRate::new(48_000).unwrap(), 256, 1);
    let mut r = Resampler::new();
    r.prepare(&cfg);
    r.set_rate(rate);
    let mut inb = AudioBuffer::new(1, 256).unwrap();
    let mut outb = AudioBuffer::new(1, 256).unwrap();
    for s in inb.channel_mut(0).unwrap().iter_mut() {
        *s = 0.5;
    }
    let mut out: Vec<f32> = Vec::new();
    for _ in 0..40 {
        let _ = r.write(&inb, 256);
        loop {
            let n = r.read(&mut outb, 256);
            if n == 0 {
                break;
            }
            out.extend_from_slice(&outb.channel(0).unwrap()[..n]);
        }
    }
    let settled = &out[200..out.len() - 200];
    let lo = settled.iter().cloned().fold(f32::INFINITY, f32::min);
    let hi = settled.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    println!(
        "rate {rate}: constant 0.5 -> [{lo:.6}, {hi:.6}]  ripple {:.4} dB",
        20.0 * (hi / lo).log10()
    );
}

#[test]
fn probe() {
    for rate in [1.0, 0.8, 1.1, 1.189207, 1.25, 2.0] {
        run(rate);
    }
}
