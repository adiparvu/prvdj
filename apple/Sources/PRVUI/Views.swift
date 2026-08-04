import Foundation
import PRVCore
import PRVKit

// The SwiftUI layer.
//
// # Authored, not verified
//
// SwiftUI is unavailable to the continuous integration this project has, so
// nothing below this line is compiled by any gate (see
// `docs/mts/17-known-limitations.md`, R-01).
//
// That is why the views are as empty as they are. Every string is a key, every
// number is formatted before it arrives, and every decision — what counts as
// busy, what counts as too short to plan, what a version is called — lives in
// `SpaceModels.swift`, which *is* compiled and tested on every commit. A view
// here can be wrong about a colour. It cannot be wrong about the product.

#if canImport(SwiftUI)
    import SwiftUI

    /// The transport, wherever it appears.
    public struct TransportBar: View {
        private let model: TransportModel
        private let onPlay: () -> Void
        private let onPause: () -> Void

        public init(
            model: TransportModel,
            onPlay: @escaping () -> Void,
            onPause: @escaping () -> Void
        ) {
            self.model = model
            self.onPlay = onPlay
            self.onPause = onPause
        }

        public var body: some View {
            HStack(spacing: 12) {
                Button(action: model.isPlaying ? onPause : onPlay) {
                    Image(systemName: model.isPlaying ? "pause.fill" : "play.fill")
                }
                .disabled(model.isBusy)
                .accessibilityLabel(LocalizedStringKey(model.stateKey))

                Text(model.position).monospacedDigit()
                ProgressView(value: model.progress)
                Text(model.duration).monospacedDigit().foregroundStyle(.secondary)

                if model.isBusy {
                    ProgressView().controlSize(.small)
                }
                if model.hasIncompleteAudio {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .foregroundStyle(.orange)
                        .accessibilityLabel(LocalizedStringKey("audio.incomplete"))
                }
            }
            .padding(.horizontal)
        }
    }

    /// One track, in a list.
    public struct TrackRowView: View {
        private let row: TrackRow

        public init(row: TrackRow) {
            self.row = row
        }

        public var body: some View {
            HStack {
                VStack(alignment: .leading) {
                    Text(row.title)
                    Text(row.artist).font(.caption).foregroundStyle(.secondary)
                }
                Spacer()
                if let tempo = row.tempo {
                    Text(tempo).monospacedDigit().foregroundStyle(.secondary)
                }
                if let key = row.key {
                    Text(key).foregroundStyle(.secondary)
                }
                if !row.isPlannable, let reason = row.unplannableReasonKey {
                    // Shown rather than hidden. A track the planner will not use
                    // is not a broken track, and a user who cannot see why will
                    // assume the feature is broken instead.
                    Text(LocalizedStringKey(reason))
                        .font(.caption)
                        .foregroundStyle(.orange)
                }
            }
        }
    }

    /// The library.
    public struct LibrarySpace: View {
        private let model: LibraryModel

        public init(model: LibraryModel) {
            self.model = model
        }

        public var body: some View {
            List {
                if model.isTooSmallToPlan {
                    Label(
                        LocalizedStringKey("library.too_small_to_plan"),
                        systemImage: "info.circle"
                    )
                }
                ForEach(model.rows) { row in
                    TrackRowView(row: row)
                }
            }
        }
    }

    /// The AI Studio: ask for a set, choose between the answers.
    public struct AIStudioSpace: View {
        private let model: PlanningModel
        private let onSelect: (Int) -> Void
        private let onAdopt: (Int) -> Void

        public init(
            model: PlanningModel,
            onSelect: @escaping (Int) -> Void,
            onAdopt: @escaping (Int) -> Void
        ) {
            self.model = model
            self.onSelect = onSelect
            self.onAdopt = onAdopt
        }

        public var body: some View {
            VStack(alignment: .leading) {
                HStack {
                    ForEach(model.options) { option in
                        Button {
                            onSelect(option.id)
                        } label: {
                            VStack(alignment: .leading) {
                                Text(option.name).bold()
                                Text("\(option.trackCount) tracks · \(option.duration)")
                                    .font(.caption)
                                if !option.isCloseEnough {
                                    Text(
                                        "\(option.lengthError) "
                                            + String(localized: "plan.off_target")
                                    )
                                    .font(.caption)
                                    .foregroundStyle(.orange)
                                }
                            }
                        }
                        .buttonStyle(.bordered)
                    }
                }
                .padding(.horizontal)

                List(model.tracklist) { row in
                    TrackRowView(row: row)
                }

                Button(String(localized: "plan.adopt")) {
                    onAdopt(model.selected)
                }
                .padding()
            }
        }
    }

    /// The window: a space picker beside whatever space is chosen.
    public struct StudioWindow: View {
        @State private var space: Space = .home
        @State private var selectedClip: UInt64?
        private let library: LibraryModel
        private let planning: PlanningModel
        private let transport: TransportModel
        private let home: HomeModel
        private let mix: MixEditorModel
        private let live: LiveModel
        private let settings: SettingsModel
        private let onPlay: () -> Void
        private let onPause: () -> Void
        private let onSelect: (Int) -> Void
        private let onAdopt: (Int) -> Void
        private let onRemoveClip: (UInt64) -> Void
        private let onUndo: () -> Void
        private let onConsent: (Purpose, Bool) -> Void

        public init(
            library: LibraryModel,
            planning: PlanningModel,
            transport: TransportModel,
            home: HomeModel,
            mix: MixEditorModel,
            live: LiveModel,
            settings: SettingsModel,
            onPlay: @escaping () -> Void,
            onPause: @escaping () -> Void,
            onSelect: @escaping (Int) -> Void,
            onAdopt: @escaping (Int) -> Void,
            onRemoveClip: @escaping (UInt64) -> Void,
            onUndo: @escaping () -> Void,
            onConsent: @escaping (Purpose, Bool) -> Void
        ) {
            self.library = library
            self.planning = planning
            self.transport = transport
            self.home = home
            self.mix = mix
            self.live = live
            self.settings = settings
            self.onPlay = onPlay
            self.onPause = onPause
            self.onSelect = onSelect
            self.onAdopt = onAdopt
            self.onRemoveClip = onRemoveClip
            self.onUndo = onUndo
            self.onConsent = onConsent
        }

        public var body: some View {
            NavigationSplitView {
                List(Space.allCases, selection: $space) { entry in
                    Label(
                        LocalizedStringKey(entry.titleKey),
                        systemImage: StudioWindow.icon(for: entry)
                    )
                    .tag(entry)
                }
            } detail: {
                VStack {
                    // Every space, with no fall-through. A `default` arm here
                    // would let a space added to the enum quietly render a
                    // placeholder instead of failing to compile — which is how
                    // four of them stayed empty for as long as they did.
                    switch space {
                    case .home:
                        HomeSpace(model: home, onGo: { space = StudioWindow.destination(from: home) })
                    case .library:
                        LibrarySpace(model: library)
                    case .aiStudio:
                        AIStudioSpace(model: planning, onSelect: onSelect, onAdopt: onAdopt)
                    case .mixEditor:
                        MixEditorSpace(
                            model: mix,
                            selection: $selectedClip,
                            onRemove: onRemoveClip,
                            onUndo: onUndo
                        )
                    case .live:
                        LiveSpace(model: live, onPlay: onPlay, onPause: onPause)
                    case .settings:
                        SettingsSpace(model: settings, onToggle: onConsent)
                    }

                    // The live space carries its own transport and needs the
                    // screen; anywhere else the bar sits along the bottom.
                    if space != .live {
                        Divider()
                        TransportBar(model: transport, onPlay: onPlay, onPause: onPause)
                            .padding(.bottom, 8)
                    }
                }
            }
        }

        /// Where the home screen's one suggestion leads.
        ///
        /// The mapping is here rather than in the model because it is
        /// navigation, and navigation is presentation. The model decides *what*
        /// to suggest; this decides where that lives.
        private static func destination(from home: HomeModel) -> Space {
            switch home.nextStepKey {
            case "home.next.import": .library
            case "home.next.plan", "home.next.adopt": .aiStudio
            default: .live
            }
        }

        private static func icon(for space: Space) -> String {
            switch space {
            case .home: "house"
            case .library: "music.note.list"
            case .aiStudio: "wand.and.stars"
            case .mixEditor: "slider.horizontal.3"
            case .live: "waveform"
            case .settings: "gearshape"
            }
        }
    }
#endif
