//! Compiles a C program against the generated header and runs it.
//!
//! # Why a Rust test that calls a C compiler
//!
//! Every other test in this crate calls the exported functions from Rust, which
//! proves the logic and proves nothing about the boundary. Rust calling a Rust
//! `extern "C"` function does not exercise the header, does not exercise the
//! linker, and would keep passing if the header said `int32_t` where the library
//! meant `int64_t`.
//!
//! This test is the one that would catch that. It writes a C host, compiles it
//! against `PRVBridge.h` as generated and committed, links it against the real
//! static library, runs it, and requires it to exit zero.
//!
//! # What it does not cover
//!
//! The Apple platform's own linker and calling convention. This runs on whatever
//! host builds the repository, which today is Linux on x86-64. A mismatch that
//! only appears on arm64-apple-darwin would not be caught here — but a mismatch
//! that appears anywhere is caught somewhere, which is the difference between a
//! boundary that has been tested and one that has been read.
//!
//! # Skipped rather than failed when there is no compiler
//!
//! A contributor without `cc` on their path should not see a red test they
//! cannot act on. Continuous integration has one, so the coverage is real where
//! it counts.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    clippy::print_stdout,
    reason = "a test that cannot build its own fixture should fail loudly, and \
              this one reports what it skipped"
)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// The C host. Written here rather than in a `.c` file so that what is compiled
/// and what is read are the same text.
const PROGRAM: &str = r#"
#include <stdio.h>
#include <string.h>
#include "PRVBridge.h"

static int failures = 0;

#define CHECK(condition, message)                                              \
    do {                                                                       \
        if (!(condition)) {                                                    \
            fprintf(stderr, "FAIL %s:%d: %s\n", __FILE__, __LINE__, message);  \
            failures += 1;                                                     \
        }                                                                      \
    } while (0)

/* A host-side decoder, in the shape a real one has: it owns its memory and
 * hands the core a view of it without allocating. */
typedef struct {
    float value;
    uint32_t available;
    uint32_t calls;
} FakeDecoder;

static uint32_t read_audio(void *user_data,
                           uint64_t track,
                           int64_t source_offset,
                           float *planar,
                           uint32_t channels,
                           uint32_t capacity,
                           uint32_t destination,
                           uint32_t frames) {
    FakeDecoder *decoder = (FakeDecoder *)user_data;
    uint32_t wanted = frames < decoder->available ? frames : decoder->available;
    (void)track;
    (void)source_offset;

    decoder->calls += 1;
    for (uint32_t channel = 0; channel < channels; channel += 1) {
        for (uint32_t frame = 0; frame < wanted; frame += 1) {
            planar[channel * capacity + destination + frame] = decoder->value;
        }
    }
    return wanted;
}

