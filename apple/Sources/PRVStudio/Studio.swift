import Foundation
import PRVCore
import PRVKit
import PRVUI

/// The application's one long-lived object.
///
/// # What belongs here and what does not
///
/// Wiring, and nothing else. It owns the session, the policy and the experience,
/// keeps the view models current, and turns a button press into a call. Every
/// decision it appears to make — whether a library is too small to plan from,
/// what counts as busy, whether a set is far enough off target to flag — was
/// made in a model or in the core, and this only asks.
///
/// The test of that is that this file has no `if` about the product in it.
///
/// `@Observable` only where it exists. On Linux there is no Observation
/// framework and no view to observe, and the headless start reads the same
/// properties directly — which is the point of keeping them plain values.
public final class Studio {
    private var session: Session?
    private var policy: Policy?
    private var experience: Experience?
    private var alternatives: [[PlannedTrack]] = []
    private var requestedFrames: Int64 = 0

    /// Whether the stack came up.
    public private(set) var isReady = false

    /// What went wrong, if anything did.
    public private(set) var failure: String?

    public private(set) var library = LibraryModel(items: [], facts: [:], sampleRate: 48_000)
    public private(set) var planning = PlanningModel(
        alternatives: [], items: [], requestedFrames: 0, sampleRate: 48_000
    )
    public private(set) var transport = TransportModel(
        snapshot: SessionSnapshot(
            playback: .stopped, position: 0, duration: 0,
            placementCount: 0, audioComplete: true
        ),
        sampleRate: 48_000
    )
    public private(set) var home = HomeModel(
        trackCount: 0, projectFrames: 0, sampleRate: 48_000, planOptions: 0
    )
    public private(set) var mix = MixEditorModel(
        clips: [], projectFrames: 0, sampleRate: 48_000
    )
    public private(set) var live = LiveModel(
        transport: TransportModel(
            snapshot: SessionSnapshot(
                playback: .stopped, position: 0, duration: 0,
                placementCount: 0, audioComplete: true
            ),
            sampleRate: 48_000
        ),
        clips: [],
        positionFrames: 0,
        sampleRate: 48_000
    )
    public private(set) var settings = SettingsModel(
        consent: ConsentModel(rows: []),
        sync: SyncModel(
            snapshot: SyncSnapshot.offline
        ),
        tierKey: "tier.free"
    )

    /// Where synchronisation is, for the installation rather than the project.
    private var sync: Sync?

    public init() {}

    /// Whether anything currently leaves the device.
    public var sendsAnything: Bool { policy?.anythingLeavesTheDevice ?? false }

    /// Brings the stack up.
    ///
    /// The version check happens first, inside `Engine`, before any pointer is
    /// exchanged. A failure here is reported rather than thrown, because there
    /// is no caller above this to catch it and a window that opens saying what
    /// is wrong is better than one that does not open.
    public func start() {
        guard session == nil else { return }
        do {
            let output: AudioOutput?
            #if canImport(AVFoundation)
                output = CoreAudioOutput()
            #else
                // No device on this platform. The session still runs; a host
                // renders by hand. That is what makes the headless start a real
                // test rather than a stub.
                output = nil
            #endif

            session = try Session(decoder: InMemoryDecoder(tracks: [:]), output: output)
            policy = try Policy()
            experience = try Experience()
            isReady = true
            refresh()
        } catch {
            failure = String(describing: error)
            isReady = false
        }
    }

    /// Imports a file.
    public func `import`(_ item: MediaItem) {
        guard let session else { return }
        do {
            try session.import(item)
            refresh()
        } catch {
            failure = String(describing: error)
        }
    }

    /// Asks for a set.
    public func plan(minutes: Int, shape: EnergyShape) {
        guard let session else { return }
        do {
            requestedFrames = Int64(minutes) * 60 * Int64(session.sampleRate)
            alternatives = try session.planSet(minutes: minutes, shape: shape)
            refresh()
        } catch {
            failure = String(describing: error)
        }
    }

    /// Chooses which alternative to show.
    public func select(_ index: Int) {
        planning = PlanningModel(
            alternatives: alternatives,
            items: session?.library ?? [],
            requestedFrames: requestedFrames,
            sampleRate: session?.sampleRate ?? 48_000,
            selected: index
        )
    }

