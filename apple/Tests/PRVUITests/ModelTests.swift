import Foundation
import PRVCore
import PRVKit
import Testing

@testable import PRVUI

@Suite("What the interface shows")
struct PresentationTests {
    private static let rate: UInt32 = 48_000

    @Test("a length reads as prose in a list and as a clock in a transport")
    func durationsReadDifferently() {
        // Two formats on purpose. A performer watching a playhead wants a clock;
        // somebody scanning a list of sets wants prose. One format serving both
        // makes the list unreadable or the clock useless.
        let twoHours = Int64(Self.rate) * 3600 * 2 + Int64(Self.rate) * 60 * 14
        #expect(Duration.describe(frames: twoHours, sampleRate: Self.rate) == "2h 14m")
        #expect(Duration.clock(frames: twoHours, sampleRate: Self.rate) == "2:14:00")

        let fourMinutes = Int64(Self.rate) * (4 * 60 + 32)
        #expect(Duration.describe(frames: fourMinutes, sampleRate: Self.rate) == "4m 32s")
        #expect(Duration.clock(frames: fourMinutes, sampleRate: Self.rate) == "4:32")

        #expect(Duration.describe(frames: 0, sampleRate: Self.rate) == "0s")
        #expect(Duration.clock(frames: -1, sampleRate: Self.rate) == "0:00")
        #expect(Duration.clock(frames: 100, sampleRate: 0) == "0:00", "a zero rate must not divide")
    }

    @Test("a key reads the way a DJ writes it, and absence stays absent")
    func keyNames() {
        #expect(KeyName.describe(semitones: 9, isMinor: true) == "A minor")
        #expect(KeyName.describe(semitones: 0, isMinor: false) == "C major")
        // Wrapping, so a host counting from a different C still names a pitch.
        #expect(KeyName.describe(semitones: 21, isMinor: true) == "A minor")
        #expect(KeyName.describe(semitones: -3, isMinor: false) == "A major")
        // Absent stays absent: the view decides how "unknown" looks.
        #expect(KeyName.describe(semitones: nil, isMinor: true) == nil)
        #expect(KeyName.describe(semitones: 4, isMinor: nil) == nil)
    }

    @Test("buffering shows a spinner and pausing does not")
    func transportDistinguishesBusyFromPaused() {
        // The distinction a performer needs at a glance.
        let busy = TransportModel(
            snapshot: SessionSnapshot(
                playback: .buffering, position: 0, duration: 100,
                placementCount: 1, audioComplete: true
            ),
            sampleRate: Self.rate
        )
        #expect(busy.isBusy)
        #expect(!busy.isPlaying)

        let paused = TransportModel(
            snapshot: SessionSnapshot(
                playback: .paused, position: 0, duration: 100,
                placementCount: 1, audioComplete: true
            ),
            sampleRate: Self.rate
        )
        #expect(!paused.isBusy)
        #expect(paused.stateKey == "playback.paused")
    }

    @Test("an empty timeline does not warn about missing audio")
    func noWarningOnEmptyTimeline() {
        // A set with no placements reports incomplete because it read nothing.
        // True, and not worth a warning badge on an empty timeline.
        let empty = TransportModel(
            snapshot: SessionSnapshot(
                playback: .stopped, position: 0, duration: 0,
                placementCount: 0, audioComplete: false
            ),
            sampleRate: Self.rate
        )
        #expect(!empty.hasIncompleteAudio)

        let real = TransportModel(
            snapshot: SessionSnapshot(
                playback: .playing, position: 0, duration: 100,
                placementCount: 4, audioComplete: false
            ),
            sampleRate: Self.rate
        )
        #expect(real.hasIncompleteAudio)
    }

    @Test("a library too small to plan says so before the user asks for a set")
    func warnsBeforePlanning() {
        // The difference between a product and an error message.
        let items = (0..<2).map {
            MediaItem(
                id: UInt64($0), title: "T\($0)", artist: "A",
                location: URL(fileURLWithPath: "/dev/null")
            )
        }
        let facts: [UInt64: Session.TrackFacts] = [
            0: .init(bpm: 128, isPlannable: true),
            1: .init(bpm: nil, isPlannable: false),
        ]
        let model = LibraryModel(items: items, facts: facts, sampleRate: Self.rate)

        #expect(model.rows.count == 2, "an unplannable track was hidden from the library")
        #expect(model.plannableCount == 1)
        #expect(model.isTooSmallToPlan)
        #expect(model.rows[1].unplannableReasonKey == "track.no_tempo_found")
    }

    @Test("a set that misses its target length is flagged, a close one is not")
    func planOptionsFlagTheirError() {
        let requested = Int64(Self.rate) * 3600
        let close = [PlannedTrack(track: 1, start: 0, duration: requested)]
        let short = [PlannedTrack(track: 1, start: 0, duration: requested / 2)]

        let model = PlanningModel(
            alternatives: [close, short],
            items: [],
            requestedFrames: requested,
            sampleRate: Self.rate
        )
        #expect(model.options.count == 2)
        #expect(model.options[0].isCloseEnough)
        #expect(!model.options[1].isCloseEnough, "a set half the length was not flagged")
        #expect(model.options[1].lengthError == "50%")
        #expect(model.options[0].name == "Version A")
    }

    @Test("selecting an alternative that does not exist does not crash the interface")
    func selectionIsClamped() {
        let model = PlanningModel(
            alternatives: [[PlannedTrack(track: 1, start: 0, duration: 10)]],
            items: [],
            requestedFrames: 10,
            sampleRate: Self.rate,
            selected: 99
        )
        #expect(model.selected == 0)
        #expect(model.tracklist.count == 1)
    }

    @Test("a tracklist uses the library's names, and falls back when it cannot")
    func tracklistNaming() {
        let items = [
            MediaItem(id: 1, title: "Real Name", artist: "A", location: URL(fileURLWithPath: "/x"))
        ]
        let model = PlanningModel(
            alternatives: [[
                PlannedTrack(track: 1, start: 0, duration: 100),
                PlannedTrack(track: 9, start: 100, duration: 100),
            ]],
            items: items,
            requestedFrames: 200,
            sampleRate: Self.rate
        )
        #expect(model.tracklist[0].title == "Real Name")
        #expect(model.tracklist[1].title == "Track 9", "an unknown track had no readable name")
    }

    @Test("every space has a distinct localisation key")
    func spaceKeys() {
        let keys = Set(Space.allCases.map(\.titleKey))
        #expect(keys.count == Space.allCases.count)
        #expect(Space.allCases.count == 6)
        for space in Space.allCases {
            #expect(space.titleKey.hasPrefix("space."))
            #expect(space.purposeKey.hasPrefix("space."))
        }
    }
}
