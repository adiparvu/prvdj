//! probe
#![allow(missing_docs, clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::cast_possible_wrap)]
use prv_project::{DeviceId, OperationLog, OperationPayload, PlacementId, ProjectState, TrackRef};
use prv_time::{BeatGrid, Frames, SampleRate, Tempo, TimeSignature};
use prv_timeline::{Clip, Snap, Timeline};

fn grid() -> BeatGrid {
    BeatGrid::new(SampleRate::HZ_44100, Tempo::from_bpm(120.0).unwrap(), TimeSignature::FOUR_FOUR, Frames::ZERO)
}

#[test]
fn probe_undo_of_delete_loses_source_offset() {
    let mut log = OperationLog::new();
    let d1 = DeviceId::new(1);
    for p in [
        OperationPayload::PlaceTrack { placement: PlacementId::new(1), track: TrackRef::new(1), position: Frames::new(0), length: Frames::new(88_200*4), lane: 0 },
        OperationPayload::SetPlacementSource { placement: PlacementId::new(1), source_offset: Frames::new(88_200) },
        OperationPayload::RemovePlacement { placement: PlacementId::new(1) },
    ] {
        let op = log.author(d1, 0, p);
        log.append(op).unwrap();
    }
    let undo = log.undo_for(d1, 0).expect("undo exists");
    println!("undo payload = {:?}", undo.payload);
    log.append(undo).unwrap();
    let st = log.state();
    println!("restored = {:?}", st.placements.get(&PlacementId::new(1)));
    assert_eq!(st.placements.get(&PlacementId::new(1)).map(|p| p.source_offset), Some(Frames::new(88_200)),
        "undo of a delete lost the source offset");
}

#[test]
fn probe_add_drops_source_offset() {
    let mut tl = Timeline::new();
    let c = Clip::new(PlacementId::new(1), TrackRef::new(1), 0, Frames::new(0), Frames::new(88_200*4))
        .with_source_offset(Frames::new(1000));
    let edit = tl.add(c, &grid(), Snap::Off).unwrap();
    println!("ops = {:?}", edit.operations());
    let mut state = ProjectState::default();
    for op in edit.operations() { state.apply(op); }
    let (rebuilt, skipped) = Timeline::from_project(&state);
    assert_eq!(skipped, 0);
    let live = tl.clip(PlacementId::new(1)).unwrap().source_offset();
    let folded = rebuilt.clip(PlacementId::new(1)).unwrap().source_offset();
    println!("live={live:?} folded={folded:?}");
    assert_eq!(live, folded, "the refold disagrees with the live timeline");
}

#[test]
fn probe_branch_same_device_collides() {
    let mut log = OperationLog::new();
    let d1 = DeviceId::new(1);
    for id in [1u64, 2] {
        let op = log.author(d1, 0, OperationPayload::PlaceTrack { placement: PlacementId::new(id), track: TrackRef::new(id), position: Frames::new(id as i64 * 100_000), length: Frames::new(48_000), lane: 0 });
        log.append(op).unwrap();
    }
    let mut branch = log.branch_at(1).unwrap();
    let op = branch.author(d1, 0, OperationPayload::PlaceTrack { placement: PlacementId::new(9), track: TrackRef::new(9), position: Frames::new(900_000), length: Frames::new(48_000), lane: 0 });
    println!("branch op id = {}", op.id);
    branch.append(op).unwrap();

    let to_trunk = branch.operations_since(log.version_vector());
    println!("to_trunk = {} ops", to_trunk.len());
    let report = log.merge(&to_trunk).unwrap();
    println!("report = {report}");
    let st = log.state();
    println!("trunk placements = {:?}", st.placements.keys().collect::<Vec<_>>());
    assert!(st.placements.contains_key(&PlacementId::new(9)), "the branch's edit vanished");
}

#[test]
fn probe_undo_clobbers_other_device() {
    let mut log = OperationLog::new();
    let d1 = DeviceId::new(1);
    let d2 = DeviceId::new(2);
    let op = log.author(d1, 0, OperationPayload::PlaceTrack { placement: PlacementId::new(1), track: TrackRef::new(1), position: Frames::new(0), length: Frames::new(48_000), lane: 0 });
    log.append(op).unwrap();
    let op = log.author(d1, 0, OperationPayload::MovePlacement { placement: PlacementId::new(1), position: Frames::new(200_000), lane: 0 });
    log.append(op).unwrap();
    // d2 has seen everything and makes its own, later, move
    let op = log.author(d2, 0, OperationPayload::MovePlacement { placement: PlacementId::new(1), position: Frames::new(300_000), lane: 0 });
    log.append(op).unwrap();
    assert_eq!(log.state().placements.get(&PlacementId::new(1)).unwrap().position, Frames::new(300_000));
    let undo = log.undo_for(d1, 0).unwrap();
    println!("d1 undo = {:?}", undo.payload);
    log.append(undo).unwrap();
    let pos = log.state().placements.get(&PlacementId::new(1)).unwrap().position;
    println!("after d1 undo, position = {pos:?}");
    assert_eq!(pos, Frames::new(300_000), "d1's undo reverted d2's later move");
}
