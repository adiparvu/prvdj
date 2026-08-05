import Foundation
import Testing

@testable import PRVCore
@testable import PRVKit
@testable import PRVUI

@Suite("The four spaces that used to be empty")
struct SpaceModelTests {

    private func clip(
        id: UInt64,
        track: UInt64,
        at position: Int64,
        length: Int64 = 48_000 * 60,
        lane: UInt32 = 0,
        title: String? = nil
    ) -> ClipModel {
        ClipModel(
            placement: Placement(
                id: id, track: track, position: position,
                length: length, lane: lane, sourceOffset: 0
            ),
            title: title
        )
    }

    // MARK: - Home

    @Test("an empty installation suggests importing, and says so with a dash")
    func emptyHome() {
        let home = HomeModel(trackCount: 0, projectFrames: 0, sampleRate: 48_000, planOptions: 0)

        #expect(home.isEmpty)
        #expect(home.nextStepKey == "home.next.import")
        // A dash rather than a zero: "0 minutes" reads as a measurement, and
        // there is nothing to measure yet.
        #expect(home.standings.first { $0.titleKey == "home.project" }?.value == "—")
    }

    @Test("the suggestion follows what is missing, one step at a time")
    func nextStep() {
        let withTracks = HomeModel(
            trackCount: 12, projectFrames: 0, sampleRate: 48_000, planOptions: 0
        )
        #expect(withTracks.nextStepKey == "home.next.plan")

        let withPlans = HomeModel(
            trackCount: 12, projectFrames: 0, sampleRate: 48_000, planOptions: 3
        )
        #expect(withPlans.nextStepKey == "home.next.adopt")

        let withSet = HomeModel(
            trackCount: 12, projectFrames: 48_000 * 600, sampleRate: 48_000, planOptions: 3
        )
        #expect(withSet.nextStepKey == "home.next.play")
    }

    @Test("home flags a conflict rather than burying it in a count")
    func homeAttention() {
        let conflicted = SyncModel(
            snapshot: SyncSnapshot(
                state: .conflicted, editingIsAllowed: true, isTransferring: false,
                needsTheUser: true, waiting: 4, isNearlyFull: false
            )
        )
        let home = HomeModel(
            trackCount: 3, projectFrames: 0, sampleRate: 48_000,
            planOptions: 0, sync: conflicted
        )

        let row = home.standings.first { $0.titleKey == "home.sync" }
        #expect(row?.needsAttention == true)
        #expect(row?.value == "4")
    }

    // MARK: - Mix editor

    @Test("clips are drawn in the order they are heard, not the order they arrive")
    func timeOrder() {
        // The boundary hands them over in identity order, which is what stops a
        // list reordering under the hand dragging it. Drawing wants time order.
        let model = MixEditorModel(
            clips: [
                clip(id: 3, track: 1, at: 96_000),
                clip(id: 1, track: 2, at: 0),
                clip(id: 2, track: 3, at: 48_000),
            ],
            projectFrames: 144_000,
            sampleRate: 48_000
        )

        #expect(model.inTimeOrder.map(\.id) == [1, 2, 3])
    }

    @Test("two clips starting together keep a stable order")
    func stableTiebreak() {
        // Without a tiebreak these could swap between frames, which is a
        // timeline that flickers while nothing is happening.
        let model = MixEditorModel(
            clips: [clip(id: 9, track: 1, at: 0), clip(id: 4, track: 2, at: 0, lane: 1)],
            projectFrames: 48_000,
            sampleRate: 48_000
        )
        #expect(model.inTimeOrder.map(\.id) == [4, 9])
        #expect(model.inTimeOrder.map(\.id) == model.inTimeOrder.map(\.id))
    }

    @Test("an empty editor still draws one lane to drop something into")
    func emptyEditorHasALane() {
        let model = MixEditorModel(clips: [], projectFrames: 0, sampleRate: 48_000)
        #expect(model.isEmpty)
        #expect(model.laneCount == 1)
        #expect(model.durationText == "—")
    }

    @Test("lanes are counted from the highest one in use")
    func laneCount() {
        let model = MixEditorModel(
            clips: [clip(id: 1, track: 1, at: 0), clip(id: 2, track: 2, at: 0, lane: 3)],
            projectFrames: 48_000,
            sampleRate: 48_000
        )
        #expect(model.laneCount == 4)
    }

    @Test("a clip's span is a fraction, and an empty project has no fractions")
    func spans() {
        let piece = clip(id: 1, track: 1, at: 24_000, length: 24_000)
        #expect(piece.span(inProjectOf: 96_000) == 0.25...0.5)
        // Rather than dividing by zero: a timeline with no length has nowhere
        // to put anything.
        #expect(piece.span(inProjectOf: 0) == nil)
    }

    @Test("a clip with no name is still identifiable")
    func clipFallback() {
        #expect(clip(id: 1, track: 7, at: 0).label == "track 7")
        #expect(clip(id: 1, track: 7, at: 0, title: "Kerri Chandler").label == "Kerri Chandler")
    }