int main(void) {
    /* Rule one: ask the version before anything else. */
    uint32_t version = prv_abi_version();
    CHECK(version != 0, "the library reported no version at all");
    CHECK((version >> 16) == PRV_ABI_MAJOR,
          "the library and this header disagree about the major version");
    CHECK(prv_abi_is_compatible(PRV_ABI_MAJOR) != 0,
          "the library rejected the version its own header describes");
    CHECK(prv_abi_is_compatible(PRV_ABI_MAJOR + 1) == 0,
          "the library accepted a major version it cannot serve");

    /* A status message is always readable. */
    const char *message = prv_status_message(PRV_INVALID_STATE);
    CHECK(message != NULL, "a status had no message");
    CHECK(strlen(message) > 0, "a status message was empty");

    /* Make an engine. */
    PrvEngine *engine = NULL;
    int32_t status = prv_engine_create(48000, 2, 512, &engine);
    CHECK(status == PRV_OK, "the engine would not start");
    CHECK(engine != NULL, "a successful create returned no handle");

    /* A failed create must leave a null handle rather than a stale one. */
    PrvEngine *doomed = (PrvEngine *)0x1234;
    status = prv_engine_create(0, 2, 512, &doomed);
    CHECK(status == PRV_INVALID_ARGUMENT, "an impossible rate was accepted");
    CHECK(doomed == NULL, "a failed create left a dangling pointer");

    /* Register the decoder and place a track. */
    FakeDecoder decoder = {0.25f, 8192, 0};
    status = prv_engine_set_source(engine, read_audio, &decoder);
    CHECK(status == PRV_OK, "the source would not attach");

    uint64_t placement = 0;
    status = prv_engine_place_track(engine, 42, 0, 4096, 0, 0, 1000, &placement);
    CHECK(status == PRV_OK, "the track would not place");
    CHECK(placement != 0, "a placement identity of zero is not a name");

    int64_t duration = 0;
    status = prv_engine_duration(engine, &duration);
    CHECK(status == PRV_OK, "the duration could not be read");
    CHECK(duration == 4096, "the project is not as long as the track placed in it");

    uint64_t count = 0;
    status = prv_engine_placement_count(engine, &count);
    CHECK(status == PRV_OK, "the placement count could not be read");
    CHECK(count == 1, "the project does not hold the one placement it was given");

    /* Playing an empty deck is refused, and saying so is the point. */
    status = prv_engine_transport(engine, PRV_EVENT_PLAY);
    CHECK(status == PRV_INVALID_STATE, "play on an unloaded deck was accepted");

    /* Load, ready, play. */
    CHECK(prv_engine_transport(engine, PRV_EVENT_LOAD) == PRV_OK, "load refused");
    CHECK(prv_engine_transport(engine, PRV_EVENT_LOAD_SUCCEEDED) == PRV_OK,
          "load-succeeded refused");
    CHECK(prv_engine_transport(engine, PRV_EVENT_PLAY) == PRV_OK, "play refused");

    int32_t state = -1;
    status = prv_engine_playback_state(engine, &state);
    CHECK(status == PRV_OK, "the playback state could not be read");
    CHECK(state == PRV_PLAYBACK_PLAYING, "the transport is not playing");

    /* Render, which is the only call the audio thread makes. */
    float block[2 * 256];
    memset(block, 0, sizeof block);
    status = prv_engine_render(engine, block, 2, 256);
    CHECK(status == PRV_OK, "the render failed");
    CHECK(decoder.calls > 0, "the renderer never asked the host for audio");

    int heard = 0;
    for (size_t index = 0; index < sizeof block / sizeof block[0]; index += 1) {
        if (block[index] != 0.0f) {
            heard = 1;
            break;
        }
    }
    CHECK(heard, "the block came back silent");

    int32_t complete = 0;
    status = prv_engine_render_was_complete(engine, &complete);
    CHECK(status == PRV_OK, "completeness could not be read");
    CHECK(complete == 1, "a fully-read placement reported as incomplete");

    /* The playhead moved by exactly one block. */
    int64_t position = -1;
    status = prv_engine_position(engine, &position);
    CHECK(status == PRV_OK, "the position could not be read");
    CHECK(position == 256, "the playhead did not advance by one block");

    /* Seeking puts it where it was asked. */
    CHECK(prv_engine_seek(engine, 1024) == PRV_OK, "the seek failed");
    status = prv_engine_position(engine, &position);
    CHECK(status == PRV_OK, "the position could not be read after seeking");
    CHECK(position == 1024, "the playhead is not where it was sent");

    /* A block bigger than the engine was built for is refused, not truncated. */
    status = prv_engine_render(engine, block, 2, 1024);
    CHECK(status == PRV_INVALID_ARGUMENT, "an oversized block was accepted");

    /* Synchronisation: two projects, and only bytes between them.
     *
     * This is the shape a real host uses. The core never opens a socket — it
     * produces bytes and reads bytes, and what carries them is the host's
     * business. */
    PrvEngine *peer = NULL;
    CHECK(prv_engine_create(48000, 2, 512, &peer) == PRV_OK, "the peer would not start");
    CHECK(prv_engine_set_device(peer, 2) == PRV_OK, "the peer would not take an identity");
    CHECK(prv_engine_set_device(peer, 0) == PRV_INVALID_ARGUMENT,
          "zero was accepted as a device identity");

    /* Asking with nowhere to put the answer reports the size. */
    uint64_t needed = 0;
    CHECK(prv_engine_sync_state(peer, NULL, 0, &needed) == PRV_BUFFER_TOO_SMALL,
          "a zero-length buffer was reported as sufficient");
    CHECK(needed > 0, "an empty project claimed to have no state at all");

    uint8_t peer_state[256];
    CHECK(needed <= sizeof peer_state, "the peer's state does not fit a small buffer");
    CHECK(prv_engine_sync_state(peer, peer_state, sizeof peer_state, &needed) == PRV_OK,
          "the peer's state could not be read");

    uint64_t message_len = 0;
    CHECK(prv_engine_sync_prepare(engine, peer_state, needed, &message_len) == PRV_OK,
          "the message could not be prepared");
    CHECK(message_len > 0, "a project with a placement in it had nothing to send");

    uint8_t outbound[4096];
    uint64_t written = 0;
    CHECK(message_len <= sizeof outbound, "the message does not fit the test's buffer");
    CHECK(prv_engine_sync_outbound(engine, outbound, sizeof outbound, &written) == PRV_OK,
          "the message could not be copied out");
    CHECK(written == message_len, "the message changed size between being sized and sent");

    uint64_t applied = 0, already = 0, conflicts = 0, carried = 0;
    CHECK(prv_engine_sync_merge(peer, outbound, written, &applied, &already, &conflicts,
                                &carried) == PRV_OK,
          "the peer would not merge the message");
    CHECK(applied > 0, "nothing arrived");
    CHECK(conflicts == 0, "an exchange between an empty project and a full one conflicted");
    CHECK(carried == 0, "this build could not read a message it wrote itself");

    int64_t peer_duration = 0;
    CHECK(prv_engine_duration(peer, &peer_duration) == PRV_OK,
          "the peer's duration could not be read");
    CHECK(peer_duration == duration,
          "the peer's project is not the same length as the one it received");

    /* A network that loses a reply makes a client send again. The second
     * delivery is recognised rather than duplicated. */
    CHECK(prv_engine_sync_merge(peer, outbound, written, &applied, &already, &conflicts,
                                &carried) == PRV_OK,
          "a repeated delivery was refused");
    CHECK(applied == 0, "a repeated delivery applied work twice");
    CHECK(already > 0, "a repeated delivery was not recognised as one");

    /* And the peer now has nothing to ask for. */
    CHECK(prv_engine_sync_state(peer, peer_state, sizeof peer_state, &needed) == PRV_OK,
          "the peer's state could not be read after merging");
    CHECK(prv_engine_sync_prepare(engine, peer_state, needed, &message_len) == PRV_OK,
          "a second exchange could not be prepared");
    CHECK(message_len > 0, "an empty message is still a message");
    CHECK(message_len < written, "the second exchange sent as much as the first");

    /* Bytes that are not a message are refused rather than guessed at. */
    CHECK(prv_engine_sync_merge(peer, (const uint8_t *)"not a message at all", 20, &applied,
                                &already, &conflicts, &carried) == PRV_INVALID_ARGUMENT,
          "arbitrary bytes were accepted as a project");
    CHECK(prv_engine_sync_prepare(engine, (const uint8_t *)"nor is this", 11, &message_len)
              == PRV_INVALID_ARGUMENT,
          "arbitrary bytes were accepted as a version vector");

    /* An operation this build cannot read is kept and passed on, not dropped.
     *
     * The message is written by hand because the encoder cannot produce one:
     * it is what a build that does not exist yet would send. */
    uint8_t future[] = {
        'P', 'R', 'V', 'L',           /* magic */
        1, 0,                          /* major */
        0, 0,                          /* minor */
        4, 0, 0, 0,                    /* header length */
        1, 0, 0, 0,                    /* one entry */
        38, 0, 0, 0,                   /* entry length */
        3, 0, 0, 0, 0, 0, 0, 0,        /* device 3 */
        1, 0, 0, 0, 0, 0, 0, 0,        /* sequence 1 */
        0, 0, 0, 0, 0, 0, 0, 0,        /* timestamp */
        0, 0, 0, 0,                    /* empty context */
        0x60, 0xEA,                    /* a payload kind from the future */
        4, 0, 0, 0,                    /* payload length */
        's', 'o', 'o', 'n'
    };

    CHECK(prv_engine_sync_merge(peer, future, sizeof future, &applied, &already, &conflicts,
                                &carried) == PRV_OK,
          "a message from a newer build was refused outright");
    CHECK(applied == 0, "an unreadable operation was applied");
    CHECK(carried == 1, "an unreadable operation was not kept");

    uint64_t held = 0;
    CHECK(prv_engine_carried_count(peer, &held) == PRV_OK, "the carried count could not be read");
    CHECK(held == 1, "the operation was counted but not held");

    /* And it is still beyond this build, so promoting keeps rather than loses it. */
    uint64_t promoted = 0;
    CHECK(prv_engine_promote_carried(peer, &promoted) == PRV_OK, "promoting failed");
    CHECK(promoted == 0, "this build claimed to understand next year's work");
    CHECK(prv_engine_carried_count(peer, &held) == PRV_OK, "the carried count could not be read");
    CHECK(held == 1, "an operation was lost to a promotion attempt");

    /* It travels onward in the next message this device sends. */
    PrvEngine *onward = NULL;
    CHECK(prv_engine_create(48000, 2, 512, &onward) == PRV_OK, "the third device would not start");
    CHECK(prv_engine_set_device(onward, 4) == PRV_OK, "the third device would not take a name");
    CHECK(prv_engine_sync_state(onward, peer_state, sizeof peer_state, &needed) == PRV_OK,
          "the third device's state could not be read");
    CHECK(prv_engine_sync_prepare(peer, peer_state, needed, &message_len) == PRV_OK,
          "the relay could not prepare a message");
    CHECK(message_len <= sizeof outbound, "the relayed message does not fit the test's buffer");
    CHECK(prv_engine_sync_outbound(peer, outbound, sizeof outbound, &written) == PRV_OK,
          "the relayed message could not be copied out");
    CHECK(prv_engine_sync_merge(onward, outbound, written, &applied, &already, &conflicts,
                                &carried) == PRV_OK,
          "the third device would not merge the relayed message");
    CHECK(carried == 1, "the relay did not pass on what it could not read");
    CHECK(prv_engine_carried_count(onward, &held) == PRV_OK, "the carried count could not be read");
    CHECK(held == 1, "the third device did not keep the relayed work");
    prv_engine_destroy(onward);

    /* An identity cannot be changed underneath a log that has already used it. */
    CHECK(prv_engine_set_device(peer, 3) == PRV_INVALID_STATE,
          "the peer changed identity after it had already merged work");

    prv_engine_destroy(peer);

    /* "What goes with this?" — asked of a library, without a set. */
    PrvPlanner *shelf = NULL;
    CHECK(prv_planner_create(&shelf) == PRV_OK, "the planner would not start");
    for (uint64_t index = 0; index < 6; index += 1) {
        CHECK(prv_planner_add_candidate(shelf, index, 48000 * 300, 128.0 + (double)(index % 3),
                                        0.5f, 9, 1, 1.0f, -8.0f, 0) == PRV_OK,
              "a record would not go into the library");
    }

    uint64_t found = 0;
    CHECK(prv_planner_neighbours(shelf, 0, 1, 4, &found) == PRV_OK, "the ranking failed");
    CHECK(found == 4, "the ranking did not honour the limit asked for");

    double previous = 2.0;
    for (uint64_t index = 0; index < found; index += 1) {
        uint64_t neighbour_track = 0;
        float neighbour_score = 0.0f;
        int32_t weakest = 0;
        CHECK(prv_planner_neighbour(shelf, index, &neighbour_track, &neighbour_score,
                                    &weakest) == PRV_OK,
              "a ranked row could not be read");
        CHECK(neighbour_track != 0, "a record was returned as its own neighbour");
        CHECK(neighbour_score >= 0.0f && neighbour_score <= 1.0f, "a score left its range");
        CHECK((double)neighbour_score <= previous, "the ranking was not sorted");
        CHECK(weakest >= PRV_COMPONENT_HARMONIC && weakest <= PRV_COMPONENT_VOCAL,
              "the weakest component is not one this header defines");
        previous = (double)neighbour_score;
    }

    /* Asking about a record nobody imported is refused, not answered emptily. */
    CHECK(prv_planner_neighbours(shelf, 9999, 1, 4, &found) == PRV_INVALID_ARGUMENT,
          "a record nobody imported was ranked anyway");
    float spare_score = 0.0f;
    CHECK(prv_planner_neighbour(shelf, 999, &placement, &spare_score, &state)
              == PRV_INVALID_ARGUMENT,
          "a row past the end was read");
    prv_planner_destroy(shelf);

    /* Where synchronisation is: a separate handle, because it belongs to an
     * installation rather than to a project. */
    PrvSync *sync = NULL;
    CHECK(prv_sync_create(&sync) == PRV_OK, "the sync state would not start");

    int32_t sync_state = 0;
    CHECK(prv_sync_state(sync, &sync_state) == PRV_OK, "the sync state could not be read");
    CHECK(sync_state == PRV_SYNC_OFFLINE, "a fresh installation was optimistic");

    int32_t editing = 0, transferring = 0, needs_user = 0;
    CHECK(prv_sync_flags(sync, &editing, &transferring, &needs_user) == PRV_OK,
          "the sync flags could not be read");
    CHECK(editing != 0, "editing was refused while offline");

    /* An evening's work with no network, and no complaint about it. */
    for (uint64_t sequence = 1; sequence <= 32; sequence += 1) {
        CHECK(prv_sync_hold(sync, 1, sequence) == PRV_OK, "the outbox refused work");
    }
    uint64_t waiting = 0;
    int32_t nearly_full = 0;
    CHECK(prv_sync_waiting(sync, &waiting, &nearly_full) == PRV_OK, "the outbox could not be read");
    CHECK(waiting == 32, "the outbox lost work");
    CHECK(nearly_full == 0, "thirty-two edits were reported as a backlog");

    /* A conflict stops the transfer and nothing else. */
    CHECK(prv_sync_apply(sync, PRV_SYNC_EVENT_NETWORK_AVAILABLE) == PRV_OK, "event refused");
    CHECK(prv_sync_apply(sync, PRV_SYNC_EVENT_WORK_TO_SEND) == PRV_OK, "event refused");
    CHECK(prv_sync_apply(sync, PRV_SYNC_EVENT_CONFLICT_FOUND) == PRV_OK, "event refused");
    CHECK(prv_sync_flags(sync, &editing, &transferring, &needs_user) == PRV_OK,
          "the sync flags could not be read");
    CHECK(needs_user != 0, "a conflict did not ask the user");
    CHECK(transferring == 0, "a conflict did not stop the transfer");
    CHECK(editing != 0, "an unanswered question stopped the user working");

    /* Acknowledging twice costs nothing, which is what makes a lost reply safe. */
    CHECK(prv_sync_acknowledge(sync, 1, 1) == PRV_OK, "acknowledgement refused");
    CHECK(prv_sync_acknowledge(sync, 1, 1) == PRV_OK, "a repeated acknowledgement was refused");
    CHECK(prv_sync_waiting(sync, &waiting, &nearly_full) == PRV_OK, "the outbox could not be read");
    CHECK(waiting == 31, "acknowledging twice removed two entries");

    /* An event this version does not define is refused, not guessed at. */
    CHECK(prv_sync_apply(sync, 9999) == PRV_INVALID_ARGUMENT, "an undefined sync event applied");
    prv_sync_destroy(sync);
    prv_sync_destroy(NULL);

    /* Null is refused rather than dereferenced. */
    CHECK(prv_engine_transport(NULL, PRV_EVENT_PLAY) == PRV_NULL_POINTER,
          "a null handle was dereferenced");
    CHECK(prv_engine_position(engine, NULL) == PRV_NULL_POINTER,
          "a null out-parameter was written through");

    /* An event this version does not define is refused, not guessed. */
    CHECK(prv_engine_transport(engine, 9999) == PRV_INVALID_ARGUMENT,
          "an undefined transport event was applied");

    prv_engine_destroy(engine);
    prv_engine_destroy(NULL);

    if (failures == 0) {
        printf("the C host drove the boundary end to end\n");
    }
    return failures == 0 ? 0 : 1;
}
"#;

