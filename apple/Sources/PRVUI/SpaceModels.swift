import Foundation
import PRVCore
import PRVKit

/// The six places the application can be.
///
/// A closed set, in the order the navigation spec lays them out. Closed because
/// a space is a mode with its own toolbar and its own idea of what the user is
/// doing, and a product that grows spaces by accident grows a menu nobody reads.
public enum Space: String, CaseIterable, Sendable, Identifiable {
    case home
    case library
    case aiStudio
    case mixEditor
    case live
    case settings

    public var id: String { rawValue }

    /// A stable identifier for localisation.
    ///
    /// The English is a fallback, not the string a user sees. Returning a key
    /// rather than a sentence is what keeps the presentation layer free of
    /// copy that has to be found and replaced when it is translated.
    public var titleKey: String { "space.\(rawValue)" }

    /// What this space is for, in one line.
    public var purposeKey: String { "space.\(rawValue).purpose" }
}

/// How a length reads to a person.
///
/// # Why this is here and not in the core
///
/// It is presentation, and the architecture overview says presentation holds no
/// business rules. Formatting a duration is not a rule — but deciding that a set
/// is "2h 14m" rather than "134 minutes" is a judgement about who is reading,
/// and that belongs to the layer that knows.
public enum Duration {
    /// Frames to a compact human reading: `2h 14m`, `14m 03s`, `43s`.
    public static func describe(frames: Int64, sampleRate: UInt32) -> String {
        guard sampleRate > 0, frames > 0 else { return "0s" }
        let total = Int(frames / Int64(sampleRate))
        let hours = total / 3_600
        let minutes = (total % 3_600) / 60
        let seconds = total % 60

        if hours > 0 { return "\(hours)h \(minutes)m" }
        if minutes > 0 { return String(format: "%dm %02ds", minutes, seconds) }
        return "\(seconds)s"
    }

    /// Frames to a clock reading for a transport display: `1:04:12`, `4:12`.
    ///
    /// Different from ``describe(frames:sampleRate:)`` on purpose. A performer
    /// watching a playhead wants a clock; somebody scanning a list of sets wants
    /// prose. Using one for both makes the list unreadable or the clock useless.
    public static func clock(frames: Int64, sampleRate: UInt32) -> String {
        guard sampleRate > 0 else { return "0:00" }
        let total = Int(max(0, frames) / Int64(sampleRate))
        let hours = total / 3_600
        let minutes = (total % 3_600) / 60
        let seconds = total % 60

        if hours > 0 { return String(format: "%d:%02d:%02d", hours, minutes, seconds) }
        return String(format: "%d:%02d", minutes, seconds)
    }
}

/// How a key reads to a DJ.
public enum KeyName {
    private static let names = [
        "C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B",
    ]

    /// `A minor`, or `nil` when nothing was detected.
    ///
    /// Nil rather than a dash, so the caller decides how absence looks. A view
    /// that wants "—" writes "—"; one that wants to hide the column can.
    public static func describe(semitones: Int32?, isMinor: Bool?) -> String? {
        guard let semitones, let isMinor else { return nil }
        let index = Int(semitones.quotientAndRemainder(dividingBy: 12).remainder + 12) % 12
        guard index < names.count else { return nil }
        return "\(names[index]) \(isMinor ? "minor" : "major")"
    }
}

/// One row of a tracklist, ready to draw.
///
/// Everything a view needs and nothing it has to compute. A view that formatted
/// its own durations would be a view with a rule in it.
public struct TrackRow: Sendable, Equatable, Identifiable {
    public var id: UInt64
    public var title: String
    public var artist: String
    /// `128.0 BPM`, or `nil` when no tempo was found.
    public var tempo: String?
    /// `A minor`, or `nil`.
    public var key: String?
    /// `4m 32s`.
    public var duration: String
    /// Whether the planner can choose this track.
    public var isPlannable: Bool
    /// Why not, when it cannot. A key an interface turns into a sentence.
    public var unplannableReasonKey: String?
}

/// What the library space shows.
public struct LibraryModel: Sendable, Equatable {
    public var rows: [TrackRow]
    /// How many tracks the planner can actually choose from.
    public var plannableCount: Int
    /// Whether to warn that too little of the library is usable.
    ///
    /// The threshold is a presentation decision: below a handful of usable
    /// tracks the planner will refuse, and telling the user *before* they ask
    /// for a two-hour set is the difference between a product and an error
    /// message.
    public var isTooSmallToPlan: Bool

    /// The fewest usable tracks worth attempting a set from.
    public static let minimumUsable = 3