    @Test("undo blocked by somebody else is a sentence, not a greyed button")
    func undoBlocked() {
        let superseded = MixEditorModel(
            clips: [], projectFrames: 0, sampleRate: 48_000, undo: .supersededByAnotherDevice
        )
        #expect(!superseded.canUndo)
        #expect(superseded.undoBlockedKey == "undo.superseded")

        // "Nothing to undo" is the disabled-button case and says nothing.
        let empty = MixEditorModel(
            clips: [], projectFrames: 0, sampleRate: 48_000, undo: .nothingToUndo
        )
        #expect(!empty.canUndo)
        #expect(empty.undoBlockedKey == nil)

        let ready = MixEditorModel(
            clips: [], projectFrames: 0, sampleRate: 48_000, undo: .available
        )
        #expect(ready.canUndo)
        #expect(ready.undoBlockedKey == nil)
    }

    // MARK: - Live

    private func liveModel(at position: Int64, complete: Bool = true) -> LiveModel {
        LiveModel(
            transport: TransportModel(
                snapshot: SessionSnapshot(
                    playback: .playing, position: position, duration: 144_000,
                    placementCount: 3, audioComplete: complete
                ),
                sampleRate: 48_000
            ),
            clips: [
                clip(id: 1, track: 1, at: 0, length: 48_000, title: "First"),
                clip(id: 2, track: 2, at: 48_000, length: 48_000, title: "Second"),
                clip(id: 3, track: 3, at: 96_000, length: 48_000, title: "Third"),
            ],
            positionFrames: position,
            sampleRate: 48_000,
            renderWasComplete: complete
        )
    }

    @Test("the performer is told what is playing and what is next")
    func nowAndNext() {
        let model = liveModel(at: 60_000)
        #expect(model.nowPlaying?.label == "Second")
        #expect(model.upNext?.label == "Third")
        #expect(model.timeToNext != nil)
    }

    @Test("at the end there is nothing next, and that is not an error")
    func endOfSet() {
        let model = liveModel(at: 140_000)
        #expect(model.nowPlaying?.label == "Third")
        #expect(model.upNext == nil)
        #expect(model.timeToNext == nil)
    }

    @Test("only the sound coming out right now may interrupt a performance")
    func onlyOneWarning() {
        // Master Prompt #19. There is deliberately no queue here for anything
        // else to be added to.
        #expect(liveModel(at: 0).warningKey == nil)
        #expect(liveModel(at: 0, complete: false).warningKey == "live.warning.audio_missing")
    }

    @Test("a gap between clips has nothing playing, and still knows what is next")
    func inAGap() {
        let model = LiveModel(
            transport: TransportModel(
                snapshot: SessionSnapshot(
                    playback: .playing, position: 24_000, duration: 96_000,
                    placementCount: 1, audioComplete: true
                ),
                sampleRate: 48_000
            ),
            clips: [clip(id: 1, track: 1, at: 48_000, length: 48_000, title: "Later")],
            positionFrames: 24_000,
            sampleRate: 48_000
        )
        #expect(model.nowPlaying == nil)
        #expect(model.upNext?.label == "Later")
    }

    // MARK: - Settings

    @Test("privacy comes first, and attention follows synchronisation")
    func settingsOrder() throws {
        let quiet = SettingsModel(
            consent: ConsentModel(rows: []),
            sync: SyncModel(snapshot: .offline),
            tierKey: "tier.free"
        )
        #expect(quiet.sectionKeys.first == "settings.privacy")
        #expect(!quiet.needsAttention)

        let noisy = SettingsModel(
            consent: ConsentModel(rows: []),
            sync: SyncModel(snapshot: .offline, carried: 2),
            tierKey: "tier.free"
        )
        #expect(noisy.needsAttention)
    }
}

@Suite("Projects on the home screen")
struct HomeProjectTests {

    @Test("the open project is not offered as something to open")
    func openOneIsExcluded() {
        let home = HomeModel(
            trackCount: 3, projectFrames: 0, sampleRate: 48_000, planOptions: 0,
            projects: ["friday", "saturday", "sunday"], openProject: "saturday"
        )
        #expect(home.otherProjects == ["friday", "sunday"])
    }

    @Test("with nothing saved there is nothing to list")
    func nothingSaved() {
        let home = HomeModel(trackCount: 0, projectFrames: 0, sampleRate: 48_000, planOptions: 0)
        #expect(home.otherProjects.isEmpty)
    }

    @Test("the list is ordered, so it does not reshuffle between launches")
    func ordered() {
        let home = HomeModel(
            trackCount: 0, projectFrames: 0, sampleRate: 48_000, planOptions: 0,
            projects: ["zeta", "alpha", "mid"], openProject: nil
        )
        #expect(home.otherProjects == ["alpha", "mid", "zeta"])
    }
}