/// The directory `cargo` puts build output in, found by walking up from the test
/// binary. `CARGO_TARGET_DIR` is not exported to tests, so it is derived.
fn target_dir() -> Option<PathBuf> {
    let mut path = std::env::current_exe().ok()?;
    // .../target/debug/deps/c_host-<hash>
    path.pop();
    path.pop();
    Some(path)
}

/// Finds the static library cargo built for this crate.
fn static_library(target: &Path) -> Option<PathBuf> {
    // Only one name today. Kept as a lookup rather than inlined because the
    // Windows spelling is `prv_ffi.lib`, and the day that matters this becomes a
    // list rather than a rewrite.
    let candidate = target.join("libprv_ffi.a");
    candidate.exists().then_some(candidate)
}

#[test]
fn a_c_host_can_drive_the_boundary_through_the_generated_header() {
    let Ok(compiler_check) = Command::new("cc").arg("--version").output() else {
        println!("skipped: no C compiler on this machine");
        return;
    };
    if !compiler_check.status.success() {
        println!("skipped: the C compiler on this machine does not run");
        return;
    }

    let Some(target) = target_dir() else {
        println!("skipped: the build directory could not be located");
        return;
    };
    let Some(library) = static_library(&target) else {
        // The staticlib is produced by `cargo build`, and `cargo test` builds it
        // too — but only once the crate type has been compiled at least once in
        // this profile. Skipping is honest; failing would be a test that reports
        // on cargo's scheduling rather than on the boundary.
        println!(
            "skipped: {} has not been built yet; run `cargo build -p prv-ffi` first",
            target.display()
        );
        return;
    };

    let scratch = target.join("c_host_test");
    std::fs::create_dir_all(&scratch).expect("the scratch directory could not be made");
    let source = scratch.join("host.c");
    std::fs::write(&source, PROGRAM).expect("the C host could not be written");

    let header_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../apple/PRVKit/Bridge/Generated")
        .canonicalize()
        .expect("the generated header directory is missing; run bridgegen");

    let binary = scratch.join("host");
    let compile = Command::new("cc")
        .arg("-std=c11")
        .arg("-Wall")
        .arg("-Wextra")
        // A warning at this boundary is a type mismatch between the header and
        // the library, which is the whole thing this test exists to find.
        .arg("-Werror")
        .arg("-I")
        .arg(&header_dir)
        .arg(&source)
        .arg(&library)
        .arg("-o")
        .arg(&binary)
        // The static library needs the platform's threading and maths symbols.
        .args(["-lpthread", "-ldl", "-lm"])
        .output()
        .expect("the C compiler could not be run");

    assert!(
        compile.status.success(),
        "the C host did not compile against the generated header:\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&binary)
        .output()
        .expect("the compiled C host could not be run");

    assert!(
        run.status.success(),
        "the C host failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
}

#[test]
fn the_committed_header_is_what_the_generator_produces() {
    // The drift gate, as a test as well as a continuous-integration step. A
    // header that has fallen behind the library is the defect this whole
    // generated-bindings arrangement exists to prevent, and finding it in
    // `cargo test` is faster than finding it in a pull request.
    let header = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../apple/PRVKit/Bridge/Generated/PRVBridge.h");

    let committed =
        std::fs::read_to_string(&header).expect("the generated header is missing; run bridgegen");

    // The version the header claims must be the version the library reports.
    let expected = format!("#define PRV_ABI_MAJOR {}", prv_ffi::abi::MAJOR);
    assert!(
        committed.contains(&expected),
        "the committed header does not describe this library's major version"
    );

    for status in prv_ffi::Status::ALL {
        let line = format!("{} = {},", status.c_name(), status.code());
        assert!(
            committed.contains(&line),
            "the committed header is missing or disagrees about `{line}`"
        );
    }
}