    public init(items: [MediaItem], facts: [UInt64: Session.TrackFacts], sampleRate: UInt32) {
        rows = items.map { item in
            let fact = facts[item.id]
            let plannable = fact?.isPlannable ?? false
            return TrackRow(
                id: item.id,
                title: item.title,
                artist: item.artist,
                tempo: fact?.bpm.map { String(format: "%.1f BPM", $0) },
                key: KeyName.describe(
                    semitones: fact?.keySemitones,
                    isMinor: fact?.keyIsMinor
                ),
                duration: "—",
                isPlannable: plannable,
                unplannableReasonKey: plannable
                    ? nil
                    : (fact?.bpm == nil
                        ? "track.no_tempo_found"
                        : "track.too_short_to_analyse")
            )
        }
        plannableCount = rows.count { $0.isPlannable }
        isTooSmallToPlan = plannableCount < Self.minimumUsable
    }
}

/// What the transport shows, wherever it appears.
public struct TransportModel: Sendable, Equatable {
    /// `4:12`.
    public var position: String
    /// `1:04:12`.
    public var duration: String
    /// Zero to one.
    public var progress: Double
    /// Whether the play control should read as playing.
    public var isPlaying: Bool
    /// Whether to show a spinner instead of a paused badge.
    ///
    /// The distinction a performer needs at a glance: buffering is the transport
    /// on its way somewhere without help; paused is waiting for a person.
    public var isBusy: Bool
    /// Whether to warn that some audio was missing from the last render.
    public var hasIncompleteAudio: Bool
    /// A key for the state, for a label and for tests.
    public var stateKey: String

    public init(snapshot: SessionSnapshot, sampleRate: UInt32) {
        position = Duration.clock(frames: snapshot.position, sampleRate: sampleRate)
        duration = Duration.clock(frames: snapshot.duration, sampleRate: sampleRate)
        progress = snapshot.progress
        isPlaying = snapshot.playback.isAudible
        isBusy = snapshot.playback.isTransient
        // Only worth showing while there is something to play. A set with no
        // placements reports incomplete because it read nothing, which is true
        // and not worth a warning badge on an empty timeline.
        hasIncompleteAudio = !snapshot.audioComplete && snapshot.placementCount > 0
        stateKey = TransportModel.key(for: snapshot.playback)
    }

    private static func key(for state: PlaybackState) -> String {
        switch state {
        case .stopped: "playback.stopped"
        case .loading: "playback.loading"
        case .ready: "playback.ready"
        case .playing: "playback.playing"
        case .paused: "playback.paused"
        case .seeking: "playback.seeking"
        case .buffering: "playback.buffering"
        case .recovering: "playback.recovering"
        case .error: "playback.error"
        case .unrecognised: "playback.unknown"
        }
    }
}

/// One planned alternative, ready to show as a choice.
public struct PlanOption: Sendable, Equatable, Identifiable {
    public var id: Int
    /// `Version A`, `Version B`, `Version C`.
    public var name: String
    public var trackCount: Int
    /// `1h 58m`.
    public var duration: String
    /// How far from what was asked for, as a percentage a user can read.
    public var lengthError: String
    /// Whether this alternative is close enough to be offered without a caveat.
    public var isCloseEnough: Bool

    /// Above this fraction, a set is worth flagging as short or long.
    ///
    /// A tenth. A two-hour set that comes out twelve minutes off is something a
    /// user should be told about before they play it; two minutes is not.
    public static let acceptableError = 0.1
}

/// What the AI Studio space shows after planning.
public struct PlanningModel: Sendable, Equatable {
    public var options: [PlanOption]
    /// The tracklist of whichever option is selected.
    public var tracklist: [TrackRow]
    public var selected: Int

    public init(
        alternatives: [[PlannedTrack]],
        items: [MediaItem],
        requestedFrames: Int64,
        sampleRate: UInt32,
        selected: Int = 0
    ) {
        let names = ["Version A", "Version B", "Version C"]
        options = alternatives.enumerated().map { index, tracks in
            let end = tracks.map { $0.start + $0.duration }.max() ?? 0
            let error =
                requestedFrames > 0
                ? abs(Double(end - requestedFrames) / Double(requestedFrames)) : 0
            return PlanOption(
                id: index,
                name: index < names.count ? names[index] : "Version \(index + 1)",
                trackCount: tracks.count,
                duration: Duration.describe(frames: end, sampleRate: sampleRate),
                lengthError: String(format: "%.0f%%", error * 100),
                isCloseEnough: error <= PlanOption.acceptableError
            )
        }

        self.selected = min(max(0, selected), max(0, alternatives.count - 1))
        let chosen =
            alternatives.indices.contains(self.selected) ? alternatives[self.selected] : []
        let byId = Dictionary(uniqueKeysWithValues: items.map { ($0.id, $0) })
        tracklist = chosen.map { planned in
            let item = byId[planned.track]
            return TrackRow(
                id: planned.track,
                title: item?.title ?? "Track \(planned.track)",
                artist: item?.artist ?? "",
                tempo: nil,
                key: nil,
                duration: Duration.describe(frames: planned.duration, sampleRate: sampleRate),
                isPlannable: true,
                unplannableReasonKey: nil
            )
        }
    }
}