    /// Puts an alternative on the timeline.
    public func adopt(_ index: Int) {
        guard let session else { return }
        do {
            try session.adopt(alternative: UInt64(index))
            refresh()
        } catch {
            failure = String(describing: error)
        }
    }

    public func play() {
        guard let session else { return }
        do {
            try session.play()
            refresh()
        } catch {
            failure = String(describing: error)
        }
    }

    public func pause() {
        guard let session else { return }
        do {
            try session.pause()
            refresh()
        } catch {
            failure = String(describing: error)
        }
    }

    /// Tells the core the user is playing to a room.
    ///
    /// Everything that can wait is held from here until ``leftThePerformance()``.
    public func enteredPerformance() {
        try? experience?.setAttention(.performing)
    }

    /// Tells the core the performance is over, and returns what was held.
    @discardableResult
    public func leftThePerformance() -> [HeldNotice] {
        try? experience?.setAttention(.atTheDesk)
        return (try? experience?.release()) ?? []
    }

    /// Rebuilds every view model from one snapshot.
    ///
    /// One snapshot rather than four reads, so the interface cannot show a
    /// playhead from one moment beside a state from another.
    /// Removes a clip from the set.
    public func removeClip(_ placement: UInt64) {
        guard let session else { return }
        do {
            try session.remove(placement: placement)
            refresh()
        } catch {
            failure = String(describing: error)
        }
    }

    /// Undoes the last edit made here.
    ///
    /// A refusal is not a failure: undo declines when another device changed
    /// the same thing afterwards, and the model says so on the screen rather
    /// than reporting an error nobody can act on.
    public func undo() {
        guard let session else { return }
        do {
            try session.undo()
            refresh()
        } catch {
            failure = String(describing: error)
        }
    }

    /// Grants or withdraws one purpose.
    public func setConsent(_ purpose: Purpose, granted: Bool) {
        guard let policy else { return }
        do {
            if granted {
                try policy.grant(purpose, agreementVersion: 1)
            } else {
                try policy.withdraw(purpose)
            }
            refresh()
        } catch {
            failure = String(describing: error)
        }
    }

    private func refresh() {
        guard let session, let snapshot = try? session.snapshot() else { return }
        library = LibraryModel(
            items: session.library,
            facts: session.analyses,
            sampleRate: session.sampleRate
        )
        planning = PlanningModel(
            alternatives: alternatives,
            items: session.library,
            requestedFrames: requestedFrames,
            sampleRate: session.sampleRate
        )
        transport = TransportModel(snapshot: snapshot, sampleRate: session.sampleRate)

        // The timeline, named from the library where it can be. A clip whose
        // track has no name is still drawn and still draggable; the model falls
        // back to its identity rather than to nothing.
        let names = Dictionary(
            session.library.map { ($0.id, $0.title) },
            uniquingKeysWith: { first, _ in first }
        )
        let clips = ((try? session.placements()) ?? []).map { placement in
            ClipModel(placement: placement, title: names[placement.track])
        }

        let syncModel = SyncModel(
            snapshot: (try? sync?.snapshot())
                ?? SyncSnapshot.offline,
            carried: (try? session.carriedCount()) ?? 0
        )

        home = HomeModel(
            trackCount: session.library.count,
            projectFrames: snapshot.duration,
            sampleRate: session.sampleRate,
            planOptions: alternatives.count,
            sync: syncModel
        )
        mix = MixEditorModel(
            clips: clips,
            projectFrames: snapshot.duration,
            sampleRate: session.sampleRate,
            undo: (try? session.undoAvailability()) ?? .nothingToUndo,
            historyLength: (try? session.historyLength()) ?? 0
        )
        live = LiveModel(
            transport: transport,
            clips: clips,
            positionFrames: snapshot.position,
            sampleRate: session.sampleRate,
            renderWasComplete: snapshot.audioComplete
        )
        if let policy {
            settings = SettingsModel(
                consent: (try? ConsentModel(policy: policy)) ?? ConsentModel(rows: []),
                sync: syncModel,
                tierKey: (try? policy.tier)?.titleKey ?? "tier.free"
            )
        }
    }
}
