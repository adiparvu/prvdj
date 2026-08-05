import Foundation
import PRVCore

// The four spaces that had nothing behind them.
//
// # Every value here comes from somewhere
//
// The temptation with an empty screen is to design the thing you wish existed —
// recent projects, collaborators, a feed. Nothing below invents a concept the
// core does not already hold. Where the product has no answer yet, the model
// says so rather than showing a plausible number, because a screen that displays
// something it made up is worse than one that admits a gap.

/// A line on the home screen: one fact, and whether it needs attention.
public struct Standing: Sendable, Equatable, Identifiable {
    /// A stable key for the label.
    public let titleKey: String
    /// What to show beside it. Already formatted, because formatting is a
    /// judgement about who is reading and that belongs to this layer.
    public let value: String
    /// Whether this is the thing to deal with first.
    public let needsAttention: Bool

    public var id: String { titleKey }

    public init(titleKey: String, value: String, needsAttention: Bool = false) {
        self.titleKey = titleKey
        self.value = value
        self.needsAttention = needsAttention
    }
}

/// Where somebody lands, and what to do next.
///
/// # Why this is a list of facts rather than a dashboard
///
/// A dashboard implies numbers worth watching over time, and there are none yet:
/// no history, no sessions, no trends. What a person opening the application
/// actually needs is whether their library is there, whether there is a set, and
/// whether anything is waiting on them. Three questions, answerable today.
public struct HomeModel: Sendable, Equatable {
    public let trackCount: Int
    public let projectFrames: Int64
    public let sampleRate: UInt32
    public let planOptions: Int
    public let sync: SyncModel?
    /// The projects on disk, and which one is open.
    ///
    /// Real, not invented. This screen used to have no way to know a project
    /// existed after the application quit, because nothing saved one; now the
    /// list is what the store actually holds.
    public let projects: [String]
    public let openProject: String?

    public init(
        trackCount: Int,
        projectFrames: Int64,
        sampleRate: UInt32,
        planOptions: Int,
        sync: SyncModel? = nil,
        projects: [String] = [],
        openProject: String? = nil
    ) {
        self.trackCount = trackCount
        self.projectFrames = projectFrames
        self.sampleRate = sampleRate
        self.planOptions = planOptions
        self.sync = sync
        self.projects = projects
        self.openProject = openProject
    }

    /// The projects that could be opened, newest name first and the open one
    /// left out.
    ///
    /// Sorted by name rather than by date, because a modification date is a
    /// fact about the filesystem and this layer does not have one. Naming that
    /// limit is better than sorting by something and implying it means
    /// recency.
    public var otherProjects: [String] {
        projects.filter { $0 != openProject }.sorted()
    }

    /// What the screen shows, in the order it shows it.
    public var standings: [Standing] {
        var rows = [
            Standing(titleKey: "home.library", value: "\(trackCount)"),
            Standing(
                titleKey: "home.project",
                value: projectFrames > 0
                    ? Duration.describe(frames: projectFrames, sampleRate: sampleRate)
                    : "—"
            ),
            Standing(titleKey: "home.plans", value: planOptions > 0 ? "\(planOptions)" : "—"),
        ]
        if let sync {
            rows.append(
                Standing(
                    titleKey: "home.sync",
                    value: sync.waiting > 0 ? "\(sync.waiting)" : "—",
                    needsAttention: sync.needsTheUser || sync.shouldWarnAboutBacklog
                )
            )
        }
        return rows
    }

    /// The one thing to suggest, as a key.
    ///
    /// One, not a list. A screen offering four next steps is a screen that has
    /// not decided, and the person reading it has to decide instead — which is
    /// the work it was supposed to save them.
    public var nextStepKey: String {
        if trackCount == 0 { return "home.next.import" }
        if planOptions == 0 { return "home.next.plan" }
        if projectFrames == 0 { return "home.next.adopt" }
        return "home.next.play"
    }

    /// Whether there is anything at all yet.
    public var isEmpty: Bool { trackCount == 0 && projectFrames == 0 }
}

/// One clip, as a timeline draws it.
public struct ClipModel: Sendable, Equatable, Identifiable {
    public let placement: Placement
    /// What the library calls the track, when it knows.
    public let title: String?

    public var id: UInt64 { placement.id }

    public init(placement: Placement, title: String? = nil) {
        self.placement = placement
        self.title = title
    }

    /// The label, falling back to the identity rather than to nothing.
    ///
    /// A clip whose track has not been named yet still has to be draggable and
    /// still has to be distinguishable from the clip beside it.
    public var label: String { title ?? "track \(placement.track)" }

    /// Where it sits, as a fraction of the project, for laying it out.
    ///
    /// Returns `nil` for an empty project rather than dividing by zero — a
    /// timeline with no length has nowhere to put anything.
    public func span(inProjectOf frames: Int64) -> ClosedRange<Double>? {
        guard frames > 0 else { return nil }
        let start = Double(placement.position) / Double(frames)
        let end = Double(placement.end) / Double(frames)
        return min(start, end)...max(start, end)
    }
}

/// The timeline, and what can be done to it.
public struct MixEditorModel: Sendable, Equatable {
    public let clips: [ClipModel]
    public let projectFrames: Int64
    public let sampleRate: UInt32
    public let undo: UndoAvailability
    /// How many operations the history holds.
    public let historyLength: UInt64

    public init(
        clips: [ClipModel],
        projectFrames: Int64,
        sampleRate: UInt32,
        undo: UndoAvailability = .nothingToUndo,
        historyLength: UInt64 = 0
    ) {
        self.clips = clips
        self.projectFrames = projectFrames
        self.sampleRate = sampleRate
        self.undo = undo
        self.historyLength = historyLength
    }

    /// The clips in the order they are heard.
    ///
    /// The boundary hands them over in identity order, which is what keeps a
    /// list from reordering under the hand dragging it. Drawing wants time
    /// order, so the sort happens here — with identity as the tiebreak, so two
    /// clips starting together do not swap between frames.
    public var inTimeOrder: [ClipModel] {
        clips.sorted {
            $0.placement.position == $1.placement.position
                ? $0.placement.id < $1.placement.id
                : $0.placement.position < $1.placement.position
        }
    }

    /// How many lanes the timeline needs to show.
    ///
    /// At least one, so an empty editor still draws a lane to drop something
    /// into rather than nothing at all.
    public var laneCount: Int {
        Int(clips.map(\.placement.lane).max() ?? 0) + 1
    }

    /// How long the set is, as a person reads it.
    public var durationText: String {
        projectFrames > 0
            ? Duration.describe(frames: projectFrames, sampleRate: sampleRate)
            : "—"
    }

    /// Whether an undo control should be enabled.
    public var canUndo: Bool { undo.isAvailable }

    /// What to say when undo will not do anything, as a key.
    ///
    /// `nil` when it will. "Nothing to undo" is a disabled button; "somebody
    /// else moved this since" is a sentence, and showing the second as the first
    /// leaves a user thinking the application is broken.
    public var undoBlockedKey: String? {
        undo == .supersededByAnotherDevice ? undo.key : nil
    }

    public var isEmpty: Bool { clips.isEmpty }
}

/// What is playing, what is next, and what a performer must not be distracted
/// from.
///
/// # The rule this screen exists to keep
///
/// Master Prompt #19: during a performance, only what concerns the sound coming
/// out right now may interrupt. Everything else waits. This model surfaces
/// exactly two things beyond the transport — whether audio went missing, and
/// what is next — and deliberately carries no notification queue at all.
public struct LiveModel: Sendable, Equatable {
    public let transport: TransportModel
    public let clips: [ClipModel]
    public let positionFrames: Int64
    public let sampleRate: UInt32
    /// Whether every placement the last render touched was read in full.
    public let renderWasComplete: Bool

    public init(
        transport: TransportModel,
        clips: [ClipModel],
        positionFrames: Int64,
        sampleRate: UInt32,
        renderWasComplete: Bool = true
    ) {
        self.transport = transport
        self.clips = clips
        self.positionFrames = positionFrames
        self.sampleRate = sampleRate
        self.renderWasComplete = renderWasComplete
    }

    /// The clip under the playhead.
    public var nowPlaying: ClipModel? {
        clips.first {
            $0.placement.position <= positionFrames && positionFrames < $0.placement.end
        }
    }

    /// The clip that starts next.
    ///
    /// A performer needs this more than anything else on the screen, which is
    /// why it is computed rather than left to the view to find.
    public var upNext: ClipModel? {
        clips
            .filter { $0.placement.position > positionFrames }
            .min { $0.placement.position < $1.placement.position }
    }

    /// How long until the next clip starts, as a person reads it.
    public var timeToNext: String? {
        guard let upNext else { return nil }
        let frames = upNext.placement.position - positionFrames
        guard frames > 0 else { return nil }
        return Duration.clock(frames: frames, sampleRate: sampleRate)
    }

    /// The one thing allowed to interrupt, as a key, or `nil`.
    ///
    /// Missing audio concerns the sound coming out right now, so it qualifies.
    /// Nothing else here does, and nothing else is offered — a list would
    /// invite the next thing to be added to it.
    public var warningKey: String? {
        renderWasComplete ? nil : "live.warning.audio_missing"
    }

    /// Where the playhead is, as a clock.
    public var positionText: String {
        Duration.clock(frames: positionFrames, sampleRate: sampleRate)
    }
}

/// Everything a user can change about the application.
///
/// Consent and synchronisation are the substance of it; both were built before
/// this screen existed and neither is re-decided here.
public struct SettingsModel: Sendable, Equatable {
    public let consent: ConsentModel
    public let sync: SyncModel
    /// The licence tier, as a key.
    public let tierKey: String

    public init(consent: ConsentModel, sync: SyncModel, tierKey: String) {
        self.consent = consent
        self.sync = sync
        self.tierKey = tierKey
    }

    /// The sections, in the order they appear.
    ///
    /// Privacy first, deliberately. It is the section a worried person came for,
    /// and putting the licence above it would say something about priorities
    /// that this product does not mean.
    public var sectionKeys: [String] {
        ["settings.privacy", "settings.sync", "settings.licence"]
    }

    /// Whether anything on this screen wants attention.
    public var needsAttention: Bool {
        sync.needsTheUser || sync.shouldWarnAboutBacklog || sync.needsANewerVersion
    }
}
